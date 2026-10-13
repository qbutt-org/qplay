#include "downloads.h"

#include <stdexcept>
#include <cstdio>

#include <QElapsedTimer>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLocalSocket>
#include <QMouseEvent>
#include <QQuickItem>
#include <QQuickWindow>

#include "qplay_native/src/controller.cxxqt.h"

namespace
{
void require(bool accepted, const char *message)
{
    if (!accepted)
        throw std::runtime_error(message);
}
}

QJsonObject exerciseDownloads(Qplay::PlayerController &backend, QQuickWindow &window,
    QQuickItem &root, const QString &pipe, const QString &remoteMedia, const QString &localMedia,
    const std::function<void()> &tick)
{
    const auto wait = [&](const auto &ready, const char *message, int timeout = 15000)
    {
        QElapsedTimer timer;
        timer.start();
        while (!ready() && timer.elapsed() < timeout)
            tick();
        if (!ready())
            std::fprintf(stderr, "%s loaded=%d position=%f paused=%d error=%s\n", message, backend.mediaLoaded(), backend.position(), backend.paused(), backend.errorText().toUtf8().constData());
        require(ready(), message);
    };
    const auto settle = [&]
    {
        for (int frame = 0; frame < 12; ++frame)
            tick();
    };
    QLocalSocket receiver;
    receiver.connectToServer(pipe);
    wait([&] { return receiver.state() == QLocalSocket::ConnectedState; }, "Download receiver did not connect");
    QByteArray buffered;
    QJsonArray requests;
    QObject::connect(&receiver, &QLocalSocket::readyRead, &receiver, [&]
    {
        buffered += receiver.readAll();
        while (buffered.contains('\n'))
        {
            const int newline = buffered.indexOf('\n');
            const auto message = QJsonDocument::fromJson(buffered.left(newline)).object();
            buffered.remove(0, newline + 1);
            const auto arguments = message.value(QStringLiteral("args")).toArray();
            if (!arguments.isEmpty() && arguments.first() == QStringLiteral("qplay-download"))
                requests.append(arguments);
        }
    });
    const auto send = [&](const QJsonArray &arguments)
    {
        QJsonArray command {QStringLiteral("script-message")};
        for (const auto &argument : arguments)
            command.append(argument);
        receiver.write(QJsonDocument(QJsonObject {{QStringLiteral("command"), command}}).toJson(QJsonDocument::Compact) + '\n');
        receiver.flush();
    };
    backend.changeMute(true);
    backend.openMedia(remoteMedia);
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "Download legal HTTP media did not decode");
    require(backend.downloadState() == QStringLiteral("unavailable"), "Launch flag advertised an absent receiver capability");
    send({QStringLiteral("qplay-download-target"), QStringLiteral("10"), remoteMedia + QStringLiteral("?old")});
    settle();
    require(backend.downloadState() == QStringLiteral("unavailable"), "Stale URL enabled Save");
    send({QStringLiteral("qplay-download-target"), QStringLiteral("11"), remoteMedia});
    wait([&] { return backend.downloadState() == QStringLiteral("ready"); }, "Exact target did not enable Save");
    auto *button = root.findChild<QQuickItem *>(QStringLiteral("downloadButton"));
    require(button && button->isVisible() && button->isEnabled(), "Save control is unavailable");
    const auto point = button->mapToScene(QPointF(button->width() / 2, button->height() / 2));
    for (int click = 0; click < 2; ++click)
    {
        QMouseEvent press(QEvent::MouseButtonPress, point, point, point, Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
        QMouseEvent release(QEvent::MouseButtonRelease, point, point, point, Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
        QCoreApplication::sendEvent(&window, &press);
        QCoreApplication::sendEvent(&window, &release);
    }
    wait([&] { return requests.size() == 1; }, "Save did not emit one exact request");
    require(requests.first().toArray() == QJsonArray {QStringLiteral("qplay-download"), QStringLiteral("11"), remoteMedia},
        "Save requested another token or URL");
    require(backend.downloadState() == QStringLiteral("pending") && !button->isEnabled(), "Double click remained enabled");
    settle();
    require(!backend.paused(), "Disabled Save click paused the playing video");
    send({QStringLiteral("qplay-download-result"), QStringLiteral("10"), QStringLiteral("queued")});
    settle();
    require(backend.downloadState() == QStringLiteral("pending"), "Stale ACK marked Save queued");
    const bool timeoutQa = qEnvironmentVariableIsSet("QPLAY_DOWNLOAD_TIMEOUT_QA");
    if (timeoutQa)
    {
        send({QStringLiteral("qplay-download-result"), QStringLiteral("10"), QStringLiteral("failed")});
        receiver.write(QJsonDocument(QJsonObject {{QStringLiteral("command"),
            QJsonArray {QStringLiteral("set_property"), QStringLiteral("pause"), true}}}).toJson(QJsonDocument::Compact) + '\n');
        receiver.flush();
        wait([&] { return backend.paused(); }, "Timeout fixture did not pause playback");
        wait([&] { return backend.downloadState() == QStringLiteral("failed"); }, "Paused receiver timeout did not expire", 95'000);
        backend.togglePause();
    }
    else
    {
        send({QStringLiteral("qplay-download-result"), QStringLiteral("11"), QStringLiteral("failed")});
        wait([&] { return backend.downloadState() == QStringLiteral("failed"); }, "Failed receiver ACK was reported as success");
    }
    backend.requestDownload();
    wait([&] { return requests.size() == 2; }, "Lost ACK retry did not resend the exact request");
    send({QStringLiteral("qplay-download-result"), QStringLiteral("11"), QStringLiteral("queued")});
    wait([&] { return backend.downloadState() == QStringLiteral("queued"); }, "Durable queue ACK did not reach Save");
    send({QStringLiteral("qplay-download-target"), QStringLiteral("11"), remoteMedia});
    settle();
    backend.requestDownload();
    settle();
    require(backend.downloadState() == QStringLiteral("queued") && requests.size() == 2, "Repeated target reset queued Save");
    backend.openMedia(localMedia);
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "Replacement local video did not decode");
    send({QStringLiteral("qplay-download-target"), QStringLiteral("12"), localMedia});
    send({QStringLiteral("qplay-download-result"), QStringLiteral("11"), QStringLiteral("queued")});
    settle();
    require(backend.downloadState() == QStringLiteral("unavailable") && !button->isVisible(), "Local/replaced media retained Save capability");
    backend.openMedia(remoteMedia);
    wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "Replacement HTTP video did not decode");
    send({QStringLiteral("qplay-download-result"), QStringLiteral("11"), QStringLiteral("queued")});
    settle();
    require(backend.downloadState() == QStringLiteral("unavailable"), "New playback reused the old Save generation");
    backend.stop();
    return {{QStringLiteral("downloadExactRequest"), true}, {QStringLiteral("doubleClickSuppressed"), true},
        {QStringLiteral("downloadFailureRetry"), true}, {QStringLiteral("timeoutWhilePaused"), timeoutQa}, {QStringLiteral("staleTokenAndUrlIgnored"), true},
        {QStringLiteral("localAndReplacementUnavailable"), true}, {QStringLiteral("receiverDeclarationRequired"), true}};
}
