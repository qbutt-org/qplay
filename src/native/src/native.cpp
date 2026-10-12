#include "qplay_native/native.h"

#include <utility>
#include <QThreadPool>

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

std::unique_ptr<Qplay::TrackModel> Qplay::createTrackModel()
{
    return std::make_unique<TrackModel>();
}
