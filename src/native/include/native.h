#pragma once

#include <memory>
#include "rust/cxx.h"

class QString;

namespace Qplay
{
class TrackModel;
class PlayerController;
struct StartTask;
struct RetiredSession;
struct UpdateTask;
struct SubtitleTask;
struct GpuTask;

void startSession(::rust::Box<StartTask> task);
void retireSession(::rust::Box<RetiredSession> session);
void startUpdateTask(::rust::Box<UpdateTask> task);
void startSubtitleTask(::rust::Box<SubtitleTask> task);
void startGpuTask(::rust::Box<GpuTask> task);
void scheduleDownloadTimeout(PlayerController &player, const QString &token, int milliseconds);
std::unique_ptr<TrackModel> createTrackModel();
}
