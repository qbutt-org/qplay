# Media tools

`scripts/build.ts` bundles the pinned mpv, FFmpeg, ffprobe and yt-dlp executables with the native qplay profile and verifies archives and files against `upstream-lock.json`. Native `--runtime-info-json` verifies the immutable bundle against `build-manifest.json` before returning tool paths. Neither path runs upstream installers or uses media tools from PATH.

`ffprobe.exe` is used for video analysis and shader benchmarks. The pinned MSYS2 CLANG64 FFmpeg 9.0.2-6 package supplies both FFmpeg and ffprobe. Packaging places these tools and yt-dlp beside their shared DLLs at `engine/`, so callers can execute the discovered absolute paths without changing their working directory or PATH. Player supplies the verified probe path through `QPLAY_FFPROBE`. Original package licenses are retained under `engine/licenses/msys2`; exact package recipes, patches, source archives and dependency sources are in the public source archive pinned by `upstream-lock.json` and identified in `licenses/NOTICE.txt`.

The prepared mpv profile keeps certificate and host-name verification enabled. `profile/certs/cacert.pem` is the pinned certifi Mozilla CA collection, used together with native Windows trust. Its source and license are recorded in `upstream-lock.json` and `licenses/`.

`framegen-manager.exe` selects the available NVIDIA/TensorRT or Intel/OpenVINO backend and prepares its pinned runtime in qplay's data directory. Its source, Cargo lock, package recipe and runtime catalog are maintained under `profile/tools/framegen-manager`.

The native player keeps content detection, shader and frame-rate policy, quality monitoring and libmpv commands in `src/native/src/gpu/` and its controller. Its entry point sets `QPLAY_MEDIA_RUNTIME` to the `framegen` directory under player data, which both the manager and filter use. It starts the existing manager only when a compatible video needs an absent or damaged frame-generation runtime. That installer remains a bounded child in a kill-on-close Windows job; media changes cancel it. VapourSynth, vs-mlrt, `prepare.py` and `framegen.vpy` remain the model compilation and filter engine boundary. The native player does not embed the installer's download and archive stack.

`framegen-bootstrap.dll` is the mpv C plugin that validates the selected manager manifest and loads its portable VSScript into the running player. Its source and package recipe are under `profile/tools/framegen-bootstrap`.

`framegen-vsov.dll` is the modified GPL-3.0 OpenVINO plugin. Corresponding source, build instructions, modification notice and dependency licenses are included under `profile/tools/framegen-vsov`.

The manager and bootstrap third-party notices are generated from their exact Cargo locks. `Rust-Standard-Library-COPYRIGHT.html` records the licenses from their pinned Rust toolchain. The component package recipes check these notices and compiler identity before producing a binary.
