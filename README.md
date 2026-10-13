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

Open a local file or drop it into the window. Use the transport to pause, resume, seek, and change volume. A GStreamer runtime is required for playback on Windows; native controls and audible output still need hands-on acceptance.

## Run

PlayStrike uses the native GStreamer runtime and plugins already installed for ClipStrike. If your Windows GStreamer SDK is at `C:\Program Files\gstreamer\1.0\msvc_x86_64`. Set it for the current PowerShell session before building or running:

```powershell
$gst = 'C:\Program Files\gstreamer\1.0\msvc_x86_64'
$env:PATH = "$gst\bin;$env:PATH"
$env:PKG_CONFIG = "$gst\bin\pkg-config.exe"
$env:PKG_CONFIG_PATH = "$gst\lib\pkgconfig;$gst\lib\gstreamer-1.0\pkgconfig"
$env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gst
$env:GST_PLUGIN_SYSTEM_PATH = "$gst\lib\gstreamer-1.0"
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'target'
cargo run --offline -- path\to\media.mp4
```

You can also open or drop a file after launch. Space toggles playback, Left/Right skip five seconds, and Ctrl+O opens the file picker. The seek bar commits a seek when released.

Before building again, check disk headroom and follow [VERIFY.md](VERIFY.md). `cargo check` alone does not create a runnable binary.

Format support depends on the installed GStreamer plugins and their codec licenses. PlayStrike does not bundle a codec pack or claim to play every file. Its first video path copies decoded RGBA frames into an egui texture and currently limits preview to 4096×2160; this has a CPU and memory cost, especially for large videos. Hardware decoding may still be selected by GStreamer, but presentation is not zero-copy. Package size and performance have not been measured. Distributing PlayStrike requires packaging the needed native runtime and reviewing plugin licensing.

REA was reviewed as a reverse-engineering aid, not used as a playback dependency. The earlier local ClipStrike/REA pilot produced no compiled-code finding before its provider timeout; no behavior claim here rests on it.

## Branding and next steps

Original PlayStrike logo artwork is included in [assets/branding](assets/branding) and displayed above. Native app-icon and packaging integration remain pending. Next, inspect the visible Windows player, verify audible output and seek controls with local media, measure large-video CPU and memory use, and review GStreamer runtime packaging before distribution.
