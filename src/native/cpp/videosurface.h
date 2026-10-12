#pragma once

#include <QPointer>
#include <QQuickFramebufferObject>
#include <QtQml/qqmlregistration.h>

#include "qplay_native/src/controller.cxxqt.h"

class VideoSurface : public QQuickFramebufferObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(Qplay::PlayerController *backend READ backend WRITE setBackend NOTIFY backendChanged)

public:
    Qplay::PlayerController *backend() const;
    void setBackend(Qplay::PlayerController *backend);
    Renderer *createRenderer() const override;

signals:
    void backendChanged();

private:
    QPointer<Qplay::PlayerController> m_backend;
};
