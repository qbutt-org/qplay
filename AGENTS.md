# qplay

qplay is an independent Windows media player built on mpv. It also supplies the same installed player to qbutt and qcatch.

## Ownership

- The player is one Rust/CXX-Qt application with QML/Qt Quick presentation and libmpv inside its process. `src/native/` owns the native session, settings and typed Qt boundary; the Qt render hook owns OpenGL resources on the render thread. Use the pinned mpv engine and libraries. Startup and native teardown run outside the GUI thread and application Exit drains accepted work. Do not introduce a launcher process, a second player profile or another application runtime for native APIs.
- `profile/` contains the bundled trust store, shaders and frame-generation filters. Native Rust owns playback and GPU policy; QML owns presentation. Preserve mouse-first controls, live DPI handling, downgrade-only frame generation during a video and TLS validation. Explicit subtitle placement must survive control visibility changes and reopening playback without replacing authored ASS styles. The shared qbutt `docs/implementation.md` owns migration scope and acceptance.
- `upstream-lock.json` pins the imported public runtime, exact dependency hashes and corresponding sources.
- Installed preferences live in `%APPDATA%/qplay`; state and generated GPU runtimes live in `%LOCALAPPDATA%/qplay`. Portable builds use their own `userdata` directory. Never replace existing mpv/mpv.net profiles or Windows UserChoice.
- `--version-json` and `--runtime-info-json` are currently protocol version 1. Runtime metadata verifies the immutable build manifest before returning tool paths and configuration/data directories. Each discovered absolute tool path must run without the caller changing its working directory or PATH. Callers discover qplay through its per-user product registration or App Paths and use this public metadata, never internal engine/profile paths. qplay owns its configuration, installed scripts and engine paths. Preserve the public playback/IPC contract until its consumers are coordinated; shutting down qplay releases its native session and owned helper processes.
- qplay owns subtitle translation, its settings, original/translated track selection and cache, including standalone playback without qbutt. Reuse the shared Rust Codex transport with qbutt's reader; keep media/track identity, cancellation and subtitle evidence in qplay. qbutt owns its content library/history, reader translation and verified torrent Range endpoint; qcatch owns capture and standalone downloads. The shared implementation plan tracks replacement of the remaining qbutt subtitle service and optional external-download integration.

## Product interface

Keep the main interface and README available in Russian and English with a visible language switch. The product site follows the browser language on first visit and preserves an explicit language choice. Preserve the `#009df7` accent. Use actual application screenshots cropped to the product window, without desktop or OS background.

Review public UI, site and README copy in two separate passes in every supported language. First rewrite descriptions as natural, connected statements of concrete actions, capabilities and constraints; remove slogans, invented contrasts, mechanical triples, dramatic fragments and unsupported promises. Keep control names and subject headings short. Then remove unnecessary words, sentences and repetitions one at a time, reread in context and verify the rendered UI and affected action. Preserve meaning and the information needed for the next action; repeat until a full pass finds no further removal. Do not use `·` in user-facing text, including UI, sites and README; do not use `•` or `●` as text separators. Use layout, ordinary punctuation or brackets.

## Work and verification

Prioritize clear, simple final code and preserve unrelated work. After meaningful edits review the diff for abandoned branches, unused state, duplicated ownership and unnecessary wrappers. After a working result remove dispensable code and UI, then repeat affected checks until nothing else can be removed without losing clarity or required behavior.

Write product code, scripts and configuration for the person who will read and maintain them. Keep related logic together, separate independent blocks with blank lines and expand complex control flow; an agent's ability to search dense code does not make that representation maintainable. Split files by coherent responsibilities without fragmenting cohesive logic into wrappers, and keep manually edited configuration indented and multiline.

Use Bun/TypeScript for packaging and integration tooling; never add automatic GitHub build triggers. Do not write unit tests or unsolicited Markdown work reports. Validate legal media fixtures through actual libmpv, runtime metadata, process lifetime, settings persistence, fresh/repeated installation and the affected native mouse controls. Use the isolated AppSandbox VM for focus-taking interaction. A successful build is not playback or GPU evidence.

The root CMake project builds the native player with Qt 6.12 and the pinned Rust toolchain. After configuring it, `bun run build -- --native-build <configured-build-directory>` packages the binary with the pinned engine and Qt dependency closure, checking that compiled and packaged native sources match. Its development targets are `qplay-native`, `qplay-native-acceptance` and `all_qmllint`. Run `bun tests/native/player.ts <build directory> <qplay runtime> <legal media> <Qt SDK> <evidence directory>` after building them. The acceptance process renders the actual Qt/libmpv scene offscreen with an isolated profile; this proves playback and control behavior, not native pointer, audible output, HDR or final interface acceptance. Keep evidence outside the checkout.

The installer and portable archive must contain complete offline base playback. GPU-specific frame generation may prepare pinned runtime downloads automatically while ordinary playback remains available. Preserve imported license headers and provenance.

## Delivery

The public repository is `qbutt-org/qplay`, with only `main` published. Review the full outgoing diff and explicitly stage paths. Build and validate final artifacts before publishing. Never publish private history, secrets, personal profiles or generated caches, and verify actual repository visibility, source revision and uploaded artifact digests.

Product releases upload only `qplay-setup.exe` and `qplay-portable.zip`. GitHub provides qplay source archives at the exact release commit. Publish the complete mpv/dependency source archive in a separate `v<version>-sources` release marked not latest, linked directly from the product release body.
