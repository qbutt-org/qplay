# qplay

qplay is an independent Windows media player built on mpv. It also supplies the same installed player to qbutt and qcatch.

## Ownership

- `src/` owns the small Windows launcher, file/URL opening, settings, updater and engine lifecycle. Keep mpv as the playback engine rather than duplicating it.
- `profile/` owns immutable prepared mpv defaults, ModernX, shaders and frame generation. Keep cohesive behavior in its existing Lua modules. Preserve mouse-first controls, live DPI handling, downgrade-only frame generation during a video, TLS validation and the bundled trust store.
- `upstream-lock.json` pins the imported public runtime, exact dependency hashes and corresponding sources. Do not import private repository history, local profiles, credentials or generated caches.
- Installed preferences live in `%APPDATA%/qplay`; state and generated GPU runtimes live in `%LOCALAPPDATA%/qplay`. Portable builds use their own `userdata` directory. Never replace existing mpv/mpv.net profiles or Windows UserChoice.
- `--version-json` and `--runtime-info-json` are protocol version 1. Runtime metadata verifies the immutable build manifest before returning tool paths and configuration/data directories. Each discovered absolute tool path must run without the caller changing its working directory or PATH. Callers discover qplay through its per-user product registration or App Paths and use this public metadata, never internal engine/profile paths. Standard mpv options pass through, except qplay owns its configuration, installed scripts and engine paths. The launcher remains alive until its engine exits; terminating the launcher must close its owned child processes.
- qbutt owns its content library/history, AI subtitle translation and verified torrent Range endpoint. qcatch owns capture and standalone downloads. qplay supplies playback and the optional external-download IPC action without duplicating those services.

## Product interface

Keep the main interface and README available in Russian and English with a visible language switch. The product site follows the browser language on first visit and preserves an explicit language choice. Preserve the `#009df7` accent. Use actual application screenshots cropped to the product window, without desktop or OS background; dark VM captures are acceptable.

Review public UI, site and README copy in two separate passes in every supported language. First rewrite descriptions as natural, connected statements of concrete actions, capabilities and constraints; remove slogans, invented contrasts, mechanical triples, dramatic fragments and unsupported promises. Keep control names and subject headings short. Then remove unnecessary words, sentences and repetitions one at a time, reread in context and verify the rendered UI and affected action. Preserve meaning and the information needed for the next action; repeat until a full pass finds no further removal. Do not use `·` in user-facing text, including UI, sites and README; do not use `•` or `●` as text separators. Use layout, ordinary punctuation or brackets.

## Work and verification

Prioritize clear, simple final code and preserve unrelated work. After meaningful edits review the diff for abandoned branches, unused state, duplicated ownership and unnecessary wrappers. After a working result remove dispensable code and UI, then repeat affected checks until nothing else can be removed without losing clarity or required behavior.

Write product code, scripts and configuration for the person who will read and maintain them. Keep related logic together, separate independent blocks with blank lines and expand complex control flow; an agent's ability to search dense code does not make that representation maintainable. Split files by coherent responsibilities without fragmenting cohesive logic into wrappers, and keep manually edited configuration indented and multiline.

Use Bun/TypeScript for packaging and integration tooling. Build locally with `bun run build`; never add automatic GitHub build triggers. Do not write unit tests or unsolicited Markdown work reports. Validate legal media fixtures through actual mpv, redirected launcher metadata, process lifetime, settings persistence, fresh/repeated installation and the affected native mouse controls. A successful build is not playback or GPU evidence.

The installer and portable archive must contain complete offline base playback. GPU-specific frame generation may prepare pinned runtime downloads automatically while ordinary playback remains available. Preserve imported license headers and provenance.

## Delivery

The public repository is `qbutt-org/qplay`, with only `main` published. Review the full outgoing diff and explicitly stage paths. Build and validate final artifacts before publishing. Never publish private history or secrets, and verify actual repository visibility, source revision and uploaded artifact digests.

Product releases upload only `qplay-setup.exe` and `qplay-portable.zip`. GitHub provides qplay source archives at the exact release commit. Publish the complete mpv/dependency source archive in a separate `v<version>-sources` release marked not latest, linked directly from the product release body.
