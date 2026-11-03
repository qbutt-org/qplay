#include "qplay_native/tensorrt.h"

#include <algorithm>
#include <array>
#include <climits>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include <QLibrary>
#include <QString>

#ifdef QPLAY_TENSORRT
#include "NvInfer.h"
#include "NvOnnxParser.h"
#endif

#include "qplay_native/src/tensorrt.cxx.h"

namespace Qplay::TensorRt
{
#ifdef QPLAY_TENSORRT
namespace
{
class Library
{
public:
    explicit Library(const std::filesystem::path &path, QLibrary::LoadHints hints = {})
        : m_library {QString::fromStdU16String(path.u16string())}
    {
        m_library.setLoadHints(hints);
        if (!m_library.load())
            throw std::runtime_error("Cannot load a verified GPU library: " + m_library.errorString().toStdString());
    }

    ~Library()
    {
        m_library.unload();
    }

    Library(const Library &) = delete;
    Library &operator=(const Library &) = delete;

    template <typename Function>
    Function symbol(const char *name) const
    {
        const auto address = m_library.resolve(name);
        if (!address)
            throw std::runtime_error(std::string("Missing native GPU API: ") + name);
        return reinterpret_cast<Function>(address);
    }

private:
    mutable QLibrary m_library;
};

class CudaContext
{
public:
    explicit CudaContext(std::uint32_t ordinal)
#ifdef _WIN32
        : m_library {std::filesystem::path(qEnvironmentVariable("SystemRoot").toStdU16String()) / "System32/nvcuda.dll"}
#else
        : m_library {"libcuda.so.1"}
#endif
    {
        const auto init = m_library.symbol<int (*)(unsigned int)>("cuInit");
        const auto getDevice = m_library.symbol<int (*)(int *, int)>("cuDeviceGet");
        const auto create = m_library.symbol<int (*)(void **, unsigned int, int)>("cuCtxCreate_v2");
        m_destroy = m_library.symbol<int (*)(void *)>("cuCtxDestroy_v2");
        int device = 0;
        if (init(0) || getDevice(&device, static_cast<int>(ordinal)))
            throw std::runtime_error("The selected CUDA device is unavailable.");
        if (create(&m_context, 0, device))
            throw std::runtime_error("Cannot create the preparation CUDA context.");
    }

    ~CudaContext()
    {
        // Destroy only our context. CUDA also restores the caller's previous
        // current context; the decoder's shared primary context is untouched.
        m_destroy(m_context);
    }

private:
    Library m_library;
    void *m_context = nullptr;
    int (*m_destroy)(void *) = nullptr;
};

class Logger final : public nvinfer1::ILogger
{
public:
    void clear()
    {
        const std::lock_guard lock {m_mutex};
        m_error.clear();
    }

    void log(Severity severity, const char *message) noexcept override
    {
        if (severity > Severity::kERROR || !message)
            return;
        try
        {
            const std::lock_guard lock {m_mutex};
            m_error.assign(message, std::min<std::size_t>(std::strlen(message), 4096));
        }
        catch (...) {}
    }

    std::runtime_error failure(const char *operation)
    {
        const std::lock_guard lock {m_mutex};
        return std::runtime_error(std::string(operation) + (m_error.empty() ? " failed." : ": " + m_error));
    }

private:
    std::mutex m_mutex;
    std::string m_error;
};

#ifndef _WIN32
// TensorRT 10.16 keeps internal library owners until process finalization.
// Their dlclose can unmap executing nvinfer code, so its Linux mapping and
// logger must outlive those callbacks. GPU objects and contexts remain scoped.
Logger processLogger;
#endif

class Progress final : public nvinfer1::IProgressMonitor
{
public:
    explicit Progress(const BuildControl &control)
        : m_control {control}
    {
    }

    void phaseStart(const char *, const char *, int32_t) noexcept override {}
    void phaseFinish(const char *) noexcept override {}

    bool stepComplete(const char *, int32_t) noexcept override
    {
        return !engine_build_cancelled(m_control);
    }

private:
    const BuildControl &m_control;
};

std::filesystem::path path(rust::Str value)
{
    return std::filesystem::u8path(value.begin(), value.end());
}

bool matches(const nvinfer1::Dims &shape, std::uint32_t channels, std::uint32_t width, std::uint32_t height)
{
    return shape.nbDims == 4 && shape.d[0] == 1 && shape.d[1] == channels
        && shape.d[2] == height && shape.d[3] == width;
}

void validate(nvinfer1::IRuntime &runtime, const void *bytes, std::size_t size,
    std::uint32_t width, std::uint32_t height, Logger &logger)
{
    const std::unique_ptr<nvinfer1::ICudaEngine> engine {runtime.deserializeCudaEngine(bytes, size)};
    if (!engine)
        throw logger.failure("Deserialize TensorRT engine");
    if (engine->getNbIOTensors() != 2)
        throw std::runtime_error("RIFE requires one input and one output tensor.");
    bool input = false;
    bool output = false;
    for (int index = 0; index < engine->getNbIOTensors(); ++index)
    {
        const char *name = engine->getIOTensorName(index);
        const bool isInput = engine->getTensorIOMode(name) == nvinfer1::TensorIOMode::kINPUT;
        if (!matches(engine->getTensorShape(name), isInput ? 7 : 3, width, height)
            || engine->getTensorDataType(name) != nvinfer1::DataType::kHALF
            || engine->getTensorFormat(name) != nvinfer1::TensorFormat::kLINEAR)
        {
            throw std::runtime_error("The RIFE engine has an incompatible tensor geometry or type.");
        }
        input |= isInput;
        output |= !isInput;
    }
    if (!input || !output)
        throw std::runtime_error("The RIFE engine has invalid input/output bindings.");
}
}
#endif

rust::Vec<std::uint8_t> build_engine(rust::Str directory,
    rust::Slice<const rust::String> resources, rust::Str model,
    const std::uint32_t device, const std::uint32_t width, const std::uint32_t height,
    const BuildControl &control)
{
#ifdef QPLAY_TENSORRT
    if (engine_build_cancelled(control))
        throw std::runtime_error("Engine preparation was cancelled.");
    if (!width || !height || width > INT32_MAX || height > INT32_MAX)
        throw std::runtime_error("The source geometry cannot be represented by TensorRT.");
    const CudaContext cuda {device};
#ifdef _WIN32
    Logger logger;
    const Library infer {path(directory) / "nvinfer_10.dll"};
    const Library parserLibrary {path(directory) / "nvonnxparser_10.dll"};
    // TensorRT opens its architecture resources by basename while building.
    // Preload the pinned closure by absolute path without changing DLL search
    // directories for the rest of the application.
    std::vector<std::unique_ptr<Library>> builderResources;
    for (const rust::String &resource : resources)
        builderResources.push_back(std::make_unique<Library>(path(rust::Str(resource))));
#else
    Logger &logger = processLogger;
    logger.clear();
    const Library infer {path(directory) / "libnvinfer.so.10", QLibrary::PreventUnloadHint};
    const Library parserLibrary {path(directory) / "libnvonnxparser.so.10"};
#endif
    Progress progress {control};
    const auto createBuilder = infer.symbol<void *(*)(void *, int32_t)>("createInferBuilder_INTERNAL");
    const auto createRuntime = infer.symbol<void *(*)(void *, int32_t)>("createInferRuntime_INTERNAL");
    const auto createParser = parserLibrary.symbol<void *(*)(void *, void *, int32_t)>("createNvOnnxParser_INTERNAL");
    const std::unique_ptr<nvinfer1::IBuilder> builder {
        static_cast<nvinfer1::IBuilder *>(createBuilder(&logger, NV_TENSORRT_VERSION))};
    if (!builder)
        throw logger.failure("Create TensorRT builder");
    // Worker threads would lack our current context and retain CUDA's shared
    // primary context. Keep building on this thread so teardown stays scoped.
    builder->setMaxThreads(1);
    const std::unique_ptr<nvinfer1::INetworkDefinition> network {builder->createNetworkV2(0)};
    const std::unique_ptr<nvinfer1::IBuilderConfig> config {builder->createBuilderConfig()};
    if (!network || !config)
        throw logger.failure("Create TensorRT network configuration");
    const std::unique_ptr<nvonnxparser::IParser> parser {
        static_cast<nvonnxparser::IParser *>(createParser(network.get(), &logger, NV_ONNX_PARSER_VERSION))};
    // Open the Unicode filesystem path ourselves; TensorRT's narrow filename API
    // cannot represent every Windows profile directory. The pinned model embeds
    // its weights, so parsing the serialized model needs no external-data path.
    std::ifstream input {path(model), std::ios::binary};
    if (!input)
        throw std::runtime_error("Cannot open the pinned RIFE model.");
    const std::vector<char> modelBytes {std::istreambuf_iterator<char> {input}, std::istreambuf_iterator<char> {}};
    if (input.bad())
        throw std::runtime_error("Cannot read the pinned RIFE model.");
    if (!parser || !parser->parse(modelBytes.data(), modelBytes.size()))
        throw logger.failure("Parse the pinned RIFE model");
    if (network->getNbInputs() != 1 || network->getNbOutputs() != 1)
        throw std::runtime_error("The RIFE model requires one input and one output.");
    const nvinfer1::Dims4 shape {1, 7, static_cast<int>(height), static_cast<int>(width)};
    network->getInput(0)->setDimensions(shape);
    network->getInput(0)->setType(nvinfer1::DataType::kHALF);
    network->getOutput(0)->setType(nvinfer1::DataType::kHALF);
    const auto planar = 1U << static_cast<unsigned int>(nvinfer1::TensorFormat::kLINEAR);
    network->getInput(0)->setAllowedFormats(planar);
    network->getOutput(0)->setAllowedFormats(planar);

    // Preserve vs-mlrt v15.16 RIFEMerge v2's precision constraints. These
    // coordinate operations must stay FP32 to avoid overflow and NaN frames.
    const auto isNonActivationType = [](nvinfer1::DataType type)
    {
        return type == nvinfer1::DataType::kINT32 || type == nvinfer1::DataType::kINT64
            || type == nvinfer1::DataType::kBOOL || type == nvinfer1::DataType::kUINT8;
    };
    constexpr std::array<std::string_view, 16> preciseLayers {
        "/Cast_2", "/Cast_3", "/Cast_5", "/Cast_7", "/Reciprocal", "/Reciprocal_1",
        "/Mul", "/Mul_1", "/Mul_8", "/Mul_10", "/Sub_5", "/Sub_6",
        "ONNXTRT_Broadcast_236", "ONNXTRT_Broadcast_238",
        "ONNXTRT_Broadcast_273", "ONNXTRT_Broadcast_275"
    };
    constexpr std::string_view broadcastPrefix {"ONNXTRT_Broadcast_"};
    for (int index = 0; index < network->getNbLayers(); ++index)
    {
        auto *layer = network->getLayer(index);
        const std::string_view name {layer->getName()};
        if (std::find(preciseLayers.begin(), preciseLayers.end(), name) != preciseLayers.end())
        {
            layer->setPrecision(nvinfer1::DataType::kFLOAT);
            continue;
        }
        if (name.compare(0, broadcastPrefix.size(), broadcastPrefix) != 0)
            continue;

        // Match the pinned trtexec setLayerPrecisions wildcard rules: shape
        // and index computations retain their original integer precision.
        if (isNonActivationType(layer->getPrecision()))
            continue;
        if (layer->getType() == nvinfer1::LayerType::kCONSTANT
            && isNonActivationType(static_cast<nvinfer1::IConstantLayer *>(layer)->getWeights().type))
        {
            continue;
        }
        if (layer->getNbInputs() > 0 && layer->getInput(0)->isShapeTensor())
            continue;
        if (layer->getNbInputs() > 0 && layer->getNbOutputs() > 0
            && isNonActivationType(layer->getInput(0)->getType()) && isNonActivationType(layer->getOutput(0)->getType()))
        {
            continue;
        }
        layer->setPrecision(nvinfer1::DataType::kFLOAT);
    }
    config->setFlag(nvinfer1::BuilderFlag::kFP16);
    config->setFlag(nvinfer1::BuilderFlag::kOBEY_PRECISION_CONSTRAINTS);
    config->clearFlag(nvinfer1::BuilderFlag::kTF32);
    config->setBuilderOptimizationLevel(5);
    config->setProgressMonitor(&progress);
    if (engine_build_cancelled(control))
        throw std::runtime_error("Engine preparation was cancelled.");
    const std::unique_ptr<nvinfer1::IHostMemory> plan {builder->buildSerializedNetwork(*network, *config)};
    if (engine_build_cancelled(control))
        throw std::runtime_error("Engine preparation was cancelled.");
    if (!plan)
        throw logger.failure("Build TensorRT engine");
    const std::unique_ptr<nvinfer1::IRuntime> runtime {
        static_cast<nvinfer1::IRuntime *>(createRuntime(&logger, NV_TENSORRT_VERSION))};
    if (!runtime)
        throw logger.failure("Create TensorRT validation runtime");
    validate(*runtime, plan->data(), plan->size(), width, height, logger);
    rust::Vec<std::uint8_t> result;
    result.reserve(plan->size());
    const auto *bytes = static_cast<const std::uint8_t *>(plan->data());
    std::copy(bytes, bytes + plan->size(), std::back_inserter(result));
    return result;
#else
    throw std::runtime_error("TensorRT preparation is unavailable on this platform.");
#endif
}

void validate_engine(rust::Str directory, rust::Slice<const std::uint8_t> engine,
    const std::uint32_t device, const std::uint32_t width, const std::uint32_t height)
{
#ifdef QPLAY_TENSORRT
    const CudaContext cuda {device};
#ifdef _WIN32
    Logger logger;
    const Library infer {path(directory) / "nvinfer_10.dll"};
#else
    Logger &logger = processLogger;
    logger.clear();
    const Library infer {path(directory) / "libnvinfer.so.10", QLibrary::PreventUnloadHint};
#endif
    const auto createRuntime = infer.symbol<void *(*)(void *, int32_t)>("createInferRuntime_INTERNAL");
    const std::unique_ptr<nvinfer1::IRuntime> runtime {
        static_cast<nvinfer1::IRuntime *>(createRuntime(&logger, NV_TENSORRT_VERSION))};
    if (!runtime)
        throw logger.failure("Create TensorRT validation runtime");
    validate(*runtime, engine.data(), engine.size(), width, height, logger);
#else
    throw std::runtime_error("TensorRT preparation is unavailable on this platform.");
#endif
}
}
