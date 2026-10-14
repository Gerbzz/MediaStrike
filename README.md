<p align="center">
  <img src="assets/branding/playstrike-lyrebird-icon.png" alt="Original PlayStrike lyrebird logo" width="160" height="160">
</p>

# PlayStrike

<p align="center">
  <a href="https://prowebdevsolutions.com"><picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/PWDS-logo-orange.svg">
    <img src="assets/branding/PWDS-logo.png" alt="PWDS company logo" width="120">
  </picture></a>
</p>

**Press play on media you already have.** PlayStrike is a focused Windows desktop player for local video and audio files. Open a file, drop it into the window, or pass its path when launching.

[How to run](#run-from-source) · [Verification](VERIFY.md) · [Original artwork](assets/branding)

## Player preview

![PlayStrike showing a decoded frame from a synthetic local WebM](assets/screenshots/playstrike-player.png)

This [app-rendered capture](docs/SCREENSHOT_PROVENANCE.md) shows a first-party two-second test pattern in the current Windows player. It shows the visible interface; transport interaction and audible output still need hands-on review.

## The player

### Open a local file your way

Use the file picker, drag and drop, or a launch path. The player reports an error when a file cannot be opened. It does not bundle a media library, account, cloud service, or codec pack.

### Stay in control of playback

Play and pause, seek with the bar or five-second steps, and adjust volume in the window. Keyboard shortcuts include **Space** for play/pause, **Left/Right** for seeking, and **Ctrl+O** for the file picker. The seek bar commits its new position when released.

### Decode with the installed GStreamer runtime

GStreamer supplies decoding and playback. Format support depends on the installed plugins and their licenses. The current video path copies decoded RGBA frames into an egui texture and limits preview to 4096×2160; CPU and memory costs for large videos still need measurement. Hardware decoding may be chosen by GStreamer, but presentation is not zero-copy.

## Run from source

Install a Rust toolchain that supports edition 2024 and a **GStreamer 1.0 MSVC x86_64 development SDK**, including the runtime and plugins needed for your media. If installed at the path below, run this in PowerShell from the repository root; adjust `$gst` for another location:

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

Omit the media path to choose or drop a file in the window. Add `--offline` to Cargo after dependencies are cached. Check disk headroom before building and follow [VERIFY.md](VERIFY.md) for the remaining native review.

## Current status

This is a **first Windows source slice**, with no verified installer. Offline unit tests and a headless GStreamer playback test passed. A current app-rendered screenshot verifies the visible player window and one decoded synthetic video frame. Interactive transport behavior, audible output, broader file compatibility, accessibility, and packaging remain unverified. [VERIFY.md](VERIFY.md) separates those checks from the work still needed.

The original [lyrebird artwork](assets/branding) is included; native app-icon integration remains pending. Distributing PlayStrike also requires packaging the native GStreamer runtime and reviewing plugin licenses. The next steps are a visible Windows player review, audible playback and seek checks, performance measurements, and runtime packaging.

Explore the other [PWDS Strike repositories](https://github.com/Gerbzz?tab=repositories&q=Strike).

## PWDS brand artwork

The company mark in this masthead is the existing [official PWDS logo](https://github.com/Gerbzz/PWDS-Website/blob/main/public/prowebdevsolutions-logo.png), with the [official orange display asset](https://prowebdevsolutions.com/images/prowebdevsolutions-logo-orange.svg) for dark backgrounds. Any app logo above it remains the original Strike suite artwork.
