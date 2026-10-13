#pragma once

#include <functional>

#include <QJsonObject>
#include <QString>

namespace Qplay { class PlayerController; }
class QQuickItem;
class QQuickWindow;

QJsonObject exerciseDownloads(Qplay::PlayerController &backend, QQuickWindow &window,
    QQuickItem &root, const QString &pipe, const QString &remoteMedia, const QString &localMedia,
    const std::function<void()> &tick);
