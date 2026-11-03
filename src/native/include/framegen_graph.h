#pragma once

#include <cstddef>
#include "VapourSynth4.h"

// mpv_qbutt_vapoursynth_graph_fn ABI. Arguments are borrowed for construction;
// the returned node owns all graph state through VapourSynth free callbacks.
extern "C" VSNode *qplay_create_framegen_graph(const VSAPI *api, VSCore *core,
    VSNode *input, const char *config, char *error, std::size_t errorSize) noexcept;
