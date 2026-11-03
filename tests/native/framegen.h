#pragma once

#include <functional>
#include <QJsonObject>

class QOpenGLFramebufferObject;
class QString;
namespace Qplay { class PlayerController; }

QJsonObject exerciseFramegen(Qplay::PlayerController &backend, QOpenGLFramebufferObject &target,
    const QString &media, const QString &preferences, const QString &evidencePrefix,
    const std::function<void()> &tick,
    const std::function<void(const std::function<bool()> &, const char *)> &wait);
