#include "videosurface.h"

#include <cstddef>
#include <optional>

#include <QMetaObject>
#include <QOpenGLContext>
#include <QOpenGLFramebufferObject>
#include <QQuickOpenGLUtils>

namespace
{
void *resolveOpenGL(void *, const char *name)
{
    return reinterpret_cast<void *>(QOpenGLContext::currentContext()->getProcAddress(name));
}

class VideoRenderer : public QQuickFramebufferObject::Renderer
{
public:
    void synchronize(QQuickFramebufferObject *item) override
    {
        if (m_lease || m_failed)
            return;

        auto *backend = static_cast<VideoSurface *>(item)->backend();
        if (!backend || !backend->engineReady())
            return;

        try
        {
            m_lease.emplace(Qplay::acquire_render_lease(*backend));
            m_generation = (*m_lease)->render_generation();
            (*m_lease)->initialize_render(reinterpret_cast<std::size_t>(&resolveOpenGL));
            QMetaObject::invokeMethod(backend, [backend, generation = m_generation]
            {
                backend->surfaceReady(generation);
            }, Qt::QueuedConnection);
        }
        catch (const std::exception &error)
        {
            m_failed = true;
            const QString message = QString::fromUtf8(error.what());
            QMetaObject::invokeMethod(backend, [backend, generation = m_generation, message]
            {
                backend->surfaceFailed(generation, message);
            }, Qt::QueuedConnection);
        }
    }

    void render() override
    {
        if (!m_lease || m_failed)
            return;

        const auto *target = framebufferObject();
        try
        {
            (*m_lease)->draw(static_cast<int>(target->handle()), target->width(), target->height());
        }
        catch (const std::exception &error)
        {
            m_failed = true;
            (*m_lease)->report_render_error(error.what());
        }
        QQuickOpenGLUtils::resetOpenGLState();
    }

private:
    std::optional<rust::Box<Qplay::RenderLease>> m_lease;
    quint64 m_generation = 0;
    bool m_failed = false;
};
}

Qplay::PlayerController *VideoSurface::backend() const
{
    return m_backend;
}

void VideoSurface::setBackend(Qplay::PlayerController *backend)
{
    if (m_backend == backend)
        return;

    if (m_backend)
        disconnect(m_backend, nullptr, this, nullptr);

    m_backend = backend;
    if (backend)
        connect(backend, &Qplay::PlayerController::frameReady, this, &VideoSurface::update);

    emit backendChanged();
    update();
}

QQuickFramebufferObject::Renderer *VideoSurface::createRenderer() const
{
    return new VideoRenderer;
}
