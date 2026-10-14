#pragma once

#include <functional>

#include <QJsonObject>
#include <QString>

namespace Qplay { class PlayerController; }

QJsonObject exerciseStartup(Qplay::PlayerController &backend, const QString &library,
    const QString &media, const QString &secondMedia, const QString &profile,
    const std::function<void()> &tick);
