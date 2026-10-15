# MediaStrike verification

On 13 October 2026, before the MediaStrike rename, `cargo check --offline --tests` and the two normal unit tests passed. The generated WebM passed `gst-discoverer`, and `cargo test --offline decodes_and_controls_local_audio_video -- --ignored` passed against it. A three-second hidden executable launch stayed alive and had a native window handle. On 15 October, the [hosted Windows build](https://github.com/Gerbzz/MediaStrike/actions/runs/38717559048) passed the two normal tests and a synthetic-media playback test, and a [native renderer capture](docs/MEDIASTRIKE_CAPTURE_PROVENANCE.md) verified one decoded frame in the renamed UI. Audible output and completed transport interactions remain unverified. Run the remaining checks only in a coordinated build window after confirming that C: remains above 20 GiB free.

## 1. Create a tiny local audio/video sample

From the MediaStrike repository folder in PowerShell:

```powershell
$gst = 'C:\Program Files\gstreamer\1.0\msvc_x86_64'
$env:PATH = "$gst\bin;$env:PATH"
$env:PKG_CONFIG = "$gst\bin\pkg-config.exe"
$env:PKG_CONFIG_PATH = "$gst\lib\pkgconfig;$gst\lib\gstreamer-1.0\pkgconfig"
$env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gst
$env:GST_PLUGIN_SYSTEM_PATH = "$gst\lib\gstreamer-1.0"
New-Item -ItemType Directory -Path verification -Force | Out-Null
$sample = Join-Path (Get-Location) 'verification\playstrike-sample.webm'
$sampleGst = $sample.Replace('\', '/')
& "$gst\bin\gst-launch-1.0.exe" -q webmmux name=mux ! filesink "location=$sampleGst" `
  videotestsrc pattern=ball num-buffers=60 ! 'video/x-raw,width=320,height=180,framerate=30/1' ! videoconvert ! vp8enc deadline=1 ! queue ! mux. `
  audiotestsrc wave=sine freq=440 volume=0.02 num-buffers=94 ! 'audio/x-raw,rate=48000,channels=2' ! audioconvert ! opusenc ! queue ! mux.
& "$gst\bin\gst-discoverer-1.0.exe" $sample
```

The installed SDK has been checked for these elements. The generated 35 KB sample in `verification/playstrike-sample.webm` is a seekable, two-second VP8/Opus file. Keep it local; it contains only a test pattern and a quiet tone.

## 2. Build and exercise MediaStrike

From the MediaStrike repository folder, choose a target directory with sufficient free space:

```powershell
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'target'
$env:CARGO_BUILD_JOBS = '2'
cargo build --offline
$env:PLAYSTRIKE_SAMPLE = $sample
cargo test --offline decodes_and_controls_local_audio_video -- --ignored
& "$env:CARGO_TARGET_DIR\debug\playstrike.exe" $sample
```

Check that a video frame appears in the MediaStrike window and the quiet audio plays. Confirm the lyrebird logo and MediaStrike name appear on the initial view and in About. Pause and resume, drag the seek bar near the middle, adjust volume, and drop the same file into the window. Then open a missing file to confirm a visible error without losing the current playable file. Record whether end-of-stream can be replayed. Observe CPU/memory only after the player is running; source size or `cargo check` is not performance evidence.

The [current app-rendered capture](assets/screenshots/mediastrike-draft-player-2026-10-15.png) was made with a synthetic AVI and inspected. Its [provenance](docs/MEDIASTRIKE_CAPTURE_PROVENANCE.md) records the source, executable, fixture, and PNG hashes. Repeat a visible review for the interactive and audible checks above.
