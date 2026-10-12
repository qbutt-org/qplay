#pragma once

#include <memory>
#include "rust/cxx.h"

namespace Qplay
{
class TrackModel;
struct StartTask;
struct RetiredSession;

void startSession(::rust::Box<StartTask> task);
void retireSession(::rust::Box<RetiredSession> session);
std::unique_ptr<TrackModel> createTrackModel();
}
