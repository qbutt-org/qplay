#include "framegen.h"

#ifdef _WIN32
#include <Windows.h>
#include <TlHelp32.h>
#endif

#include <cmath>
#include <cstdio>
#include <stdexcept>

#include <QElapsedTimer>
#include <QImage>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QOpenGLFramebufferObject>
#include <QSet>
#include <QString>

#include "qplay_native/src/controller.cxxqt.h"

namespace
{
void require(bool condition, const char *message)
{
    if (!condition)
        throw std::runtime_error(message);
}

QJsonArray loadedLibraries()
{
    QJsonArray libraries;
#ifdef _WIN32
    const auto snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, GetCurrentProcessId());
    require(snapshot != INVALID_HANDLE_VALUE, "Inspect native GPU dependencies failed");
    MODULEENTRY32W entry {sizeof(entry)};
    if (Module32FirstW(snapshot, &entry))
    {
        do
        {
            libraries.append(QDir::fromNativeSeparators(QString::fromWCharArray(entry.szExePath)));
        }
        while (Module32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);
#else
    QFile maps(QStringLiteral("/proc/self/maps"));
    require(maps.open(QIODevice::ReadOnly), "Inspect native GPU dependencies failed");
    QSet<QString> paths;
    for (const QByteArray &line : maps.readAll().split('\n'))
    {
        const auto start = line.indexOf('/');
        if (start >= 0)
        {
            const QString path = QString::fromUtf8(line.mid(start)).trimmed();
            if (path.contains(QStringLiteral(".so")))
                paths.insert(path);
        }
    }
    require(!paths.isEmpty(), "Native GPU dependency mappings were empty");
    QStringList ordered = paths.values();
    ordered.sort();
    for (const QString &path : ordered)
        libraries.append(path);
#endif
    return libraries;
}
}

QJsonObject exerciseFramegen(Qplay::PlayerController &backend, QOpenGLFramebufferObject &target,
    const QString &secondMedia, const QString &preferences, const QString &evidencePrefix,
    const std::function<void()> &tick,
    const std::function<void(const std::function<bool()> &, const char *)> &wait)
{
    QJsonObject evidence;
    const auto waitGpu = [&](const auto &condition, qint64 timeout, const char *message)
    {
        QElapsedTimer timer;
        timer.start();
        QString previousStatus;
        while (!condition() && timer.elapsed() < timeout)
        {
            tick();
            const auto status = backend.framegenStatus();
            if (status != previousStatus)
            {
                std::fprintf(stderr, "Frame generation: %s\n", status.toUtf8().constData());
                std::fflush(stderr);
                previousStatus = status;
            }
            if (!backend.gpuError().isEmpty())
                throw std::runtime_error(backend.gpuError().toStdString());
        }
        require(condition(), message);
    };

    backend.changeSpeed(1);
    wait([&] { return std::abs(backend.speed() - 1) < 0.001; },
        "Normal playback speed did not return");
    backend.changeShader(QStringLiteral("auto"));
    backend.changeContent(QStringLiteral("movie"));
    backend.changeFramegen(QStringLiteral("on"));
    backend.openMedia(secondMedia);
    wait([&] { return backend.mediaLoaded(); }, "Frame generation video did not load");
    if (!backend.paused())
        backend.togglePause();
    wait([&] { return backend.paused(); }, "Pause before graph preparation failed");
    waitGpu([&] { return backend.framegenStatus() == QStringLiteral("starting")
        || backend.framegenStatus() == QStringLiteral("active"); }, 30 * 60 * 1000,
        "Frame generation graph preparation timed out");
    if (backend.paused())
        backend.togglePause();
    waitGpu([&] { return backend.framegenStatus() == QStringLiteral("active"); },
        2 * 60 * 1000, "Frame generation output was not verified");
    require(backend.gpuError().isEmpty(), "Frame generation returned an error");
    tick();
    const QImage firstVideo = target.toImage().copy(0, 88, 1280, 500);
    const double firstPosition = backend.position();
    QElapsedTimer playback;
    playback.start();
    while (playback.elapsed() < 5000)
        tick();
    const double playbackAdvance = backend.position() - firstPosition;
    require(playbackAdvance >= 4.0 && backend.framegenStatus() == QStringLiteral("active"),
        "Frame generation did not sustain real-time playback");
    require(target.toImage().copy(0, 88, 1280, 500) != firstVideo,
        "Frame generation did not render changing video");
    const auto observation = backend.framegenObservation();
    require(observation.source_fps > 0 && observation.target_fps > observation.source_fps,
        "Frame generation did not increase the source frame rate");
    require(std::abs(observation.measured_fps - observation.target_fps) <= observation.target_fps * 0.03,
        "The rendered filter did not produce the target frame rate");
    require(observation.output_width > 0 && observation.output_height > 0
        && observation.filter_name == "@mpv-framegen",
        "The rendered frame generation output was not present");
    require(target.toImage().save(evidencePrefix + QStringLiteral(".framegen.png")),
        "Save generated-frame playback composition failed");
    evidence[QStringLiteral("framegenActive")] = true;
    evidence[QStringLiteral("framegenPlaybackAdvanceSeconds")] = playbackAdvance;
    evidence[QStringLiteral("framegenWallSeconds")] = playback.elapsed() / 1000.0;
    evidence[QStringLiteral("framegenRenderedVideoChanged")] = true;
    evidence[QStringLiteral("framegenChoice")] = backend.framegenChoice();
    evidence[QStringLiteral("framegenStatus")] = backend.framegenStatus();
    evidence[QStringLiteral("framegenSourceFps")] = observation.source_fps;
    evidence[QStringLiteral("framegenTargetFps")] = observation.target_fps;
    evidence[QStringLiteral("framegenMeasuredFps")] = observation.measured_fps;
    evidence[QStringLiteral("framegenOutputWidth")] = static_cast<int>(observation.output_width);
    evidence[QStringLiteral("framegenOutputHeight")] = static_cast<int>(observation.output_height);
    evidence[QStringLiteral("framegenFilter")] = QString::fromUtf8(
        observation.filter_name.data(), observation.filter_name.size());
    evidence[QStringLiteral("loadedLibraries")] = loadedLibraries();
    backend.changeShader(QStringLiteral("anime4k_a"));
    waitGpu([&] { return backend.framegenStatus() == QStringLiteral("off")
        && backend.framegenChoice() == QStringLiteral("off")
        && backend.effectiveShader() == QStringLiteral("anime4k_a"); }, 15 * 1000,
        "Manual Anime4K did not replace active frame generation");
    require(backend.framegenObservation().filter_name.empty(),
        "Manual Anime4K left the frame generation filter active");
    evidence[QStringLiteral("manualShaderStoppedFramegen")] = true;
    backend.stop();
    wait([&] { return !backend.mediaLoaded(); }, "Stopping generated-frame playback failed");
    Qplay::PlayerController restored;
    restored.configure(preferences, QStringLiteral("en_US"));
    require(restored.shaderChoice() == QStringLiteral("anime4k_a")
        && restored.framegenChoice() == QStringLiteral("off"),
        "Manual GPU choices were not saved together");
    evidence[QStringLiteral("manualGpuChoicesPersisted")] = true;
    backend.changeFramegen(QStringLiteral("auto"));
    require(backend.shaderChoice() == QStringLiteral("auto")
        && backend.framegenChoice() == QStringLiteral("auto"),
        "Automatic frame generation did not restore automatic shader choice");
    backend.changeShader(QStringLiteral("anime4k_a"));
    require(backend.framegenChoice() == QStringLiteral("off"),
        "Manual Anime4K did not disable automatic frame generation");
    evidence[QStringLiteral("autoFramegenResolvedManualShader")] = true;
    backend.changeFramegen(QStringLiteral("on"));
    require(backend.shaderChoice() == QStringLiteral("fsr")
        && backend.framegenChoice() == QStringLiteral("on"),
        "Manual frame generation did not select compatible FSR");
    backend.openMedia(secondMedia);
    wait([&] { return backend.mediaLoaded(); }, "Frame generation did not reopen after manual shader");
    waitGpu([&] { return backend.framegenStatus() == QStringLiteral("active"); },
        2 * 60 * 1000, "Frame generation did not resume after manual Anime4K");
    evidence[QStringLiteral("framegenRestoredAfterManualShader")] = true;
    backend.stop();
    wait([&] { return !backend.mediaLoaded(); }, "Stopping restored frame generation failed");
    return evidence;
}
