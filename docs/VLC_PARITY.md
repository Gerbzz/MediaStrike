# MediaStrike's VLC-parity work

The product goal is VLC-class desktop media capability in a Rust application with a player layout informed by [Juxtopposed's VLC redesign video](https://www.youtube.com/watch?v=ERFeOgqk324) and its linked [Figma community file](https://www.figma.com/community/file/1691465999134370980). The video shows a near-black playback canvas, restrained orange accents, a compact transport rail, a left library rail, and an optional playlist panel (notably around 9:30 and 13:30). MediaStrike implements its own layout and keeps the approved PWDS lyrebird artwork. The Figma canvas and reuse terms have not been verified; no design assets are copied.

The feature groups below come from VideoLAN's [desktop basic usage](https://docs.videolan.me/vlc-user/desktop/3.0/en/basic/index.html) and [advanced usage](https://docs.videolan.me/vlc-user/desktop/3.0/en/advanced/index.html). This is a working coverage map, not a claim that every VLC module has been enumerated. "Built" means source exists; the verification column states what was actually exercised.

| Area | Current MediaStrike implementation | Verification and remaining work |
| --- | --- | --- |
| Local playback | Open local video/audio by picker, drop, or launch path; play/pause, seek, volume; session queue with next/previous and repeat | Synthetic AVI decoded on hosted Windows and appeared in a native renderer capture. Queue behavior has unit tests. Multi-file UI interaction and audible output still need hands-on review. |
| Playlist and library | Rust-managed session queue, selection, and optional right panel | No saved playlists, media scanning, search, sort, reorder, metadata index, or persistent library yet. |
| Video presentation | Decoded frames copied to an egui texture, bounded to 4096 x 2160 | No fullscreen control, snapshot command, aspect/crop, deinterlace, HDR, 360-degree display, or measured large-video performance. |
| Audio | GStreamer audio sink and volume control | No track/output selection, equalizer, visualization, effects, or verified audible playback. |
| Subtitles | No explicit controls | Embedded/external subtitle selection, styling, and synchronization remain. |
| Navigation | File queue and time seek | Bookmarks, chapters, titles, programs, and recent media remain. |
| Sources | Local files only | Network streams, discs, capture devices, and optical media remain. |
| Creation and streaming | None | Recording, conversion/transcoding, stream output, and remote control remain. |
| Extensibility and settings | About view only | Preferences, keyboard customization, plugins/extensions, profiles, and accessibility review remain. |
| Distribution | Source and hosted preview build | No licensed runtime bundle, installer, clean-machine test, or release artifact. |

The application and queue/UI logic are written in Rust. Decoding and media pipeline behavior currently come from the native GStreamer runtime and installed plugins; MediaStrike is not a pure-Rust codec implementation. VLC's scope also includes codecs and protocols beyond the installed GStreamer setup. Codec, plugin, and design-asset licenses must be checked before any distribution or reuse.
