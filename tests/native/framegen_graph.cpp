#include "framegen_graph.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <dlfcn.h>
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <mutex>
#include <stdexcept>
#include <string>

namespace
{
void require(bool condition, const char *message)
{
    if (!condition)
        throw std::runtime_error(message);
}

void requireNativeLibraries()
{
#ifdef _WIN32
    require(!GetModuleHandleW(L"python3.dll") && !GetModuleHandleW(L"python312.dll")
        && !GetModuleHandleW(L"python313.dll")
        && !GetModuleHandleW(L"vsscript.dll"), "Python or VSScript was loaded");
#else
    std::ifstream maps {"/proc/self/maps"};
    require(maps.is_open(), "Read loaded native library mappings failed");
    const std::string libraries {std::istreambuf_iterator<char> {maps}, {}};
    require(libraries.find("libpython") == std::string::npos
        && libraries.find("libvsscript") == std::string::npos, "Python or VSScript was loaded");
#endif
}

void requireMetadata(const VSAPI *api, const VSFrame *frame, int sourceIndex, int matrix, int range)
{
    const VSMap *props = api->getFramePropertiesRO(frame);
    require(api->mapGetInt(props, "_SourceIndex", 0, nullptr) == sourceIndex
        && api->mapGetInt(props, "_SARNum", 0, nullptr) == 4
        && api->mapGetInt(props, "_SARDen", 0, nullptr) == 3
        && api->mapGetInt(props, "_DurationNum", 0, nullptr) == 1001
        && api->mapGetInt(props, "_DurationDen", 0, nullptr) == 60000
        && api->mapGetInt(props, "_Matrix", 0, nullptr) == matrix
        && api->mapGetInt(props, "_Primaries", 0, nullptr) == 1
        && api->mapGetInt(props, "_Transfer", 0, nullptr) == 1
        && api->mapGetInt(props, "_ColorRange", 0, nullptr) == range
        && api->mapGetDataSize(props, "_MP_IMAGE", 0, nullptr) == 15
        && std::memcmp(api->mapGetData(props, "_MP_IMAGE", 0, nullptr), "native-metadata", 15) == 0,
        "Native frame generation lost source metadata or exact duration");
}

#ifdef _WIN32
std::wstring widePath(const char *path)
{
    const int length = MultiByteToWideChar(CP_UTF8, 0, path, -1, nullptr, 0);
    std::wstring wide(length, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, path, -1, wide.data(), length);
    std::replace(wide.begin(), wide.end(), L'/', L'\\');
    return wide;
}
#endif

bool lockAvailable(const char *path)
{
#ifdef _WIN32
    const auto file = CreateFileW(widePath(path).c_str(), GENERIC_READ | GENERIC_WRITE,
        0, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE)
    {
        require(GetLastError() == ERROR_SHARING_VIOLATION, "Unexpected GPU lock probe error");
        return false;
    }
    CloseHandle(file);
    return true;
#else
    const int file = open(path, O_CREAT | O_RDWR, 0600);
    require(file >= 0, "Open GPU lock probe failed");
    const bool available = flock(file, LOCK_EX | LOCK_NB) == 0;
    close(file);
    return available;
#endif
}

class Library
{
#ifdef _WIN32
    HMODULE handle;
#else
    void *handle;
#endif

public:
    explicit Library(const char *path)
    {
#ifdef _WIN32
        handle = LoadLibraryExW(widePath(path).c_str(), nullptr,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
#else
        handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
#endif
        if (!handle)
        {
#ifdef _WIN32
            const auto code = GetLastError();
            throw std::runtime_error(std::string("Load native graph fixture library failed: ")
                + path + " (Windows error " + std::to_string(code) + ")");
#else
            throw std::runtime_error(std::string("Load native graph fixture library failed: ") + dlerror());
#endif
        }
    }

    ~Library()
    {
#ifdef _WIN32
        FreeLibrary(handle);
#else
        dlclose(handle);
#endif
    }

    Library(const Library &) = delete;
    Library &operator=(const Library &) = delete;

    template <typename T>
    T symbol(const char *name) const
    {
#ifdef _WIN32
        const auto address = GetProcAddress(handle, name);
#else
        const auto address = dlsym(handle, name);
#endif
        require(address != nullptr, "Resolve native graph fixture symbol failed");
        return reinterpret_cast<T>(address);
    }
};

struct Source
{
    VSVideoInfo info {};
    std::atomic_int *live;
    std::atomic_int requests = 0;
    std::atomic_bool cancelled = false;
};

const VSFrame *VS_CC sourceFrame(int n, int reason, void *instance, void **,
    VSFrameContext *context, VSCore *core, const VSAPI *api)
{
    if (reason != arInitial)
        return nullptr;
    auto &source = *static_cast<Source *>(instance);
    ++source.requests;
    if (source.cancelled || n >= 5)
    {
        api->setFilterError("Fixture EOF or cancellation", context);
        return nullptr;
    }
    VSFrame *frame = api->newVideoFrame(&source.info.format,
        source.info.width, source.info.height, nullptr, core);
    for (int plane = 0; plane < 3; ++plane)
    {
        auto *pixels = api->getWritePtr(frame, plane);
        const auto stride = api->getStride(frame, plane);
        for (int y = 0; y < source.info.height; ++y)
        {
            if (source.info.format.sampleType == stFloat)
            {
                auto *row = reinterpret_cast<float *>(pixels + y * stride);
                for (int x = 0; x < source.info.width; ++x)
                    row[x] = 0.08f + 0.7f * x / source.info.width + 0.05f * y / source.info.height
                        + 0.01f * plane + 0.005f * n;
                continue;
            }
            for (int x = 0; x < source.info.width; ++x)
                pixels[y * stride + x] = static_cast<uint8_t>(plane ? 128
                    : n >= 3 ? 230 : 16 + x * 160 / source.info.width
                        + y * 24 / source.info.height + n);
        }
    }
    VSMap *props = api->getFramePropertiesRW(frame);
    api->mapSetInt(props, "_SourceIndex", n, maReplace);
    api->mapSetInt(props, "_SARNum", 4, maReplace);
    api->mapSetInt(props, "_SARDen", 3, maReplace);
    api->mapSetInt(props, "_Matrix", source.info.format.colorFamily == cfRGB ? 0 : 1, maReplace);
    api->mapSetInt(props, "_Primaries", 1, maReplace);
    api->mapSetInt(props, "_Transfer", 1, maReplace);
    api->mapSetInt(props, "_ColorRange", source.info.format.colorFamily == cfRGB ? 0 : 1, maReplace);
    api->mapSetData(props, "_MP_IMAGE", "native-metadata", 15, dtBinary, maReplace);
    return frame;
}

void VS_CC freeSource(void *instance, VSCore *, const VSAPI *)
{
    auto *source = static_cast<Source *>(instance);
    --*source->live;
    delete source;
}

void scenario(const VSAPI *api, int width, int height, const std::string &config,
    int (*inferences)(), int (*teardowns)(), int (*earlyUnlocks)(), const char *lockPath)
{
    const int destroyedBefore = teardowns();
    VSCore *core = api->createCore(ccfDisableAutoLoading);
    require(core, "Create native VS core failed");
    api->setThreadCount(2, core);
    std::atomic_int live = 1;
    auto *source = new Source;
    source->live = &live;
    source->info.width = width;
    source->info.height = height;
    source->info.numFrames = INT32_MAX / 16;
    api->queryVideoFormat(&source->info.format, cfYUV, stInteger, 8, 0, 0, core);
    VSNode *input = api->createVideoFilter2("LegalNativeFramegenFixture", &source->info,
        sourceFrame, freeSource, fmParallel, nullptr, 0, source, core);
    char error[1024] {};
    VSNode *output = qplay_create_framegen_graph(api, core, input, config.c_str(), error, sizeof(error));
    if (!output)
        throw std::runtime_error(error);
    require(source->requests == 0, "Graph construction requested source frames");
    require(!lockAvailable(lockPath), "The native graph did not acquire its GPU lease");
    VSCore *replacementCore = api->createCore(ccfDisableAutoLoading);
    auto *replacementSource = new Source;
    replacementSource->info = source->info;
    replacementSource->live = &live;
    ++live;
    VSNode *replacementInput = api->createVideoFilter2("LegalReplacementFramegenFixture",
        &replacementSource->info, sourceFrame, freeSource, fmParallel, nullptr, 0,
        replacementSource, replacementCore);
    VSNode *replacementOutput = qplay_create_framegen_graph(api, replacementCore,
        replacementInput, config.c_str(), error, sizeof(error));
    if (!replacementOutput)
        throw std::runtime_error(error);
    require(replacementSource->requests == 0, "Replacement graph requested source frames during construction");
    const VSVideoInfo *info = api->getVideoInfo(output);
    require(info->width == width && info->height == height
        && info->fpsNum == 60000 && info->fpsDen == 1001, "Output geometry or rational rate changed");

    const int before = inferences();
    for (int index : {0, 1, 5, 6, 8, 10})
    {
        const VSFrame *frame = api->getFrame(index, output, error, sizeof(error));
        if (!frame)
            throw std::runtime_error(error);
        require(api->getFrameWidth(frame, 0) == width && api->getFrameHeight(frame, 0) == height,
            "Native frame generation resized a frame");
        requireMetadata(api, frame, index * 2 / 5, 1, 1);
        if (index != 1)
        {
            const VSFrame *original = api->getFrame(index * 2 / 5, input, error, sizeof(error));
            require(original, "Read the original fixture frame failed");
            for (int plane = 0; plane < 3; ++plane)
            {
                for (int y = 0; y < height; ++y)
                {
                    require(std::memcmp(api->getReadPtr(frame, plane) + y * api->getStride(frame, plane),
                        api->getReadPtr(original, plane) + y * api->getStride(original, plane), width) == 0,
                        "A coincident, cut or duplicate frame changed decoded pixels");
                }
            }
            api->freeFrame(original);
        }
        api->freeFrame(frame);
        if (inferences() - before != (index ? 1 : 0))
            throw std::runtime_error("Unexpected inference count at output frame " + std::to_string(index)
                + ": " + std::to_string(inferences() - before));
    }
    const VSFrame *tail = api->getFrame(11, output, error, sizeof(error));
    require(!tail && std::strstr(error, "EOF"), "Fractional dynamic EOF tail was not dropped");

    struct Pending
    {
        const VSAPI *api;
        std::mutex mutex;
        std::condition_variable done;
        int count = 0;
        int failures = 0;
    } pending {api};
    source->cancelled = true;
    pending.count = 16;
    for (int n = 100; n < 116; ++n)
    {
        api->getFrameAsync(n, output, [](void *instance, const VSFrame *frame, int,
            VSNode *, const char *)
        {
            auto &pending = *static_cast<Pending *>(instance);
            if (frame)
                pending.api->freeFrame(frame);
            std::lock_guard lock(pending.mutex);
            pending.failures += !frame;
            if (--pending.count == 0)
                pending.done.notify_one();
        }, &pending);
    }
    {
        std::unique_lock lock(pending.mutex);
        require(pending.done.wait_for(lock, std::chrono::seconds(10), [&] { return pending.count == 0; }),
            "Native graph cancellation did not drain");
    }
    require(pending.failures == 16, "Native graph cancellation returned a frame");
    api->freeNode(output);
    api->freeNode(input);
    require(live == 1 && !lockAvailable(lockPath), "An overlapping graph lost its GPU lease");
    api->freeNode(replacementOutput);
    api->freeNode(replacementInput);
    require(live == 0, "A source node survived native graph teardown");
    require(lockAvailable(lockPath), "The native graph retained its GPU lease after teardown");
    api->freeCore(replacementCore);
    api->freeCore(core);
    require(teardowns() - destroyedBefore == 2 && earlyUnlocks() == 0,
        "The GPU lease was released before the Model free callback finished");
    std::printf("PASS native-graph %dx%d rate=60000/1001 metadata=preserved lazy-inferences=1 EOF=11 cancellation=16 overlap-lease=released model-free-lease=held live=0\n",
        width, height);
}

void rifeScenario(const VSAPI *api, const std::string &config, int width, int height)
{
    VSCore *core = api->createCore(ccfDisableAutoLoading);
    api->setThreadCount(2, core);
    std::atomic_int live = 1;
    auto *source = new Source;
    source->live = &live;
    source->info.width = width;
    source->info.height = height;
    source->info.numFrames = 5;
    api->queryVideoFormat(&source->info.format, cfRGB, stFloat, 32, 0, 0, core);
    VSNode *input = api->createVideoFilter2("LegalNativeRifeFixture", &source->info,
        sourceFrame, freeSource, fmParallel, nullptr, 0, source, core);
    char error[1024] {};
    VSNode *output = qplay_create_framegen_graph(api, core, input, config.c_str(), error, sizeof(error));
    if (!output)
        throw std::runtime_error(error);
    const VSFrame *generated = api->getFrame(1, output, error, sizeof(error));
    if (!generated)
        throw std::runtime_error(error);
    const VSFrame *left = api->getFrame(0, input, error, sizeof(error));
    require(left && api->getFrameWidth(generated, 0) == width && api->getFrameHeight(generated, 0) == height,
        "Native RIFE inference changed original geometry");
    requireMetadata(api, generated, 0, 0, 0);
    double difference = 0;
    float minimum = 100;
    float maximum = -100;
    for (int plane = 0; plane < 3; ++plane)
    {
        for (int y = 0; y < height; ++y)
        {
            const auto *pixels = reinterpret_cast<const float *>(
                api->getReadPtr(generated, plane) + y * api->getStride(generated, plane));
            const auto *original = reinterpret_cast<const float *>(
                api->getReadPtr(left, plane) + y * api->getStride(left, plane));
            for (int x = 0; x < width; ++x)
            {
                require(std::isfinite(pixels[x]), "Native RIFE produced a non-finite pixel");
                minimum = std::min(minimum, pixels[x]);
                maximum = std::max(maximum, pixels[x]);
                difference += std::abs(static_cast<double>(pixels[x]) - original[x]);
            }
        }
    }
    difference /= static_cast<double>(width) * height * 3;
    require(difference > 0.0001, "Native RIFE did not generate a distinct fractional frame");
    api->freeFrame(left);
    api->freeFrame(generated);
    api->freeNode(output);
    api->freeNode(input);
    require(live == 0, "Native RIFE retained source ownership");
    api->freeCore(core);
    requireNativeLibraries();
    std::printf("PASS native-RIFE-GPU %dx%d finite-pixels=%lld mean-absolute-difference=%.9f range=%.6f..%.6f live=0 Python=absent VSScript=absent\n",
        width, height, static_cast<long long>(width) * height * 3, difference, minimum, maximum);
}

// Exercise the same production factory through the native mpv driver. The
// configuration may name either the counting fixture or a prepared real GPU.
void mpvScenario(const std::string &filter,
    const char *mpvPath, const char *media, const char *lockPath)
{
    Library library(mpvPath);
    struct Event
    {
        int id;
        int error;
        uint64_t userdata;
        void *data;
    };
    struct LogMessage
    {
        const char *prefix;
        const char *level;
        const char *text;
        int logLevel;
    };
    const auto create = library.symbol<void *(*)()>("mpv_create");
    const auto initialize = library.symbol<int (*)(void *)>("mpv_initialize");
    const auto terminate = library.symbol<void (*)(void *)>("mpv_terminate_destroy");
    const auto registerGraph = library.symbol<int (*)(void *, decltype(&qplay_create_framegen_graph))>(
        "mpv_qbutt_set_vapoursynth_graph_factory");
    const auto option = library.symbol<int (*)(void *, const char *, const char *)>("mpv_set_option_string");
    const auto command = library.symbol<int (*)(void *, const char *const *)>("mpv_command");
    const auto get = library.symbol<int (*)(void *, const char *, int, void *)>("mpv_get_property");
    const auto requestLog = library.symbol<int (*)(void *, const char *)>("mpv_request_log_messages");
    const auto event = library.symbol<Event *(*)(void *, double)>("mpv_wait_event");
    void *handle = create();
    require(handle, "Create the native mpv graph session failed");
    // The deleter runs while the DLL and production factory still exist.
    std::unique_ptr<void, decltype(terminate)> session(handle, terminate);
    require(registerGraph(handle, qplay_create_framegen_graph) == 0,
        "Register the production graph with mpv failed");
    for (const auto &[name, value] : {std::pair {"config", "no"}, {"terminal", "no"},
        {"load-scripts", "no"}, {"osc", "no"}, {"ytdl", "no"}, {"vo", "null"},
        {"ao", "null"}, {"hwdec", "no"}, {"idle", "yes"}, {"keep-open", "no"}, {"pause", "yes"}})
    {
        require(option(handle, name, value) == 0, "Set the native mpv graph option failed");
    }
    require(option(handle, "vf", filter.c_str()) == 0, "Set the production native graph filter failed");
    require(requestLog(handle, "warn") == 0 && initialize(handle) == 0,
        "Initialize the native mpv graph session failed");

    std::string log;
    bool ended = false;
    auto pump = [&]
    {
        const Event *next = event(handle, 0.01);
        if (next->id == 2)
        {
            const auto *message = static_cast<LogMessage *>(next->data);
            if (message->text && log.size() < 4096)
                log += message->text;
        }
        else if (next->id == 7)
        {
            const auto *reason = static_cast<const int *>(next->data);
            require(reason && reason[0] == 0 && reason[1] == 0, "Native graph playback ended with an error");
            ended = true;
        }
    };
    auto wait = [&](auto condition, const char *failure)
    {
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
        while (!condition() && std::chrono::steady_clock::now() < deadline)
            pump();
        if (!condition())
            throw std::runtime_error(std::string(failure) + "\n" + log);
    };
    auto integer = [&](const char *name)
    {
        int64_t value = 0;
        get(handle, name, 4, &value);
        return value;
    };
    auto number = [&](const char *name)
    {
        double value = 0;
        get(handle, name, 5, &value);
        return value;
    };
    auto run = [&](const char *name, const char *argument, const char *second = nullptr)
    {
        const char *arguments[] {name, argument, second, nullptr};
        require(command(handle, arguments) == 0, "Native graph playback command failed");
    };
    run("loadfile", media);
    wait([&] { return integer("video-out-params/w") > 0; }, "The production native graph produced no output");
    const int64_t width = integer("video-params/w");
    const int64_t height = integer("video-params/h");
    require(width == integer("video-out-params/w") && height == integer("video-out-params/h"),
        "The production native graph changed decoded geometry");
    run("set", "pause", "no");
    wait([&] { return number("time-pos") >= 0.25; }, "Native graph playback did not advance");
    const double filteredRate = number("estimated-vf-fps");
    require(std::abs(filteredRate - 60000.0 / 1001) < 0.05,
        "Native mpv did not observe the exact generated frame rate");
    run("set", "pause", "yes");
    run("seek", "0.5", "absolute+exact");
    wait([&] { return number("time-pos") >= 0.45; }, "Native graph exact seek did not advance");
    run("set", "pause", "no");
    wait([&] { return ended; }, "Native graph EOF did not drain");
    ended = false;
    run("loadfile", media);
    wait([&] { return number("time-pos") >= 0.25; }, "Native graph reopen did not advance");
    run("stop", nullptr);
    session.reset();
    require(lockAvailable(lockPath), "mpv retained the graph GPU lease after terminate_destroy");
    requireNativeLibraries();
    std::printf("PASS libmpv-production-native-graph %lldx%lld fps=%.9f pause-seek-resume EOF reopen stop terminate GPU-lease=released Python=absent VSScript=absent\n",
        static_cast<long long>(width), static_cast<long long>(height), filteredRate);
}
}

int main(int argc, char **argv)
{
    const bool mpv = argc == 6 && std::strcmp(argv[1], "--mpv") == 0;
    const bool rife = argc == 6 && std::strcmp(argv[1], "--rife") == 0;
    if (argc != 5 && !mpv && !rife)
        return 2;
    try
    {
        if (mpv)
        {
            std::ifstream file(argv[2]);
            const std::string config(std::istreambuf_iterator<char> {file}, {});
            mpvScenario(config, argv[3], argv[4], argv[5]);
            return 0;
        }
        if (rife)
        {
            Library core(argv[2]);
            const auto getApi = core.symbol<VSGetVapourSynthAPI>("getVapourSynthAPI");
            std::ifstream file(argv[3]);
            const std::string config(std::istreambuf_iterator<char> {file}, {});
            rifeScenario(getApi(VAPOURSYNTH_API_VERSION), config, std::stoi(argv[4]), std::stoi(argv[5]));
            return 0;
        }
        Library core(argv[1]);
        Library model(argv[3]);
        auto getApi = core.symbol<VSGetVapourSynthAPI>("getVapourSynthAPI");
        const VSAPI *api = getApi(VAPOURSYNTH_API_VERSION);
        require(api, "The native VS API is unavailable");
        std::ifstream file(argv[2]);
        const std::string config(std::istreambuf_iterator<char> {file}, {});
        auto inferences = model.symbol<int (*)()>("qplay_fixture_inference_count");
        model.symbol<void (*)(const char *)>("qplay_fixture_set_gpu_lock_path")(argv[4]);
        auto teardowns = model.symbol<int (*)()>("qplay_fixture_teardown_count");
        auto earlyUnlocks = model.symbol<int (*)()>("qplay_fixture_early_unlock_count");
        for (const auto &[width, height] : {std::pair {1, 1}, {17, 19}, {19, 17}, {641, 359}, {359, 641},
            {1921, 1081}, {3840, 2160}})
        {
            scenario(api, width, height, config, inferences, teardowns, earlyUnlocks, argv[4]);
        }
        requireNativeLibraries();
        std::puts("PASS native-graph Python=absent VSScript=absent GPU-inference=not-tested");
        return 0;
    }
    catch (const std::exception &error)
    {
        std::fprintf(stderr, "%s\n", error.what());
        return 1;
    }
}
