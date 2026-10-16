# MediaStrike external subtitle capture provenance

The two 1120 x 720 PNGs below were written by MediaStrike's egui/eframe renderer on 15 October 2026 UTC. They are unedited captures of a synthetic local video and captions, with no user media. The app source is isolated on `codex/mediastrike-subtitles`, based on the reviewed queue head `74932b920d158bfd4ec10bf18d14c42c0c3f6435`. Earlier player and queue captures remain preserved.

| Item | Identity |
| --- | --- |
| App source | `f27f94d8e574885f5f5fd5d6fa29ae584d283482` |
| Hosted Windows build and tests | [GitHub Actions run 38818480943](https://github.com/Gerbzz/MediaStrike/actions/runs/38818480943); artifact `mediastrike-native-preview` (ID `12012510872`, compressed size 6,003,567 bytes) |
| Artifact manifest | Source commit above; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,378,176 bytes, SHA-256 `ED273A82B2EBDADD8C4338C10631C2BC94090DF5382D06BF0F50A6EB2421B5C7` |
| Local synthetic video | Six-second 640 x 360 black AVI, 4,148,144 bytes, SHA-256 `88C3819DE6C7FBCF252C6946D4DDE3A2304FD736B23946448BFEF27785980D32` |
| Local SRT | Caption `Synthetic subtitle test`, SHA-256 `C3FCF535664444B456BF43464F66D355EFCB93497807D75E7007BA259FAAA7FF` |
| Local WebVTT | Caption `Synthetic VTT caption`, SHA-256 `DA3C51B34EEE76ABC33322F4CB1FB23427A1E1E97D313996D693CEA656B1579E` |
| [SRT capture](../assets/screenshots/mediastrike-subtitles-srt-preview.png) | 96,631 bytes, SHA-256 `7CA6AAA23E1C37E91921EABAC36FF74CED7B8E73ABB6E31197DFFC658579B8FF` |
| [WebVTT capture](../assets/screenshots/mediastrike-subtitles-vtt-preview.png) | 96,534 bytes, SHA-256 `BD1231184065037F3308B2C3423B11CDD9BC3556AD154F9E0088964224511D04` |

The hosted run passed nine normal tests and three explicitly run synthetic-media tests. The subtitle test discovered an external SRT text stream, rejected an invalid track ID, turned text output Off, and restored the selected track; it checked both the player selection and GStreamer's text-output flag. The other two tests exercised playback and queue controls. The downloaded executable, source manifest, and lockfile hashes matched the local branch and hosted log.

On Neil's Office PC, the installed GStreamer runtime generated the local AVI and parsed each external subtitle. The exact hosted executable was launched twice as `playstrike.exe VIDEO.avi --subtitle FILE --capture OUTPUT.png` with `GST_AUDIOSINK=fakesink`; both processes exited 0. No local Cargo build was run. Both PNGs were opened at native resolution and visually inspected: each shows its caption over the black video, the selected file in the queue, the `Subtitles (file)` control, and the MediaStrike title and lyrebird icon. The original PWDS assets in the branch were unchanged. The renderer capture excludes the operating-system window border.

The captures establish initial external SRT and WebVTT rendering. Native clicks of the Subtitles menu, file picker, Off control, and track choices were not exercised. Embedded multi-track media, pause/position restoration after using the picker, audible playback, other caption formats, styling, and synchronization controls remain unverified. The local fixtures and downloaded executable stay under `verification/` and are not published. C: stayed above the agreed 20 GiB free-space floor during capture.
