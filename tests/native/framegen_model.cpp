// A counting VS inference fixture. This proves graph scheduling, never GPU inference.
#include "VapourSynth4.h"

#include <atomic>
#include <cmath>
#include <initializer_list>
#include <filesystem>
#include <string>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

namespace
{
std::atomic_int requests = 0;
std::atomic_int teardowns = 0;
std::atomic_int earlyUnlocks = 0;
std::string gpuLockPath;

bool gpuLockAvailable()
{
#ifdef _WIN32
    const auto path = std::filesystem::u8path(gpuLockPath);
    const auto file = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE,
        0, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE)
        return GetLastError() != ERROR_SHARING_VIOLATION;
    CloseHandle(file);
    return true;
#else
    const int file = open(gpuLockPath.c_str(), O_CREAT | O_RDWR, 0600);
    if (file < 0)
        return true;
    const bool available = flock(file, LOCK_EX | LOCK_NB) == 0;
    close(file);
    return available;
#endif
}

struct Model
{
    VSNode *left;
    VSNode *right;
    VSNode *mask;
};

const VSFrame *VS_CC modelFrame(int n, int reason, void *instance, void **,
    VSFrameContext *context, VSCore *, const VSAPI *api)
{
    const auto &model = *static_cast<Model *>(instance);
    if (reason == arInitial)
    {
        ++requests;
        for (VSNode *node : {model.left, model.right, model.mask})
            api->requestFrameFilter(n, node, context);
    }
    else if (reason == arAllFramesReady)
    {
        const VSFrame *left = api->getFrameFilter(n, model.left, context);
        api->freeFrame(api->getFrameFilter(n, model.right, context));
        const VSFrame *mask = api->getFrameFilter(n, model.mask, context);
        const float timepoint = *reinterpret_cast<const float *>(api->getReadPtr(mask, 0));
        api->freeFrame(mask);
        if (std::abs(timepoint - static_cast<float>(int64_t(n) * 2 % 5) / 5) > 0.00001f)
        {
            api->freeFrame(left);
            api->setFilterError("The native timepoint mask differs from the rational timeline.", context);
            return nullptr;
        }
        return left;
    }
    return nullptr;
}

void VS_CC freeModel(void *instance, VSCore *, const VSAPI *api)
{
    auto *model = static_cast<Model *>(instance);
    earlyUnlocks += gpuLockAvailable();
    for (VSNode *node : {model->left, model->right, model->mask})
        api->freeNode(node);
    earlyUnlocks += gpuLockAvailable();
    delete model;
    ++teardowns;
}

void VS_CC createModel(const VSMap *input, VSMap *output, void *, VSCore *core, const VSAPI *api)
{
    auto *model = new Model {
        api->mapGetNode(input, "clips", 0, nullptr),
        api->mapGetNode(input, "clips", 1, nullptr),
        api->mapGetNode(input, "clips", 2, nullptr)};
    const VSFilterDependency dependencies[] {
        {model->left, rpGeneral}, {model->right, rpGeneral}, {model->mask, rpGeneral}};
    api->createVideoFilter(output, "QplayCountingInferenceFixture", api->getVideoInfo(model->left),
        modelFrame, freeModel, fmParallel, dependencies, 3, model, core);
}
}

VS_EXTERNAL_API(int) qplay_fixture_inference_count()
{
    return requests.load();
}

VS_EXTERNAL_API(void) qplay_fixture_set_gpu_lock_path(const char *path)
{
    gpuLockPath = path;
}

VS_EXTERNAL_API(int) qplay_fixture_teardown_count()
{
    return teardowns.load();
}

VS_EXTERNAL_API(int) qplay_fixture_early_unlock_count()
{
    return earlyUnlocks.load();
}

VS_EXTERNAL_API(void) VapourSynthPluginInit2(VSPlugin *plugin, const VSPLUGINAPI *api)
{
    api->configPlugin("org.qbutt.qplay.inference-fixture", "ov", "Counting inference fixture",
        VS_MAKE_VERSION(1, 0), VAPOURSYNTH_API_VERSION, 0, plugin);
    api->registerFunction("Model", "clips:vnode[];network_path:data;device:data;builtin:int;fp16:int;config:func;",
        "clip:vnode;", createModel, nullptr, plugin);
}
