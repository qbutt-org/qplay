# Media tools

`scripts/build.ts` bundles the pinned mpv, FFmpeg, ffprobe and yt-dlp executables with the native qplay profile and verifies archives and files against `upstream-lock.json`. Native `--runtime-info-json` verifies the immutable bundle against `build-manifest.json` before returning tool paths. Neither path runs upstream installers or uses media tools from PATH.

`ffprobe.exe` is used for video analysis and shader benchmarks. The pinned MSYS2 CLANG64 FFmpeg 9.0.2-6 package supplies both FFmpeg and ffprobe. Packaging places these tools and yt-dlp beside their shared DLLs at `engine/`, so callers can execute the discovered absolute paths without changing their working directory or PATH. Player supplies the verified probe path through `QPLAY_FFPROBE`. Original package licenses are retained under `engine/licenses/msys2`; exact package recipes, patches, source archives and dependency sources are in the public source archive pinned by `upstream-lock.json` and identified in `licenses/NOTICE.txt`.

The prepared mpv profile keeps certificate and host-name verification enabled. `profile/certs/cacert.pem` is the pinned certifi Mozilla CA collection, used together with native Windows trust. Its source and license are recorded in `upstream-lock.json` and `licenses/`.

Frame generation runs inside the native player. Rust in `src/native/src/gpu/` owns device selection, pinned downloads, verified native payloads and the per-device engine cache. Preparation runs on the existing Qt worker pool, with cancellation and generation checks. TensorRT builds engines through its native C++ API; the mpv native VapourSynth filter receives a typed configuration and constructs its graph directly through VSAPI.

`framegen-vsov.dll` is the modified GPL-3.0 OpenVINO plugin. Corresponding source, build instructions, modification notice and dependency licenses are included under `profile/tools/framegen-vsov`. The Intel model and its reproducible rewrite remain under `profile/framegen/models/openvino` and `profile/tools/framegen-models`.

Frame-generation license notices are maintained under `licenses/framegen` and copied into the prepared runtime. `Rust-Standard-Library-COPYRIGHT.html` retains the supplied Rust standard-library notices.
