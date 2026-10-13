#include "qplay_native/native.h"

#include <utility>
#include <QThreadPool>
#include <QTimer>

#include "qplay_native/src/controller.cxxqt.h"
#include "qplay_native/src/track_model.cxxqt.h"

void Qplay::startSession(::rust::Box<StartTask> task)
{
    QThreadPool::globalInstance()->start([task = std::move(task)]() mutable
    {
        run_start_task(std::move(task));
    });
}

void Qplay::retireSession(::rust::Box<RetiredSession> session)
{
    QThreadPool::globalInstance()->start([session = std::move(session)] {});
}

void Qplay::startUpdateTask(::rust::Box<UpdateTask> task)
{
    QThreadPool::globalInstance()->start([task = std::move(task)]() mutable
    {
        run_update_task(std::move(task));
    });
}

void Qplay::startSubtitleTask(::rust::Box<SubtitleTask> task)
{
    QThreadPool::globalInstance()->start([task = std::move(task)]() mutable
    {
        run_subtitle_task(std::move(task));
    });
}

void Qplay::scheduleDownloadTimeout(PlayerController &player, const QString &token, const int milliseconds)
{
    QTimer::singleShot(milliseconds, Qt::PreciseTimer, &player, [&player, token] { player.downloadTimeout(token); });
}

std::unique_ptr<Qplay::TrackModel> Qplay::createTrackModel()
{
    return std::make_unique<TrackModel>();
}
