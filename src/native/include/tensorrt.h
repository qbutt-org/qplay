#pragma once

#include <cstdint>
#include "rust/cxx.h"

namespace Qplay::TensorRt
{
struct BuildControl;

rust::Vec<std::uint8_t> build_engine(rust::Str directory,
    rust::Slice<const rust::String> resources, rust::Str model,
    std::uint32_t device, std::uint32_t width, std::uint32_t height,
    const BuildControl &control);
void validate_engine(rust::Str directory, rust::Slice<const std::uint8_t> engine,
    std::uint32_t device, std::uint32_t width, std::uint32_t height);
}
