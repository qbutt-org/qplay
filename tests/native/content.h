#pragma once

#include <functional>
#include <QJsonObject>

class QOpenGLFramebufferObject;
class QString;
namespace Qplay { class PlayerController; }

QJsonObject exerciseContentPolicy(Qplay::PlayerController &backend, QOpenGLFramebufferObject &target,
    const QString &localAnime, const QString &remoteAnime, const QString &preferences,
    const QString &evidencePrefix, const std::function<void()> &tick,
    const std::function<void(const std::function<bool()> &, const char *)> &wait);
