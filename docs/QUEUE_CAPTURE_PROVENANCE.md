# MediaStrike queue-layout screenshot provenance

`assets/screenshots/mediastrike-queue-preview.png` is a genuine 1120 x 720 PNG returned by the MediaStrike queue branch's egui/eframe renderer on 15 October 2026 UTC. The queue branch is a draft PR based on the separate MediaStrike foundation draft. The earlier MediaStrike and PlayStrike captures remain in the repository.

| Item | Identity |
| --- | --- |
| Queue source commit | `c97773268ae44d178d9d60a55109f6e406210971` |
| Hosted Windows build | [GitHub Actions run 38775292474](https://github.com/Gerbzz/MediaStrike/actions/runs/38775292474), artifact `mediastrike-native-preview` (ID `11992634615`) |
| Artifact source manifest | Commit above; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,239,424 bytes, SHA-256 `C357DCF3A3F75CDD4B3EC93B79E4D80FE7C3BA06FB7CF84A89E0D6B96F2C13AE` |
| First local synthetic input | `mediastrike-raw-test.avi`, 346,448 bytes, SHA-256 `C2965BE7F72DA9E25BAA3F1D8CA14690A376631752CB4B844327FE8188F8D7D0` |
| Second local synthetic input | `second-synthetic-file.avi`, 346,448 bytes, same SHA-256 as the first; copied under a distinct filename to show two queue entries |
| Published screenshot | 95,369 bytes, SHA-256 `C83C37CBB2B5AFCEC54190511FBE74BAFF4A0019FEE344DDFD1E562769FFF529` |

The hosted run passed six normal tests and one explicitly run GStreamer media test. The downloaded artifact's commit, lockfile hash, and executable hash were checked against the local draft source and artifact manifests. The executable was run on Neil's Office PC with its installed GStreamer runtime and the two local synthetic AVIs using `playstrike.exe FIRST.avi SECOND.avi --capture FILE.png`; `GST_AUDIOSINK=fakesink` prevented audio output. The process exited with status 0. No local compilation was performed. Both fixtures contain the same self-generated two-second video test pattern, no user media, and were not published.

The screenshot was opened at native resolution and visually inspected. It shows two named entries in the right queue, the first selected, a decoded frame, the playback rail, MediaStrike title, and original lyrebird artwork. The near-black panels and orange emphasis are an independent implementation informed by [Juxtopposed's VLC redesign video](https://www.youtube.com/watch?v=ERFeOgqk324); no Figma or VLC assets were copied. This capture verifies visible queue population and one rendered frame. It does not verify clicking between entries, end-of-stream handoff, audible playback, completed transport interactions, broad codec support, accessibility, or distribution packaging. The app renderer screenshot excludes the operating-system window border and title bar.

Before the final download and capture, C: had more than 23,000,000,000 free bytes, above the agreed 20 GiB floor. The capture itself is the same bytes as the inspected local file.
