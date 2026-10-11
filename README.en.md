[Русский](README.md) / **English**

# qplay

[![Windows x64](https://img.shields.io/badge/Windows-x64-0078d4)](https://qbutt-org.github.io/qplay/en/)
[![Latest release](https://img.shields.io/github/v/release/qbutt-org/qplay?label=release&color=009df7)](https://github.com/qbutt-org/qplay/releases/latest)
[![Licenses](https://img.shields.io/badge/license-GPL-blue)](LICENSE)

A video player based on [mpv](https://mpv.io/).

[![Download for Windows](docs/assets/download.en.svg)](https://github.com/qbutt-org/qplay/releases/latest/download/qplay-setup.exe)

[Website](https://qbutt-org.github.io/qplay/en/) / [Portable and all downloads](https://github.com/qbutt-org/qplay/releases/latest) / [What's new](https://github.com/qbutt-org/qplay/releases)

## Installation

Run **qplay-setup.exe**. qplay supports Windows 10 version 2004 or later and Windows 11, x64. qplay appears in the Start menu and the Windows apps list.

For the portable version, extract **qplay-portable.zip** and run `qplay.exe`.

Open a media file in qplay, drag it into the window, or paste a direct URL into the start window. qplay appears in the Windows **Open with** menu for common video and audio formats. It does not replace your chosen default player.

## Controls and image quality

The bundle includes the ModernX interface: a timeline, volume, track selection, and subtitles. Picture processing settings are available from the player. Upscaling and frame generation use the GPU; available modes and performance depend on the hardware.

Basic playback works offline from the installed bundle. GPU modes may automatically download and prepare additional pinned components.

## qbutt and qcatch

[qbutt](https://github.com/qbutt-org/qbutt) uses qplay for online video and watching torrent files while they download. The qbutt installer installs this same independent player.

[qcatch](https://github.com/qbutt-org/qcatch) finds video in the browser and opens the selected stream in the installed qplay.

## Settings

The installed version stores settings in `%APPDATA%\qplay`, and caches and prepared GPU components in `%LOCALAPPDATA%\qplay`. The portable version uses the `userdata` folder next to the app. Existing mpv and mpv.net settings are not replaced.

Settings include language, resuming from the last position, and automatic frame generation. Updates are checked there too: the installed version downloads a verified EXE, while the portable version offers a new archive.

qplay accepts ordinary mpv options. The player controls the paths to its engine and its own profile.

## Development

Windows launcher sources are in `src/`; the prepared player profile is in `profile/`. Pinned dependencies and runtime provenance are recorded in `upstream-lock.json`.

The local build uses Bun 1.4.2 and the built-in .NET Framework compiler. The .NET SDK is not required. Release validation used the following command; substitute your paths to the pinned runtime archive and Inno Setup 6.7.3 `ISCC.exe`:

```powershell
bun scripts/build.ts --runtime-archive "C:\build\qbutt-media-runtime-1.3.0.zip" --installer-compiler "C:\tools\Inno Setup 6\ISCC.exe"
```

The script verifies the runtime SHA-256 hash and creates the installer and portable archive in `%TEMP%\qplay-build`; use `--output` for another directory. `bun run build` can prepare dependencies automatically, but the Inno Setup bootstrap in that mode has not been validated yet.

`qplay.exe --version-json` and `qplay.exe --runtime-info-json` return integration metadata. The launcher stays alive until its engine exits and owns the engine's lifecycle. The calling app supplies media and playback options.

See the [project rules](AGENTS.md).

## Licenses and provenance

The bundle includes ModernX, Lua scripts, shaders, and media dependencies with their authors' notices preserved. Licenses are in [licenses/](licenses/), and qplay source terms are in [LICENSE](LICENSE).

Releases publish pinned sources and runtime contents alongside the binaries.
