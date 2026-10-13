#pragma once

#include <memory>
#include "rust/cxx.h"

namespace Qplay
{
class TrackModel;
struct StartTask;
struct RetiredSession;
struct UpdateTask;
struct SubtitleTask;

void startSession(::rust::Box<StartTask> task);
void retireSession(::rust::Box<RetiredSession> session);
void startUpdateTask(::rust::Box<UpdateTask> task);
void startSubtitleTask(::rust::Box<SubtitleTask> task);
std::unique_ptr<TrackModel> createTrackModel();
}
