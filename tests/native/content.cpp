#include "content.h"

#include <stdexcept>

#include <QElapsedTimer>
#include <QImage>
#include <QOpenGLFramebufferObject>
#include <QString>
#include <QThreadPool>

#include "qplay_native/src/controller.cxxqt.h"

namespace
{
void require(bool condition, const char *message)
{
    if (!condition)
        throw std::runtime_error(message);
}
}

QJsonObject exerciseContentPolicy(Qplay::PlayerController &backend, QOpenGLFramebufferObject &target,
    const QString &localAnime, const QString &remoteAnime, const QString &preferences,
    const QString &evidencePrefix, const std::function<void()> &tick,
    const std::function<void(const std::function<bool()> &, const char *)> &wait)
{
    const QString remoteMovie = qEnvironmentVariable("QPLAY_CONTENT_MOVIE_QA");
    const QString cancelledMedia = qEnvironmentVariable("QPLAY_CONTENT_CANCEL_QA");
    require(!remoteMovie.isEmpty() && !cancelledMedia.isEmpty(), "Missing content fixture URLs");
    const auto drainProbe = [&]
    {
        QElapsedTimer timer;
        timer.start();
        do
        {
            tick();
        }
        while (QThreadPool::globalInstance()->activeThreadCount() > 0 && timer.elapsed() < 65000);
        require(QThreadPool::globalInstance()->activeThreadCount() == 0, "Content probe did not drain");
        tick();
        require(backend.framegenChoice() == QStringLiteral("off"), "Classification prepared frame generation");
    };
    const auto open = [&](const QString &media)
    {
        backend.openMedia(media);
        wait([&] { return backend.mediaLoaded() && backend.position() > 0.25; }, "Content media did not decode");
        drainProbe();
    };
    const auto animePreset = [&]
    {
        return backend.effectiveShader().startsWith(QStringLiteral("anime4k"))
            || backend.effectiveShader().startsWith(QStringLiteral("gan"));
    };
    backend.changeFramegen(QStringLiteral("off"));
    backend.changeShader(QStringLiteral("auto"));
    backend.changeContent(QStringLiteral("auto"));
    open(localAnime);
    wait(animePreset, "Local anime did not select the anime policy");
    const QString localPreset = backend.effectiveShader();
    open(remoteAnime);
    wait(animePreset, "HTTP anime remained Unknown instead of selecting the anime policy");
    const QString remotePreset = backend.effectiveShader();
    require(target.toImage().save(evidencePrefix + QStringLiteral(".auto-anime.png")), "Save HTTP Auto frame failed");

    backend.changeContent(QStringLiteral("movie"));
    drainProbe();
    wait([&] { return backend.effectiveShader() == QStringLiteral("fsr"); }, "Manual content override was ignored");
    {
        Qplay::PlayerController restored;
        restored.configure(preferences, QStringLiteral("en_US"));
        require(restored.contentChoice() == QStringLiteral("movie"), "Manual content choice did not persist");
    }
    backend.changeContent(QStringLiteral("auto"));
    open(remoteMovie);
    wait([&] { return backend.effectiveShader() == QStringLiteral("fsr"); }, "HTTP movie selected an anime shader");
    backend.changeShader(QStringLiteral("anime4k_a"));
    wait([&] { return backend.effectiveShader() == QStringLiteral("anime4k_a"); }, "Manual shader was ignored");
    backend.seek(8.0);
    wait([&] { return backend.position() >= 8.0; }, "Seek while using the manual policy failed");
    require(backend.shaderChoice() == QStringLiteral("anime4k_a"), "Seek replaced the manual shader");
    require(target.toImage().save(evidencePrefix + QStringLiteral(".manual-seek.png")), "Save manual seek frame failed");

    backend.changeShader(QStringLiteral("auto"));
    backend.openMedia(cancelledMedia);
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.25; }, "Cancellable media did not decode");
    require(QThreadPool::globalInstance()->activeThreadCount() > 0, "A pending network probe was not observed");
    QElapsedTimer cancellation;
    cancellation.start();
    open(remoteMovie);
    wait([&] { return backend.effectiveShader() == QStringLiteral("fsr"); }, "Replacement movie did not apply its shader after cancellation");
    require(cancellation.elapsed() < 5000, "Cancelled classification delayed the replacement media");
    backend.stop();
    drainProbe();
    return {
        {QStringLiteral("localAutoPreset"), localPreset},
        {QStringLiteral("httpAutoPreset"), remotePreset},
        {QStringLiteral("manualContentPreserved"), true},
        {QStringLiteral("manualShaderSeekPreserved"), true},
        {QStringLiteral("cancelReplacementMs"), cancellation.elapsed()},
        {QStringLiteral("noFramegenPreparation"), true},
        {QStringLiteral("probeDrained"), true}
    };
}
