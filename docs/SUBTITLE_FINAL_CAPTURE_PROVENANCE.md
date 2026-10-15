# MediaStrike subtitle captures from the verified follow-up source

The two 1120 × 720 PNGs below were written on 15 October 2026 UTC by MediaStrike's own egui/eframe screenshot command. They are unedited app-only captures of a synthetic local black video and captions. No user media or design assets were modified. The previous `f27f94d` captures remain preserved in [their original provenance record](SUBTITLE_CAPTURE_PROVENANCE.md).

| Item | Identity |
| --- | --- |
| App source | `c48564ea5edc81edfd233f5213c589a1e3857ffc` on `codex/mediastrike-subtitles-verification` |
| Hosted Windows build and tests | [GitHub Actions run 38823281278](https://github.com/Gerbzz/MediaStrike/actions/runs/38823281278), artifact `mediastrike-native-preview` (ID `12013908565`, compressed size 6,008,162 bytes) |
| Artifact manifest | `commit=c48564ea5edc81edfd233f5213c589a1e3857ffc`; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,389,952 bytes, SHA-256 `EE5D82922DFB0A17B74C2CCD20E7D7D673193ABD4F0961D47AA85059BC56055A` |
| Local synthetic video | Six-second 640 × 360 black AVI, 4,148,144 bytes, SHA-256 `88C3819DE6C7FBCF252C6946D4DDE3A2304FD736B23946448BFEF27785980D32` |
| Local SRT | Caption `Synthetic subtitle test`, SHA-256 `C3FCF535664444B456BF43464F66D355EFCB93497807D75E7007BA259FAAA7FF` |
| Local WebVTT | Caption `Synthetic VTT caption`, SHA-256 `DA3C51B34EEE76ABC33322F4CB1FB23427A1E1E97D313996D693CEA656B1579E` |
| [Fresh SRT capture](../assets/screenshots/mediastrike-subtitles-srt-c48564e.png) | 96,641 bytes, SHA-256 `B2BFDDCEF5FB61EA8C04F8EBEC9994A90FD4F59AED6E85722FBD0975F1CAAF94` |
| [Fresh WebVTT capture](../assets/screenshots/mediastrike-subtitles-vtt-c48564e.png) | 96,521 bytes, SHA-256 `EB078BCB0458228AAE5FE80678639470A24E8BA63857E6537E949761ADC1C524` |

The artifact's `SOURCE.txt` and `SHA256.txt` matched the local `Cargo.lock` and downloaded executable; its executable hash also matched the hosted log. On Neil's Office PC, the installed GStreamer runtime supplied decoding and subtitle parsing. With `GST_AUDIOSINK=fakesink`, the exact hosted executable ran twice as `playstrike.exe VIDEO.avi --subtitle FILE --capture OUTPUT.png`, once with SRT and once with WebVTT. Both exited 0. The captured files were copied byte-for-byte into `assets/screenshots`; source and destination hashes matched. No local Cargo build was run.

Both PNGs were opened at native resolution and visually inspected. Each shows the expected readable caption over the black video, MediaStrike title and lyrebird icon, selected AVI in the queue, and `Subtitles (file)` control. These captures verify initial external caption rendering by source `c48564e`. They do not show pointer interaction with the Subtitles menu or Windows file picker. Native desktop control is unavailable in this task's computer-use surface, so Off, track switching, and dialog cancellation were verified by generated-media tests through app methods rather than native clicks. Audio was intentionally routed to `fakesink`, so audible output remains unverified. The downloaded executable and source fixtures stay under ignored `verification/`; the original PWDS brand assets remain unchanged. C: stayed above the agreed 20 GiB free-space floor.
