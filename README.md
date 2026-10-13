<p align="center">
  <img src="assets/branding/playstrike-lyrebird-icon.png" alt="PlayStrike lyrebird logo" width="160" height="160">
</p>

# PlayStrike

**by PWDS**

**Press play on your own media.**

PlayStrike is a focused desktop player for files already on your PC. Open or drop a video or audio file, pause and resume, move through it with seek controls, and adjust volume.

### Highlights

- Open local video and audio files.
- Use play, pause, seek, and volume controls.
- Drop a file into the player to start a session.

## Current status

First Windows source slice. Offline unit tests and a headless GStreamer playback test passed, but visible native UI interaction, audible output, broader file compatibility, and runtime packaging still need review. No verified installer is provided.

## Play a file

Open a local file with the picker, drop one into the window, or pass its path at launch. Space pauses or resumes; Left and Right seek five seconds; the seek bar commits when released. Ctrl+O opens the picker. The player displays an error for a file it cannot open.

## Run from source

On Windows, install a Rust toolchain that supports edition 2024 and a GStreamer 1.0 MSVC x86_64 development SDK with the runtime and plugins needed for your media. No GStreamer runtime is bundled. If the SDK is installed at `C:\Program Files\gstreamer\1.0\msvc_x86_64`, set these variables for the current PowerShell session from the repository root; adjust `$gst` for a different location:

```powershell
$gst = 'C:\Program Files\gstreamer\1.0\msvc_x86_64'
$env:PATH = "$gst\bin;$env:PATH"
$env:PKG_CONFIG = "$gst\bin\pkg-config.exe"
$env:PKG_CONFIG_PATH = "$gst\lib\pkgconfig;$gst\lib\gstreamer-1.0\pkgconfig"
$env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gst
$env:GST_PLUGIN_SYSTEM_PATH = "$gst\lib\gstreamer-1.0"
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'target'
cargo run --locked -- path\to\media.mp4
```

Run without a media path to choose or drop a file in the window. After dependencies have been cached, add `--offline` to the Cargo command for an offline build.

Before building again, check disk headroom and follow [VERIFY.md](VERIFY.md). `cargo check` alone does not create a runnable binary.

Format support depends on the installed GStreamer plugins and their codec licenses. PlayStrike does not bundle a codec pack or claim to play every file. Its first video path copies decoded RGBA frames into an egui texture and currently limits preview to 4096×2160; this has a CPU and memory cost, especially for large videos. Hardware decoding may still be selected by GStreamer, but presentation is not zero-copy. Package size and performance have not been measured. Distributing PlayStrike requires packaging the needed native runtime and reviewing plugin licensing.


## Branding and next steps

Original PlayStrike logo artwork is included in [assets/branding](assets/branding) and displayed above. Native app-icon and packaging integration remain pending. Next, inspect the visible Windows player, verify audible output and seek controls with local media, measure large-video CPU and memory use, and review GStreamer runtime packaging before distribution.
