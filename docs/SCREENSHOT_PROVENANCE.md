# PlayStrike player screenshot provenance

`assets/screenshots/playstrike-player.png` is a genuine 1120 x 720 screenshot returned by PlayStrike's egui/eframe Glow renderer on 14 October 2026 UTC. It shows the native player, a decoded frame, seek bar, transport controls, status, and filename. The local input was a self-generated two-second, 35,038-byte WebM with a test-pattern ball video and quiet synthetic tone. It contains no user media or client asset and was not uploaded.

The first-party source adds `playstrike MEDIA_FILE --capture FILE.png`. The capture waits for a decoded texture, allows two painted frames, requests `egui::ViewportCommand::Screenshot`, writes the returned `egui::Event::Screenshot` as PNG, and closes its window. Ordinary `playstrike [MEDIA_FILE]` playback remains available. The capture process exited with status 0.

| Item | SHA-256 |
| --- | --- |
| Local synthetic WebM fixture (not published) | `E6F393BEB3CCCE729E3B53EE9DE8053E5D66D3C4FB25F5C0CB2E49F60627D8CE` |
| Local capture executable (not published) | `DFEE33ABB26065B785F8BFDCD6C9BF0433C129DBF831DD69F08CCEA0328BFBFB` |
| Published screenshot PNG | `22407C3DA9FC65473B40B8C1CAAB8985E8C9E220127A5C11AEC7AD37A211DAC7` |

The capture executable was built from this repository's source plus the PNG dependency with `cargo build --locked --offline -j 1 -p playstrike --bin playstrike` against a pre-existing shared target. Cargo reported a successful dev build in 7.30 seconds. The per-second monitor observed 20,105,891 bytes of net target growth at completion, below its 750 MiB limit, and C: remained above its 9 GiB free-space floor. No download, clean, installer, release, or codec packaging was made. The repository's existing MIT license was preserved.

This screenshot establishes one visible decoded frame and interface state. It does not establish audible playback, a completed pause/seek/volume interaction, other codecs, accessibility, or a distributable Windows package. The earlier unit and headless GStreamer tests were not rerun after the capture-only source change.
