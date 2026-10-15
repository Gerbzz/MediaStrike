# MediaStrike draft player screenshot provenance

`assets/screenshots/mediastrike-draft-player-2026-10-15.png` is a genuine 1120 × 720 PNG returned by the MediaStrike draft application's egui/eframe renderer on 15 October 2026 UTC. It is a capture of the unmerged rename branch, not of the PlayStrike source currently on `main`. The earlier PlayStrike screenshot is preserved separately.

| Item | Identity |
| --- | --- |
| Draft source commit | `092da37953c43f182e35c0043b318aa14568cc09` |
| Hosted Windows build | [GitHub Actions run 38716186675](https://github.com/Gerbzz/MediaStrike/actions/runs/38716186675), artifact `mediastrike-native-preview` (ID `11962563840`) |
| Artifact source manifest | Commit above; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,164,160 bytes, SHA-256 `20673F95B1250A9DBFF1FD6D4315B9AC7EFA1A290611CDE6A97F72683737191D` |
| Local synthetic input | `mediastrike-raw-test.avi`, 346,448 bytes, SHA-256 `C2965BE7F72DA9E25BAA3F1D8CA14690A376631752CB4B844327FE8188F8D7D0` |
| Published screenshot | 48,871 bytes, SHA-256 `C42851D013AD25C2A132517FDD8EE4753F6BFFA8A91391A39A1860A5C1D857CE` |

The hosted run passed two ordinary tests and one explicitly run local-media integration test. Before launching, the downloaded executable and `Cargo.lock` hashes were compared with the artifact manifests and local draft source. The executable was run on Neil's Office PC with its installed GStreamer runtime and the self-generated two-second AVI test pattern, using `playstrike.exe MEDIA_FILE --capture FILE.png`. The capture process exited with status 0; no local compilation was performed. The synthetic file contains no user media and was not published.

The screenshot was opened and visually inspected. It shows a decoded video frame, filename, position bar, 0:00 / 0:02 time display, Pause button, volume slider, and playback status. The controls are light around a dark video canvas. This capture establishes one rendered frame and visible controls. It does not establish completed pause, seek, or volume interactions, audible output, broader codec support, accessibility, or packaged distribution. The renderer screenshot does not include the operating-system window border or title bar.
