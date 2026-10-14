#include "startup.h"

#include <stdexcept>

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLocalSocket>
#include <QThread>

#include "qplay_native/src/controller.cxxqt.h"

namespace
{
void require(bool accepted, const char *message)
{
    if (!accepted)
        throw std::runtime_error(message);
}
}

QJsonObject exerciseStartup(Qplay::PlayerController &backend, const QString &library,
    const QString &media, const QString &secondMedia, const QString &profile,
    const std::function<void()> &tick)
{
    const QString pipe = QStringLiteral("qbutt-mpv-startup-%1").arg(QCoreApplication::applicationPid());
    const QString extractor = QFileInfo(library).dir().filePath(QStringLiteral("yt-dlp.exe"));
    require(QFileInfo::exists(extractor), "Pinned extractor fixture is missing");
    require(backend.configureLaunch({
        QStringLiteral("--input-ipc-server=\\\\.\\pipe\\") + pipe,
        QStringLiteral("--script-opts-append=ytdl_hook-ytdl_path=") + extractor
    }).isEmpty(), "Configure startup IPC failed");
    backend.openMedia(media);
    backend.initialize(library, profile);

    // Process controller callbacks without synchronizing the Qt render control.
    QElapsedTimer held;
    held.start();
    while ((!backend.engineReady() || held.elapsed() < 750) && held.elapsed() < 5000)
    {
        QCoreApplication::processEvents();
        QThread::msleep(10);
    }
    require(backend.engineReady(), "Startup engine did not initialize");
    require(!backend.mediaLoaded(), "Startup media loaded before the render context");
    QLocalSocket early;
    early.connectToServer(pipe);
    require(!early.waitForConnected(100), "IPC admitted media before the render context");
    early.abort();
    const qint64 renderHeldMs = held.elapsed();

    const auto wait = [&](const auto &ready, const char *message)
    {
        QElapsedTimer timer;
        timer.start();
        while (!ready() && timer.elapsed() < 15000)
        {
            tick();
            require(backend.errorText().isEmpty(), "Player failed after render initialization");
        }
        require(ready(), message);
    };
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; },
        "Deferred startup media did not decode");

    QLocalSocket receiver;
    receiver.connectToServer(pipe);
    wait([&] { return receiver.state() == QLocalSocket::ConnectedState; },
        "IPC did not open after render initialization");
    QByteArray buffered;
    int nextRequest = 0;
    const auto command = [&](const QJsonArray &arguments)
    {
        const int request = ++nextRequest;
        receiver.write(QJsonDocument(QJsonObject {
            {QStringLiteral("command"), arguments}, {QStringLiteral("request_id"), request}})
            .toJson(QJsonDocument::Compact) + '\n');
        receiver.flush();
        QJsonObject result;
        wait([&]
        {
            buffered += receiver.readAll();
            while (buffered.contains('\n'))
            {
                const int newline = buffered.indexOf('\n');
                const auto message = QJsonDocument::fromJson(buffered.left(newline)).object();
                buffered.remove(0, newline + 1);
                if (message.value(QStringLiteral("request_id")).toInt() == request)
                    result = message;
            }
            return !result.isEmpty();
        }, "IPC response timed out");
        if (result.value(QStringLiteral("error")) != QStringLiteral("success"))
            throw std::runtime_error((QJsonDocument(arguments).toJson(QJsonDocument::Compact)
                + ' ' + QJsonDocument(result).toJson(QJsonDocument::Compact)).toStdString());
        return result.value(QStringLiteral("data"));
    };
    const auto firstVideo = command({QStringLiteral("get_property"), QStringLiteral("video-out-params")}).toObject();
    require(firstVideo.value(QStringLiteral("w")).toInt() > 0
        && firstVideo.value(QStringLiteral("h")).toInt() > 0,
        "Startup media has no decoded video");

    command({QStringLiteral("loadfile"), secondMedia, QStringLiteral("replace")});
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "IPC replacement did not decode");
    const auto replacementPath = command({QStringLiteral("get_property"), QStringLiteral("path")}).toString();
    require(QDir::fromNativeSeparators(replacementPath) == QDir::fromNativeSeparators(secondMedia),
        "IPC replacement selected another file");
    const auto secondVideo = command({QStringLiteral("get_property"), QStringLiteral("video-out-params")}).toObject();
    require(secondVideo.value(QStringLiteral("w")).toInt() > 0, "IPC replacement lost video");
    receiver.disconnectFromServer();
    backend.stop();
    return {
        {QStringLiteral("renderHeldMs"), renderHeldMs},
        {QStringLiteral("mediaWaitedForRender"), true},
        {QStringLiteral("ipcWaitedForRender"), true},
        {QStringLiteral("startupVideo"), firstVideo},
        {QStringLiteral("ipcReplacementVideo"), secondVideo},
        {QStringLiteral("ipcReplacementPath"), replacementPath},
    };
}
