# MediaStrike video-focused view capture

The two 1120 x 720 PNGs below are unedited app renderer captures from 16 October 2026. They use the same synthetic local video and external SRT caption. The earlier queue and subtitle screenshots remain preserved with their own provenance.

| Item | Identity |
| --- | --- |
| App source | `6e414e772a5a69f372b6c34baf5594e549c45776` on `codex/mediastrike-focus-view` |
| Hosted Windows build and tests | [GitHub Actions run 38949945681](https://github.com/Gerbzz/MediaStrike/actions/runs/38949945681), artifact `mediastrike-native-preview` (ID `12067921643`, compressed size 6,008,199 bytes) |
| Artifact manifest | `commit=6e414e772a5a69f372b6c34baf5594e549c45776`; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,396,608 bytes, SHA-256 `66FC6BEA8829CEF344B103393769E95C8EC878C93A3C96FC7F3DE146EEA7F447` |
| Local synthetic video | Six-second 640 x 360 black AVI, 4,148,144 bytes, SHA-256 `88C3819DE6C7FBCF252C6946D4DDE3A2304FD736B23946448BFEF27785980D32` |
| Local SRT | Caption `Synthetic subtitle test`, SHA-256 `C3FCF535664444B456BF43464F66D355EFCB93497807D75E7007BA259FAAA7FF` |
| [Focused capture](../assets/screenshots/mediastrike-focus-6e414e7.png) | 41,212 bytes, SHA-256 `AAE1362C0C06868631E7C5F6289CE2EBF7EBFC173C7B5DC4295F4A560FEC69EF` |
| [Normal capture](../assets/screenshots/mediastrike-normal-6e414e7.png) | 98,034 bytes, SHA-256 `30986AF56BE36810B2470E1875DA2843FB1419D60EAB4D759A80F0F40BB19490` |

The hosted run passed the native build, ten ordinary Rust tests, and eight separately invoked generated-media tests. Its source manifest, lockfile hash, and executable hash matched the downloaded artifact. On Neil's Office PC, the installed GStreamer runtime supplied decoding and subtitle parsing. With `GST_AUDIOSINK=fakesink`, the exact hosted executable ran as `playstrike.exe VIDEO.avi --subtitle FILE.srt --focus-view --capture OUTPUT.png` and then without `--focus-view` for the normal comparison. Both processes exited 0. No local Cargo build was run. The copied PNG hashes match their capture originals.

Both PNGs were opened at native resolution and inspected. The focused frame shows the captioned video across the window, one bottom transport row, and an Exit focus control. The normal frame shows the same media with the identity bar, navigation rail, queue, subtitle control, and transport. The focus launch option and layout are verified by this capture; F11 and button clicks, restoration of queue visibility, audible output, actual display fullscreen, and broader codec compatibility still need hands-on review. The fixtures and hosted executable remain in ignored `verification/`; approved PWDS and lyrebird assets were not changed.
