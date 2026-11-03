#include "qplay_native/framegen_graph.h"
#include "qplay_native/src/framegen_graph.cxx.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <utility>

namespace
{
using Qplay::Framegen::GraphConfig;
using Qplay::Framegen::GraphLease;
using Qplay::Framegen::Sample;

template <auto Free>
struct VsFree
{
    const VSAPI *api;

    template <typename T>
    void operator()(T *value) const { (api->*Free)(value); }
};

using Node = std::unique_ptr<VSNode, VsFree<&VSAPI::freeNode>>;
using Frame = std::unique_ptr<const VSFrame, VsFree<&VSAPI::freeFrame>>;
using Map = std::unique_ptr<VSMap, VsFree<&VSAPI::freeMap>>;
using Function = std::unique_ptr<VSFunction, VsFree<&VSAPI::freeFunction>>;

class Builder
{
public:
    const VSAPI *api;
    VSCore *core;

    Map arguments() const { return Map(api->createMap(), {api}); }

    Map invoke(const char *pluginNamespace, const char *name, const Map &args) const
    {
        VSPlugin *plugin = api->getPluginByNamespace(pluginNamespace, core);
        if (!plugin)
            throw std::runtime_error(std::string("VapourSynth plugin is unavailable: ") + pluginNamespace);

        Map result(api->invoke(plugin, name, args.get()), {api});
        if (const char *error = api->mapGetError(result.get()))
            throw std::runtime_error(std::string(pluginNamespace) + "." + name + ": " + error);
        return result;
    }

    Node invokeNode(const char *pluginNamespace, const char *name, const Map &args) const
    {
        Map result = invoke(pluginNamespace, name, args);
        Node node(api->mapGetNode(result.get(), "clip", 0, nullptr), {api});
        if (!node)
            throw std::runtime_error(std::string(pluginNamespace) + "." + name + " returned no video node.");
        return node;
    }

    Node resize(VSNode *input, int format, int matrix, bool fullRange,
        int width = 0, int height = 0, bool point = false) const
    {
        Map args = arguments();
        api->mapSetNode(args.get(), "clip", input, maReplace);
        api->mapSetInt(args.get(), "format", format, maReplace);
        if (width)
        {
            api->mapSetInt(args.get(), "width", width, maReplace);
            api->mapSetInt(args.get(), "height", height, maReplace);
        }
        const auto *source = api->getVideoInfo(input);
        VSVideoFormat destination;
        if (!api->getVideoFormatByID(&destination, format, core))
            throw std::runtime_error("VapourSynth rejected the frame generation format.");

        if (source->format.colorFamily != destination.colorFamily)
        {
            const bool fromRgb = source->format.colorFamily == cfRGB;
            api->mapSetInt(args.get(), fromRgb ? "matrix" : "matrix_in", matrix, maReplace);
            api->mapSetInt(args.get(), fromRgb ? "range" : "range_in", fullRange ? 0 : 1, maReplace);
        }
        if (point)
        {
            api->mapSetInt(args.get(), "range", 0, maReplace);
            api->mapSetData(args.get(), "dither_type", "none", -1, dtUtf8, maReplace);
        }
        return invokeNode("resize", point ? "Point" : "Bicubic", args);
    }
};

struct ModelPadding
{
    Node source;
    int width;
    int height;
};

const VSFrame *VS_CC paddingFrame(int n, int reason, void *instance, void **,
    VSFrameContext *context, VSCore *core, const VSAPI *api)
{
    const auto &padding = *static_cast<ModelPadding *>(instance);
    if (reason == arInitial)
    {
        api->requestFrameFilter(n, padding.source.get(), context);
        return nullptr;
    }
    if (reason != arAllFramesReady)
        return nullptr;
    Frame source(api->getFrameFilter(n, padding.source.get(), context), {api});
    if (!source)
        return nullptr;
    const VSVideoFormat *format = api->getVideoFrameFormat(source.get());
    VSFrame *frame = api->newVideoFrame(format, padding.width, padding.height, source.get(), core);
    const int sourceWidth = api->getFrameWidth(source.get(), 0);
    const int sourceHeight = api->getFrameHeight(source.get(), 0);
    const auto pixelBytes = static_cast<size_t>(format->bytesPerSample);
    for (int plane = 0; plane < format->numPlanes; ++plane)
    {
        auto *pixels = api->getWritePtr(frame, plane);
        const auto stride = api->getStride(frame, plane);
        for (int y = 0; y < sourceHeight; ++y)
        {
            const auto *original = api->getReadPtr(source.get(), plane) + y * api->getStride(source.get(), plane);
            auto *row = pixels + y * stride;
            std::memcpy(row, original, static_cast<size_t>(sourceWidth) * pixelBytes);
            for (int x = sourceWidth; x < padding.width; ++x)
                std::memcpy(row + x * pixelBytes, original + (sourceWidth - 1) * pixelBytes, pixelBytes);
        }
        for (int y = sourceHeight; y < padding.height; ++y)
            std::memcpy(pixels + y * stride, pixels + (sourceHeight - 1) * stride,
                static_cast<size_t>(padding.width) * pixelBytes);
    }
    return frame;
}

void VS_CC freePadding(void *instance, VSCore *, const VSAPI *)
{
    delete static_cast<ModelPadding *>(instance);
}

Node padModelInput(const Builder &builder, VSNode *input, int width, int height)
{
    VSVideoInfo info = *builder.api->getVideoInfo(input);
    if (info.width == width && info.height == height)
        return Node(builder.api->addNodeRef(input), {builder.api});
    info.width = width;
    info.height = height;
    auto padding = std::make_unique<ModelPadding>(ModelPadding {
        Node(builder.api->addNodeRef(input), {builder.api}), width, height});
    const VSFilterDependency dependency {input, rpStrictSpatial};
    Node node(builder.api->createVideoFilter2("QplayModelEdgePadding", &info, paddingFrame,
        freePadding, fmParallel, &dependency, 1, padding.get(), builder.core), {builder.api});
    if (!node)
        throw std::runtime_error("Could not create the native model padding.");
    padding.release();
    return node;
}

struct Timeline
{
    uint64_t numerator;
    uint64_t denominator;

    Sample position(int n) const
    {
        return Qplay::Framegen::sample_position(static_cast<uint32_t>(n), numerator, denominator);
    }
};

struct Sampler
{
    Node source;
    Timeline timeline;
    int offset;
};

const VSFrame *VS_CC sampleFrame(int n, int reason, void *instance, void **,
    VSFrameContext *context, VSCore *, const VSAPI *api)
{
    const auto &sampler = *static_cast<Sampler *>(instance);
    const int index = sampler.timeline.position(n).index + sampler.offset;
    if (reason == arInitial)
        api->requestFrameFilter(index, sampler.source.get(), context);
    else if (reason == arAllFramesReady)
        return api->getFrameFilter(index, sampler.source.get(), context);
    return nullptr;
}

void VS_CC freeSampler(void *instance, VSCore *, const VSAPI *)
{
    delete static_cast<Sampler *>(instance);
}

Node sampleNode(const Builder &builder, VSNode *source, const VSVideoInfo &outputInfo,
    Timeline timeline, int offset)
{
    auto sampler = std::make_unique<Sampler>(Sampler {
        Node(builder.api->addNodeRef(source), {builder.api}), timeline, offset});
    VSVideoInfo info = *builder.api->getVideoInfo(source);
    info.fpsNum = outputInfo.fpsNum;
    info.fpsDen = outputInfo.fpsDen;
    info.numFrames = outputInfo.numFrames;
    const VSFilterDependency dependency {source, rpGeneral};
    Node node(builder.api->createVideoFilter2("QplayRifeSample", &info, sampleFrame,
        freeSampler, fmParallel, &dependency, 1, sampler.get(), builder.core), {builder.api});
    if (!node)
        throw std::runtime_error("Could not create the native frame generation sampler.");
    sampler.release();
    return node;
}

struct TimeMask
{
    // Model releases this input inside its GPU teardown callback. VS79 defers
    // nested free callbacks, so freeMask drops the lease after teardown returns.
    rust::Box<GraphLease> lease;
    VSVideoInfo info;
    Timeline timeline;
};

const VSFrame *VS_CC maskFrame(int n, int reason, void *instance, void **,
    VSFrameContext *, VSCore *core, const VSAPI *api)
{
    if (reason != arInitial)
        return nullptr;
    const auto &mask = *static_cast<TimeMask *>(instance);
    const Sample position = mask.timeline.position(n);
    const float fraction = static_cast<float>(static_cast<double>(position.remainder)
        / static_cast<double>(mask.timeline.denominator));
    VSFrame *frame = api->newVideoFrame(&mask.info.format, mask.info.width,
        mask.info.height, nullptr, core);
    auto *pixels = api->getWritePtr(frame, 0);
    const ptrdiff_t stride = api->getStride(frame, 0);
    for (int y = 0; y < mask.info.height; ++y)
    {
        auto *row = reinterpret_cast<float *>(pixels + y * stride);
        std::fill_n(row, mask.info.width, fraction);
    }
    return frame;
}

void VS_CC freeMask(void *instance, VSCore *, const VSAPI *)
{
    delete static_cast<TimeMask *>(instance);
}

Node maskNode(const Builder &builder, VSVideoInfo info, Timeline timeline,
    rust::Box<GraphLease> lease)
{
    builder.api->getVideoFormatByID(&info.format, pfGrayS, builder.core);
    auto mask = std::make_unique<TimeMask>(TimeMask {std::move(lease), info, timeline});
    Node node(builder.api->createVideoFilter2("QplayRifeTimepoint", &info, maskFrame,
        freeMask, fmParallel, nullptr, 0, mask.get(), builder.core), {builder.api});
    if (!node)
        throw std::runtime_error("Could not create the native frame generation timepoint.");
    mask.release();
    return node;
}

void VS_CC openvinoConfig(const VSMap *, VSMap *output, void *instance,
    VSCore *, const VSAPI *api)
{
    const auto &cache = *static_cast<std::string *>(instance);
    api->mapSetInt(output, "NUM_STREAMS", 4, maReplace);
    api->mapSetData(output, "INFERENCE_PRECISION_HINT", "f16", -1, dtUtf8, maReplace);
    api->mapSetData(output, "CACHE_DIR", cache.c_str(), -1, dtUtf8, maReplace);
}

void VS_CC freeOpenvinoConfig(void *instance)
{
    delete static_cast<std::string *>(instance);
}

Node inferenceNode(const Builder &builder, const GraphConfig &config,
    VSNode *left, VSNode *right, VSNode *mask)
{
    Map args = builder.arguments();
    for (VSNode *node : {left, right, mask})
        builder.api->mapSetNode(args.get(), "clips", node, maAppend);
    if (config.tensor_rt)
    {
        builder.api->mapSetData(args.get(), "engine_path", config.engine_path.data(),
            static_cast<int>(config.engine_path.size()), dtUtf8, maReplace);
        builder.api->mapSetInt(args.get(), "device_id", config.device_id, maReplace);
        builder.api->mapSetInt(args.get(), "use_cuda_graph", 1, maReplace);
        builder.api->mapSetInt(args.get(), "num_streams", 2, maReplace);
        builder.api->mapSetInt(args.get(), "verbosity", 2, maReplace);
        return builder.invokeNode("trt", "Model", args);
    }

    builder.api->mapSetData(args.get(), "network_path", config.model_path.data(),
        static_cast<int>(config.model_path.size()), dtUtf8, maReplace);
    builder.api->mapSetData(args.get(), "device", config.openvino_device.data(),
        static_cast<int>(config.openvino_device.size()), dtUtf8, maReplace);
    builder.api->mapSetInt(args.get(), "builtin", 0, maReplace);
    builder.api->mapSetInt(args.get(), "fp16", 0, maReplace);
    auto cache = std::make_unique<std::string>(config.engine_path.data(), config.engine_path.size());
    Function function(builder.api->createFunction(openvinoConfig, cache.get(),
        freeOpenvinoConfig, builder.core), {builder.api});
    if (!function)
        throw std::runtime_error("Could not create the OpenVINO configuration.");
    cache.release();
    builder.api->mapSetFunction(args.get(), "config", function.get(), maReplace);
    return builder.invokeNode("ov", "Model", args);
}

struct Output
{
    GraphConfig config;
    Timeline timeline;
    Node source;
    Node gray;
    Node histogram;
    Node generated;
};

bool temporalGuard(const Output &output, int index, VSFrameContext *context, const VSAPI *api)
{
    Frame left(api->getFrameFilter(index, output.gray.get(), context), {api});
    Frame right(api->getFrameFilter(index + 1, output.gray.get(), context), {api});
    Frame leftBins(api->getFrameFilter(index, output.histogram.get(), context), {api});
    Frame rightBins(api->getFrameFilter(index + 1, output.histogram.get(), context), {api});
    if (!left || !right || !leftBins || !rightBins)
        throw std::runtime_error("The temporal frame generation analysis is unavailable.");

    const int width = api->getFrameWidth(left.get(), 0);
    const int height = api->getFrameHeight(left.get(), 0);
    std::array<uint64_t, 64> leftHistogram {};
    std::array<uint64_t, 64> rightHistogram {};
    double difference = 0;
    for (int y = 0; y < height; ++y)
    {
        const auto *leftRow = reinterpret_cast<const float *>(
            api->getReadPtr(left.get(), 0) + y * api->getStride(left.get(), 0));
        const auto *rightRow = reinterpret_cast<const float *>(
            api->getReadPtr(right.get(), 0) + y * api->getStride(right.get(), 0));
        const auto *leftBytes = api->getReadPtr(leftBins.get(), 0) + y * api->getStride(leftBins.get(), 0);
        const auto *rightBytes = api->getReadPtr(rightBins.get(), 0) + y * api->getStride(rightBins.get(), 0);
        for (int x = 0; x < width; ++x)
        {
            difference += std::abs(static_cast<double>(leftRow[x]) - rightRow[x]);
            ++leftHistogram[leftBytes[x] >> 2];
            ++rightHistogram[rightBytes[x] >> 2];
        }
    }
    const double count = static_cast<double>(width) * height;
    double coefficient = 0;
    for (size_t i = 0; i < leftHistogram.size(); ++i)
        coefficient += std::sqrt(static_cast<double>(leftHistogram[i]) * rightHistogram[i]);
    const double sceneScore = std::sqrt(std::max(0.0, 1.0 - coefficient / count));
    return sceneScore >= output.config.scene_threshold
        || difference / count < output.config.duplicate_threshold;
}

const VSFrame *VS_CC outputFrame(int n, int reason, void *instance, void **frameData,
    VSFrameContext *context, VSCore *core, const VSAPI *api)
{
    const auto &output = *static_cast<Output *>(instance);
    const Sample position = output.timeline.position(n);
    try
    {
        if (reason == arInitial)
        {
            api->requestFrameFilter(position.index, output.source.get(), context);
            if (position.remainder)
            {
                for (VSNode *analysis : {output.gray.get(), output.histogram.get()})
                {
                    api->requestFrameFilter(position.index, analysis, context);
                    api->requestFrameFilter(position.index + 1, analysis, context);
                }
            }
            return nullptr;
        }
        if (reason != arAllFramesReady)
            return nullptr;

        if (position.remainder && !*frameData)
        {
            if (!temporalGuard(output, position.index, context, api))
            {
                // Request the model only after the temporal guard. Coincident,
                // cut and duplicate frames never evaluate the inference node.
                *frameData = instance;
                api->requestFrameFilter(n, output.generated.get(), context);
                return nullptr;
            }
        }

        Frame source(api->getFrameFilter(position.index, output.source.get(), context), {api});
        if (!source)
            return nullptr;
        Frame generated(nullptr, {api});
        if (*frameData)
        {
            generated.reset(api->getFrameFilter(n, output.generated.get(), context));
            if (!generated)
                return nullptr;
        }
        VSFrame *frame = api->copyFrame(generated ? generated.get() : source.get(), core);
        VSMap *props = api->getFramePropertiesRW(frame);
        api->copyMap(api->getFramePropertiesRO(source.get()), props);
        api->mapSetInt(props, "_DurationNum", output.config.target_den, maReplace);
        api->mapSetInt(props, "_DurationDen", output.config.target_num, maReplace);
        return frame;
    }
    catch (const std::exception &error)
    {
        api->setFilterError(error.what(), context);
        return nullptr;
    }
}

void VS_CC freeOutput(void *instance, VSCore *, const VSAPI *)
{
    delete static_cast<Output *>(instance);
}

Node buildGraph(const Builder &builder, VSNode *input, GraphConfig config)
{
    VSVideoInfo info = *builder.api->getVideoInfo(input);
    if (!info.width || !info.height || info.format.colorFamily == cfUndefined || info.numFrames <= 0)
        throw std::runtime_error("Frame generation requires a constant video format and dimensions.");
    auto lease = Qplay::Framegen::acquire_graph_lease(config.gpu_lock_path);

    Map load = builder.arguments();
    builder.api->mapSetData(load.get(), "path", config.plugin_path.c_str(), -1, dtUtf8, maReplace);
#ifdef _WIN32
    builder.api->mapSetInt(load.get(), "altsearchpath", 1, maReplace);
#endif
    builder.invoke("std", "LoadPlugin", load);
    if (config.tensor_rt)
    {
        Map args = builder.arguments();
        builder.api->mapSetInt(args.get(), "device_id", config.device_id, maReplace);
        Map properties = builder.invoke("trt", "DeviceProperties", args);
        if (builder.api->mapNumElements(properties.get(), "uuid") != 16)
            throw std::runtime_error("TensorRT did not report the selected GPU UUID.");
        for (int i = 0; i < 16; ++i)
        {
            const auto actual = static_cast<uint8_t>(builder.api->mapGetInt(properties.get(), "uuid", i, nullptr));
            if (actual != config.gpu_uuid[i])
                throw std::runtime_error("The TensorRT GPU differs from the prepared engine's GPU.");
        }
    }

    info.fpsNum = config.target_num;
    info.fpsDen = config.target_den;
    info.numFrames = Qplay::Framegen::output_length(static_cast<uint32_t>(info.numFrames),
        config.step_num, config.step_den);
    const Timeline timeline {config.step_num, config.step_den};
    const int modelFormat = config.tensor_rt ? pfRGBH : pfRGBS;
    Node rgb = builder.resize(input, modelFormat, config.matrix, config.full_range);
    const auto shape = Qplay::Framegen::inference_shape(info.width, info.height, config.tensor_rt);
    Node modelInput = padModelInput(builder, rgb.get(), static_cast<int>(shape.width), static_cast<int>(shape.height));
    VSVideoInfo modelInfo = info;
    modelInfo.width = static_cast<int>(shape.width);
    modelInfo.height = static_cast<int>(shape.height);
    Node left = sampleNode(builder, modelInput.get(), info, timeline, 0);
    Node right = sampleNode(builder, modelInput.get(), info, timeline, 1);
    Node mask = maskNode(builder, modelInfo, timeline, std::move(lease));
    if (config.tensor_rt)
        mask = builder.resize(mask.get(), pfGrayH, config.matrix, config.full_range);
    Node generated = inferenceNode(builder, config, left.get(), right.get(), mask.get());
    if (modelInfo.width != info.width || modelInfo.height != info.height)
    {
        Map crop = builder.arguments();
        builder.api->mapSetNode(crop.get(), "clip", generated.get(), maReplace);
        builder.api->mapSetInt(crop.get(), "width", info.width, maReplace);
        builder.api->mapSetInt(crop.get(), "height", info.height, maReplace);
        generated = builder.invokeNode("std", "CropAbs", crop);
    }
    const auto &sourceFormat = builder.api->getVideoInfo(input)->format;
    const int sourceFormatId = static_cast<int>(builder.api->queryVideoFormatID(
        sourceFormat.colorFamily, sourceFormat.sampleType, sourceFormat.bitsPerSample,
        sourceFormat.subSamplingW, sourceFormat.subSamplingH, builder.core));
    generated = builder.resize(generated.get(), sourceFormatId, config.matrix, config.full_range);

    // Detector thresholds are calibrated on 320px luma, preserving aspect ratio
    // with an even analysis height. Model and output retain the decoded size.
    const int analysisHeight = std::max(2, static_cast<int>(std::nearbyint(
        static_cast<double>(info.height) * 320 / info.width / 2)) * 2);
    Node gray = builder.resize(rgb.get(), pfGrayS, 1, true, 320, analysisHeight);
    Node histogram = builder.resize(gray.get(), pfGray8, 1, true, 0, 0, true);

    auto output = std::make_unique<Output>(Output {
        std::move(config), timeline,
        Node(builder.api->addNodeRef(input), {builder.api}), std::move(gray),
        std::move(histogram), std::move(generated)});
    const VSFilterDependency dependencies[] {
        {output->source.get(), rpGeneral}, {output->gray.get(), rpGeneral},
        {output->histogram.get(), rpGeneral}, {output->generated.get(), rpGeneral}};
    Node node(builder.api->createVideoFilter2("QplayNativeRife", &info, outputFrame,
        freeOutput, fmParallel, dependencies, 4, output.get(), builder.core), {builder.api});
    if (!node)
        throw std::runtime_error("Could not create the native frame generation output.");
    output.release();
    return node;
}
}

extern "C" VSNode *qplay_create_framegen_graph(const VSAPI *api, VSCore *core,
    VSNode *input, const char *config, char *error, std::size_t errorSize) noexcept
{
    try
    {
        if (!api || !core || !input || !config)
            throw std::runtime_error("The native frame generation graph arguments are incomplete.");
        return buildGraph(Builder {api, core}, input,
            Qplay::Framegen::parse_graph_config(config)).release();
    }
    catch (const std::exception &failure)
    {
        if (error && errorSize)
        {
            const char *message = failure.what();
            size_t length = std::min(std::strlen(message), errorSize - 1);
            while (length && (static_cast<unsigned char>(message[length]) & 0xc0) == 0x80)
                --length;
            std::memcpy(error, message, length);
            error[length] = '\0';
        }
        return nullptr;
    }
}
