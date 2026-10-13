# PlayStrike

PlayStrike is a standalone, local media player in the Strike suite, by PWDS. This first Windows-focused source slice opens a local file and implements video and audio playback, pause/resume, seeking, volume, file drop, and playback errors. The UI and code are original; no third-party design assets are included. Offline unit tests and a headless GStreamer playback test pass; native visual and UI interaction checks are still pending.

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
