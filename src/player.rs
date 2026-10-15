use eframe::egui;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub id: String,
    pub label: String,
}

pub struct Player {
    pipeline: gst::Element,
    bus: gst::Bus,
    frames: Arc<Mutex<Option<Frame>>>,
    frame_error: Arc<Mutex<Option<String>>>,
    pub path: PathBuf,
    pub playing: bool,
    pub volume: f64,
    subtitle_tracks: Vec<SubtitleTrack>,
    selected_non_text: Vec<String>,
    selected_subtitle: Option<String>,
    streams_selected: bool,
    external_subtitle: Option<PathBuf>,
}

impl Player {
    pub fn open(path: &Path, ctx: egui::Context, volume: f64) -> Result<Self, String> {
        Self::open_with_subtitle(path, None, ctx, volume)
    }

    pub fn open_with_subtitle(
        path: &Path,
        subtitle: Option<&Path>,
        ctx: egui::Context,
        volume: f64,
    ) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("File not found: {}", path.display()));
        }
        let external = subtitle.map(subtitle_uri).transpose()?;
        let absolute = path
            .canonicalize()
            .map_err(|e| format!("Cannot resolve {}: {e}", path.display()))?;
        let uri = url::Url::from_file_path(&absolute)
            .map_err(|_| format!("Cannot open this file path: {}", path.display()))?;
        let pipeline = gst::ElementFactory::make("playbin3")
            .build()
            .map_err(|e| format!("GStreamer playbin3 is unavailable: {e}"))?;
        let bus = pipeline
            .bus()
            .ok_or_else(|| "GStreamer did not provide a playback bus".to_owned())?;
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build();
        let sink = gst_app::AppSink::builder()
            .caps(&caps)
            .sync(true)
            .max_buffers(2)
            .drop(true)
            .build();
        let frames = Arc::new(Mutex::new(None));
        let frame_error = Arc::new(Mutex::new(None));
        let preroll_frames = Arc::clone(&frames);
        let sample_frames = Arc::clone(&frames);
        let preroll_error = Arc::clone(&frame_error);
        let sample_error = Arc::clone(&frame_error);
        let preroll_ctx = ctx.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_preroll(move |sink| {
                    let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?;
                    publish_frame(sample, &preroll_frames, &preroll_error, &preroll_ctx)
                })
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    publish_frame(sample, &sample_frames, &sample_error, &ctx)
                })
                .build(),
        );
        pipeline.set_property("uri", uri.as_str());
        if let Some((_, subtitle_uri)) = &external {
            pipeline.set_property("suburi", subtitle_uri.as_str());
        }
        pipeline.set_property("video-sink", &sink);
        pipeline.set_property("volume", volume.clamp(0.0, 1.0));
        if let Err(error) = pipeline.set_state(gst::State::Playing) {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(format!("Playback could not start: {error}"));
        }
        Ok(Self {
            pipeline,
            bus,
            frames,
            frame_error,
            path: path.to_path_buf(),
            playing: true,
            volume,
            subtitle_tracks: Vec::new(),
            selected_non_text: Vec::new(),
            selected_subtitle: None,
            streams_selected: false,
            external_subtitle: external.map(|(path, _)| path),
        })
    }

    pub fn subtitle_tracks(&self) -> &[SubtitleTrack] {
        &self.subtitle_tracks
    }

    pub fn selected_subtitle(&self) -> Option<&str> {
        self.selected_subtitle.as_deref()
    }

    pub fn external_subtitle(&self) -> Option<&Path> {
        self.external_subtitle.as_deref()
    }

    pub fn subtitle_selection_ready(&self) -> bool {
        self.streams_selected
    }

    pub fn select_subtitle(&mut self, id: Option<&str>) -> Result<(), String> {
        if !self.streams_selected {
            return Err("Subtitle tracks are still loading".to_owned());
        }
        if let Some(id) = id {
            if !self.subtitle_tracks.iter().any(|track| track.id == id) {
                return Err("This subtitle track is no longer available".to_owned());
            }
        }
        let mut ids = self.selected_non_text.clone();
        if let Some(id) = id {
            ids.push(id.to_owned());
        }
        let ids: Vec<_> = ids.iter().map(String::as_str).collect();
        if !self
            .pipeline
            .send_event(gst::event::SelectStreams::new(&ids))
        {
            return Err("GStreamer did not accept the subtitle selection".to_owned());
        }
        Ok(())
    }

    fn collect_streams(&mut self, collection: &gst::StreamCollection) {
        for index in 0..collection.size() {
            let Some(stream) = collection.stream(index) else {
                continue;
            };
            if !stream.stream_type().contains(gst::StreamType::TEXT) {
                continue;
            }
            let Some(id) = stream.stream_id().map(|id| id.to_string()) else {
                continue;
            };
            if self.subtitle_tracks.iter().any(|track| track.id == id) {
                continue;
            }
            self.subtitle_tracks.push(SubtitleTrack {
                id,
                label: format!("Subtitle track {}", self.subtitle_tracks.len() + 1),
            });
        }
    }

    pub fn toggle(&mut self) -> Result<(), String> {
        let next = if self.playing {
            gst::State::Paused
        } else {
            gst::State::Playing
        };
        self.pipeline
            .set_state(next)
            .map_err(|e| format!("Could not change playback state: {e}"))?;
        self.playing = !self.playing;
        Ok(())
    }

    pub fn seek(&self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Seek position is invalid".to_owned());
        }
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                gst::ClockTime::from_nseconds((seconds * 1_000_000_000.0) as u64),
            )
            .map_err(|e| format!("This media could not be seeked: {e}"))
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        self.pipeline.set_property("volume", self.volume);
    }

    pub fn position(&self) -> Option<f64> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
    }

    pub fn duration(&self) -> Option<f64> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
    }

    pub fn take_frame(&self) -> Option<Frame> {
        self.frames.lock().ok()?.take()
    }

    pub fn poll_event(&mut self) -> Option<Result<(), String>> {
        let frame_error = self
            .frame_error
            .lock()
            .ok()
            .and_then(|mut error| error.take());
        if let Some(error) = frame_error {
            self.playing = false;
            let _ = self.pipeline.set_state(gst::State::Paused);
            return Some(Err(error));
        }
        while let Some(message) = self.bus.pop() {
            match message.view() {
                gst::MessageView::StreamCollection(collection) => {
                    self.collect_streams(&collection.stream_collection());
                }
                gst::MessageView::StreamsSelected(selected) => {
                    self.collect_streams(&selected.stream_collection());
                    self.selected_non_text.clear();
                    self.selected_subtitle = None;
                    for stream in selected.streams() {
                        let Some(id) = stream.stream_id() else {
                            continue;
                        };
                        if stream.stream_type().contains(gst::StreamType::TEXT) {
                            self.selected_subtitle = Some(id.to_string());
                        } else {
                            self.selected_non_text.push(id.to_string());
                        }
                    }
                    self.streams_selected = true;
                }
                gst::MessageView::Error(error) => {
                    self.playing = false;
                    return Some(Err(format!("Playback failed: {}", error.error())));
                }
                gst::MessageView::Eos(..) => {
                    self.playing = false;
                    let _ = self.pipeline.set_state(gst::State::Paused);
                    let _ = self.seek(0.0);
                    return Some(Ok(()));
                }
                _ => {}
            }
        }
        None
    }
}

fn subtitle_uri(path: &Path) -> Result<(PathBuf, String), String> {
    if !path.is_file() {
        return Err(format!("Subtitle file not found: {}", path.display()));
    }
    let supported = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("srt") || extension.eq_ignore_ascii_case("vtt")
        });
    if !supported {
        return Err("Only local .srt and .vtt subtitle files are supported".to_owned());
    }
    if fs::metadata(path).map_err(|error| error.to_string())?.len() == 0 {
        return Err("Subtitle file is empty".to_owned());
    }
    let absolute = path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve subtitle file: {error}"))?;
    let uri = url::Url::from_file_path(&absolute)
        .map_err(|_| "Cannot form a local subtitle URI".to_owned())?;
    Ok((absolute, uri.to_string()))
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn publish_frame(
    sample: gst::Sample,
    frames: &Mutex<Option<Frame>>,
    frame_error: &Mutex<Option<String>>,
    ctx: &egui::Context,
) -> Result<gst::FlowSuccess, gst::FlowError> {
    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
    let info = gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::NotNegotiated)?;
    let width = info.width() as usize;
    let height = info.height() as usize;
    // This first presentation path has a bounded allocation for one decoded frame.
    if width == 0 || height == 0 || width > 4096 || height > 2160 {
        if let Ok(mut error) = frame_error.lock() {
            *error = Some(format!(
                "Video size {width}×{height} exceeds the current 4096×2160 preview limit"
            ));
        }
        ctx.request_repaint();
        return Err(gst::FlowError::NotNegotiated);
    }
    let stride = usize::try_from(info.stride()[0]).map_err(|_| gst::FlowError::Error)?;
    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
    let pixels = map
        .as_slice()
        .get(info.offset()[0] as usize..)
        .ok_or(gst::FlowError::Error)?;
    let rgba = tight_rgba(pixels, width, height, stride).ok_or(gst::FlowError::Error)?;
    if let Ok(mut slot) = frames.lock() {
        *slot = Some(Frame {
            width,
            height,
            rgba,
        });
        ctx.request_repaint();
    }
    Ok(gst::FlowSuccess::Ok)
}

fn tight_rgba(source: &[u8], width: usize, height: usize, stride: usize) -> Option<Vec<u8>> {
    let row = width.checked_mul(4)?;
    if height == 0 || stride < row {
        return None;
    }
    let needed = (height - 1).checked_mul(stride)?.checked_add(row)?;
    if source.len() < needed {
        return None;
    }
    let mut output = Vec::with_capacity(row.checked_mul(height)?);
    for y in 0..height {
        let start = y.checked_mul(stride)?;
        output.extend_from_slice(&source[start..start + row]);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::{Player, subtitle_uri, tight_rgba};
    use gstreamer as gst;
    use gstreamer::glib::prelude::ObjectExt;
    use gstreamer::prelude::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn removes_video_row_padding_without_mixing_rows() {
        let source = [1, 2, 3, 4, 99, 99, 5, 6, 7, 8, 88, 88];
        assert_eq!(
            tight_rgba(&source, 1, 2, 6),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(tight_rgba(&source[..7], 1, 2, 6), None);
    }

    #[test]
    fn missing_subtitle_file_has_a_clear_error() {
        let error = subtitle_uri(Path::new("missing-subtitle-file.srt")).unwrap_err();
        assert!(error.starts_with("Subtitle file not found:"));
    }

    #[test]
    #[ignore = "requires a generated local AVI and SRT via PLAYSTRIKE_SUBTITLE_VIDEO and PLAYSTRIKE_SUBTITLE_SAMPLE"]
    fn discovers_and_switches_external_subtitle_track() {
        gstreamer::init().expect("GStreamer runtime");
        let video = std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("subtitle video");
        let subtitle = std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("subtitle SRT");
        let mut player = Player::open_with_subtitle(
            Path::new(&video),
            Some(Path::new(&subtitle)),
            eframe::egui::Context::default(),
            0.2,
        )
        .expect("open synthetic video with SRT");
        let canonical_subtitle = Path::new(&subtitle).canonicalize().unwrap();
        assert_eq!(
            player.external_subtitle(),
            Some(canonical_subtitle.as_path())
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && (!player.subtitle_selection_ready() || player.subtitle_tracks().is_empty())
        {
            if let Some(event) = player.poll_event() {
                event.expect("subtitle discovery should not fail");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(player.subtitle_selection_ready());
        let track = player
            .subtitle_tracks()
            .first()
            .expect("external SRT should create a text stream")
            .id
            .clone();
        assert!(player.select_subtitle(Some("not-a-stream-id")).is_err());

        player.select_subtitle(None).expect("turn subtitles off");
        let off_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < off_deadline && player.selected_subtitle().is_some() {
            if let Some(event) = player.poll_event() {
                event.expect("subtitle off should not fail");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(player.selected_subtitle(), None);

        player
            .select_subtitle(Some(&track))
            .expect("select discovered subtitle track");
        let on_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < on_deadline && player.selected_subtitle() != Some(track.as_str()) {
            if let Some(event) = player.poll_event() {
                event.expect("subtitle selection should not fail");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(player.selected_subtitle(), Some(track.as_str()));
    }

    #[test]
    #[ignore = "requires a generated local WebM via PLAYSTRIKE_SAMPLE"]
    fn decodes_and_controls_local_audio_video() {
        gstreamer::init().expect("GStreamer runtime");
        let path = std::env::var_os("PLAYSTRIKE_SAMPLE").expect("PLAYSTRIKE_SAMPLE path");
        let mut player = Player::open(Path::new(&path), eframe::egui::Context::default(), 0.2)
            .expect("open local sample");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_frame = false;
        let mut saw_position = false;
        while Instant::now() < deadline {
            if let Some(event) = player.poll_event() {
                event.expect("GStreamer playback event");
            }
            if let Some(frame) = player.take_frame() {
                assert_eq!((frame.width, frame.height), (320, 180));
                assert_eq!(frame.rgba.len(), 320 * 180 * 4);
                saw_frame = true;
            }
            saw_position |= player.position().is_some_and(|position| position > 0.1);
            if saw_frame && saw_position {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_frame, "decoded video frame should reach the app sink");
        assert!(saw_position, "playback clock should advance");
        assert!(player.duration().is_some_and(|duration| duration > 1.5));
        player.toggle().expect("pause");
        assert!(!player.playing);
        let (result, state, _) = player.pipeline.state(Some(gst::ClockTime::from_seconds(3)));
        assert!(result.is_ok(), "GStreamer should settle after pause");
        assert_eq!(state, gst::State::Paused);
        player.seek(1.0).expect("seek");
        let seek_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < seek_deadline
            && player.position().is_none_or(|position| position < 0.7)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            player.position().is_some_and(|position| position >= 0.7),
            "the native seek position should move near one second"
        );
        player.set_volume(0.5);
        assert_eq!(player.pipeline.property::<f64>("volume"), 0.5);
        player.toggle().expect("resume");
        assert!(player.playing);
        let (result, state, _) = player.pipeline.state(Some(gst::ClockTime::from_seconds(3)));
        assert!(result.is_ok(), "GStreamer should settle after resume");
        assert_eq!(state, gst::State::Playing);
    }
}
