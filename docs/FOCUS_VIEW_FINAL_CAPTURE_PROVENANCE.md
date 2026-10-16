# MediaStrike focus view with corrected seek track

These two unedited 1120 x 720 PNGs were written by MediaStrike's own renderer on 16 October 2026 using synthetic local media. They verify the focused layout after its seek track was made to use the available row width. The [earlier focus capture](FOCUS_VIEW_CAPTURE_PROVENANCE.md), queue, and subtitle screenshots remain preserved with separate source records.

| Item | Identity |
| --- | --- |
| App source | `1517f4a14c9c94d17527b3dc9b2f336e3167ba0a` on `codex/mediastrike-focus-view` |
| Hosted Windows build and tests | [GitHub Actions run 38951522969](https://github.com/Gerbzz/MediaStrike/actions/runs/38951522969), artifact `mediastrike-native-preview` (ID `12068593546`, compressed size 6,008,747 bytes) |
| Artifact manifest | `commit=1517f4a14c9c94d17527b3dc9b2f336e3167ba0a`; `Cargo.lock` SHA-256 `90303CE4BA5D7F41B24ED0DD4052F18AF2EB84A4AF65786E45B21C80216D746E` |
| Native executable | `playstrike.exe`, 11,399,168 bytes, SHA-256 `966222A608B86EAA8C8DCBED91F3A797CC98DC8E49294CA5111EC5DB9FC0083E` |
| Local synthetic video | Six-second 640 x 360 black AVI, 4,148,144 bytes, SHA-256 `88C3819DE6C7FBCF252C6946D4DDE3A2304FD736B23946448BFEF27785980D32` |
| Local SRT | Caption `Synthetic subtitle test`, SHA-256 `C3FCF535664444B456BF43464F66D355EFCB93497807D75E7007BA259FAAA7FF` |
| [Focused capture](../assets/screenshots/mediastrike-focus-1517f4a.png) | 40,907 bytes, SHA-256 `0F0495AE68ED33EAAB813018687D6D54D08D1372A65798D945DEF0C27EC5E3F6` |
| [Normal capture](../assets/screenshots/mediastrike-normal-1517f4a.png) | 98,087 bytes, SHA-256 `7150D7890F301E2C7198B4BE5CB95AAA2E7F1F6ACA6B08842BBEE4A004C58EA1` |

The hosted run passed the native build, ten ordinary Rust tests, and eight separately invoked generated-media tests. `SOURCE.txt`, `Cargo.lock`, and `SHA256.txt` matched the local source, lockfile, and downloaded executable. The hosted executable also matched the Windows build log. No local Cargo build was run.

On Neil's Office PC, the installed GStreamer runtime decoded the local AVI and loaded the SRT. With `GST_AUDIOSINK=fakesink`, the exact hosted executable ran as `playstrike.exe VIDEO.avi --subtitle FILE.srt --focus-view --capture OUTPUT.png` and then without `--focus-view` for the normal comparison. Both selected capture processes exited 0. A first focused run from this same executable captured before a caption was visible; its PNG remains under ignored `verification/`. A second focused run visibly rendered the caption and is the published frame. The copied PNG hashes match those selected originals.

Both selected PNGs were opened at native resolution and inspected. The focused frame shows the captioned video across the window, a seek track spanning most of its compact bottom row, and an Exit focus control. The normal frame shows the same media with the identity bar, navigation rail, queue, subtitle control, and transport. These captures verify the launch option and layouts; native F11/button clicks, restoration of queue visibility, audible output, actual display fullscreen, and broader codec compatibility remain unverified. The fixtures and hosted executable remain in ignored `verification/`; approved PWDS and lyrebird assets were not changed.
