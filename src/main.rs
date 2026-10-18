mod player;
mod queue;

use eframe::egui;
use player::Player;
use queue::MediaQueue;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MUTED_TEXT: egui::Color32 = egui::Color32::from_rgb(174, 174, 174);
const PLAYBACK_SPEEDS: [f64; 4] = [0.5, 1.0, 1.5, 2.0];

fn next_speed(rate: f64) -> f64 {
    let index = PLAYBACK_SPEEDS
        .iter()
        .position(|candidate| *candidate == rate)
        .unwrap_or(1);
    PLAYBACK_SPEEDS[(index + 1) % PLAYBACK_SPEEDS.len()]
}

#[derive(Default, Debug, PartialEq, Eq)]
struct PlayerShortcuts {
    open_files: bool,
    toggle: bool,
    back: bool,
    forward: bool,
    previous: bool,
    next: bool,
    focus_view: bool,
    mute: bool,
}

fn player_shortcuts(ctx: &egui::Context) -> PlayerShortcuts {
    // Read shortcuts after TextEdit has processed this frame. This also catches
    // the first click into Filter queue when a key arrives in the same frame.
    if ctx.wants_keyboard_input() {
        return PlayerShortcuts::default();
    }
    ctx.input(|input| PlayerShortcuts {
        open_files: input.modifiers.command && input.key_pressed(egui::Key::O),
        toggle: input.key_pressed(egui::Key::Space),
        back: !input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft),
        forward: !input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight),
        previous: input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft),
        next: input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight),
        focus_view: input.key_pressed(egui::Key::F11),
        mute: input.key_pressed(egui::Key::M),
    })
}

struct CaptureOnce {
    path: PathBuf,
    subtitle_path: Option<PathBuf>,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
    started: Instant,
    settle_frames: u8,
    requested: bool,
}

struct PendingSubtitleLoad {
    player: Player,
    path: PathBuf,
    started: Instant,
}

impl CaptureOnce {
    fn new(
        path: PathBuf,
        subtitle_path: Option<PathBuf>,
        outcome: Arc<Mutex<Option<Result<(), String>>>>,
    ) -> Self {
        Self {
            path,
            subtitle_path,
            outcome,
            started: Instant::now(),
            settle_frames: 0,
            requested: false,
        }
    }
}

struct MediaStrike {
    player: Option<Player>,
    queue: MediaQueue,
    show_queue: bool,
    queue_search: String,
    show_browser: bool,
    browser_folder: Option<PathBuf>,
    browser_items: Vec<PathBuf>,
    browser_identities: Vec<Option<PathBuf>>,
    browser_search: String,
    browser_ascending: bool,
    focus_view: bool,
    texture: Option<egui::TextureHandle>,
    brand_texture: Option<egui::TextureHandle>,
    show_about: bool,
    seek_preview: Option<f64>,
    volume: f64,
    muted: bool,
    speed: f64,
    status: String,
    error: Option<String>,
    gst_ready: bool,
    capture: Option<CaptureOnce>,
    pending_subtitle_load: Option<PendingSubtitleLoad>,
    #[cfg(test)]
    queue_filter_rect: Option<egui::Rect>,
    #[cfg(test)]
    ui_mute_rect: Option<egui::Rect>,
    #[cfg(test)]
    ui_speed_rect: Option<egui::Rect>,
}

impl MediaStrike {
    fn new(
        cc: &eframe::CreationContext<'_>,
        media_paths: Vec<PathBuf>,
        subtitle_path: Option<PathBuf>,
        focus_view: bool,
        capture: Option<CaptureOnce>,
    ) -> Self {
        let (gst_ready, error) = match gstreamer::init() {
            Ok(()) => (true, None),
            Err(err) => (false, Some(format!("GStreamer could not start: {err}"))),
        };
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(19, 19, 19);
        visuals.window_fill = egui::Color32::from_rgb(29, 29, 29);
        visuals.selection.bg_fill = egui::Color32::from_rgb(180, 82, 11);
        visuals.hyperlink_color = egui::Color32::from_rgb(255, 147, 47);
        visuals.override_text_color = Some(egui::Color32::from_rgb(224, 224, 224));
        visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(224, 224, 224);
        visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(218, 218, 218);
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_visuals(visuals);
        let mut app = Self {
            player: None,
            queue: MediaQueue::default(),
            show_queue: true,
            queue_search: String::new(),
            show_browser: false,
            browser_folder: None,
            browser_items: Vec::new(),
            browser_identities: Vec::new(),
            browser_search: String::new(),
            browser_ascending: true,
            focus_view: false,
            texture: None,
            brand_texture: load_brand_texture(&cc.egui_ctx),
            show_about: false,
            seek_preview: None,
            volume: 0.8,
            muted: false,
            speed: 1.0,
            status: "Open or drop a local media file".to_owned(),
            error,
            gst_ready,
            capture,
            pending_subtitle_load: None,
            #[cfg(test)]
            queue_filter_rect: None,
            #[cfg(test)]
            ui_mute_rect: None,
            #[cfg(test)]
            ui_speed_rect: None,
        };
        if app.gst_ready && !media_paths.is_empty() {
            app.enqueue_media(media_paths, true, cc.egui_ctx.clone());
            if let Some(subtitle_path) = subtitle_path {
                if let Err(error) = app.load_external_subtitle(subtitle_path, cc.egui_ctx.clone()) {
                    app.error = Some(error);
                }
            }
        }
        app.focus_view = focus_view && app.player.is_some();
        app
    }

    fn enqueue_media(&mut self, paths: Vec<PathBuf>, play_new: bool, ctx: egui::Context) {
        if !self.gst_ready {
            return;
        }
        let files: Vec<_> = paths.into_iter().filter(|path| path.is_file()).collect();
        if files.is_empty() {
            self.error = Some("No local files were added to the queue".to_owned());
            return;
        }
        let requested = files.len();
        let Some(first) = self.queue.enqueue(files) else {
            self.error = Some("The session queue is full (1,000 files)".to_owned());
            return;
        };
        let added = self.queue.items().len() - first;
        self.show_queue = true;
        self.show_browser = false;
        self.error =
            (added < requested).then(|| "The session queue is full (1,000 files)".to_owned());
        if play_new || self.player.is_none() {
            self.open_index(first, ctx);
        } else {
            self.status = format!("Added {added} file(s) to the session queue");
        }
    }

    fn open_index(&mut self, index: usize, ctx: egui::Context) {
        let Some(path) = self.queue.path(index).map(PathBuf::from) else {
            return;
        };
        match Player::open_with_settings(&path, None, ctx, self.volume, self.muted, self.speed) {
            Ok(player) => {
                self.queue.select(index);
                self.player = Some(player);
                self.texture = None;
                self.seek_preview = None;
                self.pending_subtitle_load = None;
                self.error = None;
                self.status = format!("Playing {}", file_label(&path));
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn load_external_subtitle(
        &mut self,
        subtitle_path: PathBuf,
        ctx: egui::Context,
    ) -> Result<(), String> {
        let current = self
            .player
            .as_ref()
            .ok_or("Open a video before loading subtitles")?;
        let media_path = current.path.clone();
        // A successful set_state only requests an asynchronous GStreamer load.
        // Keep the playable pipeline alive until its replacement has a text stream.
        self.pending_subtitle_load = None;
        let candidate = Player::open_with_settings(
            &media_path,
            Some(&subtitle_path),
            ctx,
            0.0,
            self.muted,
            self.speed,
        )?;
        self.pending_subtitle_load = Some(PendingSubtitleLoad {
            player: candidate,
            path: subtitle_path.clone(),
            started: Instant::now(),
        });
        self.error = None;
        self.status = format!("Loading subtitle file {}", file_label(&subtitle_path));
        Ok(())
    }

    fn apply_subtitle_file_choice(&mut self, choice: Option<PathBuf>, ctx: egui::Context) {
        if let Some(path) = choice {
            if let Err(error) = self.load_external_subtitle(path, ctx) {
                self.error = Some(error);
            }
        }
    }

    fn choose_subtitle(&mut self, choice: Option<&str>) {
        if let Some(player) = &mut self.player {
            match player.select_subtitle(choice) {
                Ok(()) => {
                    // A later explicit choice wins over an earlier file request.
                    self.pending_subtitle_load = None;
                    self.error = None;
                    self.status = if choice.is_some() {
                        "Switching subtitle track".to_owned()
                    } else {
                        "Turning subtitles off".to_owned()
                    };
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn refresh_pending_subtitle(&mut self, ctx: &egui::Context) {
        let Some(mut pending) = self.pending_subtitle_load.take() else {
            return;
        };
        let failure = match pending.player.poll_event() {
            Some(Err(error)) => Some(format!("Subtitle loading failed: {error}")),
            Some(Ok(())) => Some("Subtitle file ended before loading completed".to_owned()),
            None if pending.started.elapsed() > Duration::from_secs(5) => {
                Some("Subtitle loading timed out or no text track was found".to_owned())
            }
            None => None,
        };
        if let Some(error) = failure {
            self.error = Some(error);
            self.status = "Kept the current playback".to_owned();
            return;
        }
        if !pending.player.prerolled()
            || !pending.player.subtitle_selection_ready()
            || pending.player.subtitle_tracks().is_empty()
            || pending.player.duration().is_none()
        {
            self.pending_subtitle_load = Some(pending);
            ctx.request_repaint_after(Duration::from_millis(40));
            return;
        }
        let Some(current) = self.player.as_ref() else {
            return;
        };
        if current.path != pending.player.path {
            return;
        }
        let position = current.position().unwrap_or(0.0);
        let was_playing = current.playing;
        if position > 0.1 {
            if let Err(error) = pending.player.seek(position) {
                self.error = Some(format!(
                    "Subtitle loaded, but position could not be kept: {error}"
                ));
                return;
            }
        }
        if pending.player.playing != was_playing {
            if let Err(error) = pending.player.toggle() {
                self.error = Some(format!(
                    "Subtitle loaded, but playback state could not be kept: {error}"
                ));
                return;
            }
        }
        pending.player.set_muted(self.muted);
        pending.player.set_volume(self.volume);
        self.speed = pending.player.rate;
        let rate_warning = pending.player.take_rate_warning();
        self.player = Some(pending.player);
        self.texture = None;
        self.seek_preview = None;
        self.error = rate_warning;
        self.status = format!("Loaded subtitle file {}", file_label(&pending.path));
    }

    fn next_track(&mut self, ctx: egui::Context) {
        if let Some(index) = self.queue.next_index() {
            self.open_index(index, ctx);
        }
    }

    fn previous_track(&mut self, ctx: egui::Context) {
        if self
            .player
            .as_ref()
            .and_then(Player::position)
            .unwrap_or(0.0)
            > 3.0
        {
            if let Some(player) = &self.player {
                if let Err(err) = player.seek(0.0) {
                    self.error = Some(err);
                }
            }
        } else if let Some(index) = self.queue.previous_index() {
            self.open_index(index, ctx);
        } else if let Some(player) = &self.player {
            if let Err(err) = player.seek(0.0) {
                self.error = Some(err);
            }
        }
    }

    fn cycle_speed(&mut self) {
        let next = next_speed(self.speed);
        let Some(player) = &mut self.player else {
            return;
        };
        if let Err(error) = player.set_rate(next) {
            self.error = Some(error);
            return;
        }
        self.speed = next;
        self.error = None;
        let pending_error = self
            .pending_subtitle_load
            .as_mut()
            .and_then(|pending| pending.player.set_rate(next).err());
        if let Some(error) = pending_error {
            self.pending_subtitle_load = None;
            self.error = Some(format!(
                "Subtitle loading was canceled after the speed change: {error}"
            ));
        }
    }

    fn toggle_mute(&mut self) {
        if let Some(player) = &mut self.player {
            self.muted = !self.muted;
            player.set_muted(self.muted);
            if let Some(pending) = &mut self.pending_subtitle_load {
                pending.player.set_muted(self.muted);
            }
        }
    }

    fn toggle(&mut self) {
        if let Some(player) = &mut self.player {
            match player.toggle() {
                Ok(()) => {
                    self.error = None;
                    self.status = if player.playing { "Playing" } else { "Paused" }.to_owned();
                }
                Err(err) => self.error = Some(err),
            }
        }
    }

    fn skip(&mut self, delta: f64) {
        if let Some(player) = &self.player {
            // A key press can arrive while playbin is still discovering the file.
            // The seek bar is unavailable then; keyboard seek should behave the same way.
            let (Some(current), Some(end)) = (player.position(), player.duration()) else {
                return;
            };
            if end <= 0.0 {
                return;
            }
            let target = (current + delta).clamp(0.0, end);
            if let Err(err) = player.seek(target) {
                self.error = Some(err);
            }
        }
    }

    fn refresh_player(&mut self, ctx: &egui::Context) {
        self.refresh_pending_subtitle(ctx);
        let mut failed = false;
        let mut finished = false;
        if let Some(player) = &mut self.player {
            if let Some(frame) = player.take_frame() {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width, frame.height],
                    &frame.rgba,
                );
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture(
                        "mediastrike-video",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            if let Some(event) = player.poll_event() {
                match event {
                    Ok(()) => {
                        self.status = "Finished".to_owned();
                        finished = true;
                    }
                    Err(err) => {
                        self.status = "Playback stopped".to_owned();
                        self.error = Some(err);
                        failed = true;
                    }
                }
            }
            if let Some(warning) = player.take_rate_warning() {
                self.speed = player.rate;
                self.error = Some(warning);
            }
            if player.playing {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if failed {
            self.player = None;
            self.texture = None;
            self.seek_preview = None;
            self.pending_subtitle_load = None;
        } else if finished {
            if let Some(index) = self.queue.finished_index() {
                if self.queue.current() == Some(index) {
                    // Reuse the same pipeline for Repeat one (and one-item
                    // Repeat all) so external files, text Off, and a chosen
                    // embedded stream stay in effect.
                    if let Some(player) = &mut self.player {
                        match player.seek(0.0).and_then(|_| player.toggle()) {
                            Ok(()) => {
                                self.status = format!("Replaying {}", file_label(&player.path))
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                } else {
                    self.open_index(index, ctx.clone());
                }
            }
        }
    }

    fn write_capture_png(
        path: &std::path::Path,
        screenshot: &egui::ColorImage,
    ) -> Result<(), String> {
        let [width, height] = screenshot.size;
        let bytes = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Screenshot dimensions overflow")?;
        let mut pixels = Vec::with_capacity(bytes);
        for color in &screenshot.pixels {
            pixels.extend_from_slice(&color.to_array());
        }
        if pixels.len() != bytes {
            return Err("Screenshot pixel count does not match dimensions".into());
        }
        let width = u32::try_from(width).map_err(|_| "Screenshot width exceeds PNG limits")?;
        let height = u32::try_from(height).map_err(|_| "Screenshot height exceeds PNG limits")?;
        let image = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or("Could not construct screenshot image")?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("Create screenshot {}: {error}", path.display()))?;
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut file, image::ImageFormat::Png)
            .map_err(|error| format!("Encode screenshot: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Sync screenshot: {error}"))?;
        Ok(())
    }

    fn finish_capture(&mut self, ctx: &egui::Context, result: Result<(), String>) {
        if let Some(capture) = self.capture.take() {
            *capture.outcome.lock().unwrap() = Some(result);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn capture_after_frame(&mut self, ctx: &egui::Context) {
        let Some(capture) = self.capture.as_ref() else {
            return;
        };
        if capture.started.elapsed() > Duration::from_secs(15) {
            self.finish_capture(
                ctx,
                Err("Timed out waiting for a decoded video frame".into()),
            );
            return;
        }
        if let Some(expected) = &capture.subtitle_path {
            if self.pending_subtitle_load.is_some() {
                ctx.request_repaint_after(Duration::from_millis(40));
                return;
            }
            let attached = self.player.as_ref().and_then(Player::external_subtitle);
            if expected.canonicalize().ok().as_deref() != attached {
                self.finish_capture(
                    ctx,
                    Err(self.error.clone().unwrap_or_else(|| {
                        format!("Subtitle file was not loaded: {}", expected.display())
                    })),
                );
                return;
            }
        }
        if capture.requested {
            let image = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let result = Self::write_capture_png(&capture.path, &image);
                self.finish_capture(ctx, result);
            } else {
                ctx.request_repaint();
            }
            return;
        }
        if self.texture.is_none() {
            ctx.request_repaint_after(Duration::from_millis(40));
            return;
        }
        let capture = self.capture.as_mut().unwrap();
        if capture.settle_frames < 2 {
            capture.settle_frames += 1;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            capture.requested = true;
        }
        ctx.request_repaint();
    }
}

impl MediaStrike {
    fn set_focus_view(&mut self, focused: bool, ctx: &egui::Context) {
        if self.focus_view != focused {
            // The old view's slider will not receive drag_stopped after its
            // panel disappears. Discard its uncommitted seek preview.
            self.seek_preview = None;
            self.focus_view = focused;
            ctx.request_repaint();
        }
    }

    fn update_ui(&mut self, ctx: &egui::Context) {
        self.refresh_player(ctx);
        if self.player.is_none() {
            self.set_focus_view(false, ctx);
        }
        #[cfg(test)]
        {
            self.ui_mute_rect = None;
            self.ui_speed_rect = None;
        }
        let dropped: Vec<_> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            self.enqueue_media(dropped, true, ctx.clone());
        }

        let mut pick_files = None;
        let mut pick_folder = false;

        if !self.focus_view {
            egui::TopBottomPanel::top("identity")
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 24, 24)))
                .show(ctx, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("MediaStrike").strong().size(17.0));
                        ui.separator();
                        if let Some(player) = &self.player {
                            ui.label(
                                egui::RichText::new(file_label(&player.path)).color(MUTED_TEXT),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Your media, in one place").color(MUTED_TEXT),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("About").clicked() {
                                self.show_about = true;
                            }
                            if ui
                                .add_enabled(self.player.is_some(), egui::Button::new("Focus view"))
                                .clicked()
                            {
                                self.set_focus_view(true, ctx);
                            }
                            if ui
                                .button(if self.show_queue {
                                    "Hide queue"
                                } else {
                                    "Show queue"
                                })
                                .clicked()
                            {
                                self.show_queue = !self.show_queue;
                                self.show_browser = false;
                            }
                            if ui
                                .add_enabled(self.gst_ready, egui::Button::new("Open files"))
                                .clicked()
                            {
                                pick_files = Some(true);
                            }
                        });
                    });
                    ui.add_space(6.0);
                });
        }

        let mut previous_clicked = false;
        let mut next_clicked = false;
        let mut mute_clicked = false;
        let mut speed_clicked = false;
        let mut subtitle_choice: Option<Option<String>> = None;
        let mut choose_external_subtitle = false;
        if self.focus_view {
            egui::TopBottomPanel::bottom("focus_transport")
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 24, 24)))
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if let Some(player) = &mut self.player {
                            previous_clicked = ui.button("Prev").clicked();
                            if ui
                                .add(
                                    egui::Button::new(if player.playing {
                                        "Pause"
                                    } else {
                                        "Play"
                                    })
                                    .fill(egui::Color32::from_rgb(176, 79, 8)),
                                )
                                .clicked()
                            {
                                if let Err(error) = player.toggle() {
                                    self.error = Some(error);
                                } else {
                                    self.status = if player.playing { "Playing" } else { "Paused" }
                                        .to_owned();
                                }
                            }
                            next_clicked = ui.button("Next").clicked();
                            let mute_button = ui.button(if self.muted { "Unmute" } else { "Mute" });
                            let speed_button = ui.button(format!("Speed {:.1}×", self.speed));
                            #[cfg(test)]
                            {
                                self.ui_mute_rect = Some(mute_button.rect);
                                self.ui_speed_rect = Some(speed_button.rect);
                            }
                            mute_clicked = mute_button.clicked();
                            speed_clicked = speed_button.clicked();
                            let position = player.position().unwrap_or(0.0);
                            let duration = player.duration().filter(|time| *time > 0.0);
                            if let Some(duration) = duration {
                                let mut value =
                                    self.seek_preview.unwrap_or(position).clamp(0.0, duration);
                                let width = (ui.available_width() - 160.0).max(100.0);
                                let response = ui
                                    .scope(|ui| {
                                        ui.spacing_mut().slider_width = width;
                                        ui.add(
                                            egui::Slider::new(&mut value, 0.0..=duration)
                                                .show_value(false),
                                        )
                                    })
                                    .inner;
                                if response.dragged() {
                                    self.seek_preview = Some(value);
                                }
                                if response.drag_stopped()
                                    || (response.changed() && !response.dragged())
                                {
                                    if let Err(error) = player.seek(value) {
                                        self.error = Some(error);
                                    }
                                    self.seek_preview = None;
                                }
                            } else {
                                ui.label("Seek unavailable");
                            }
                            ui.label(format!(
                                "{} / {}",
                                format_time(position),
                                duration
                                    .map(format_time)
                                    .unwrap_or_else(|| "--:--".to_owned())
                            ));
                        }
                        if ui.button("Exit focus").clicked() {
                            self.set_focus_view(false, ctx);
                        }
                    });
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::from_rgb(255, 153, 127), error);
                    }
                    ui.add_space(4.0);
                });
        } else {
            egui::TopBottomPanel::bottom("transport")
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 24, 24)))
                .show(ctx, |ui| {
                    ui.add_space(6.0);
                    if let Some(player) = &mut self.player {
                        let position = player.position().unwrap_or(0.0);
                        let duration = player.duration().filter(|t| *t > 0.0);
                        if let Some(duration) = duration {
                            let mut value =
                                self.seek_preview.unwrap_or(position).clamp(0.0, duration);
                            let response = ui.add_sized(
                                [ui.available_width(), 18.0],
                                egui::Slider::new(&mut value, 0.0..=duration)
                                    .show_value(false)
                                    .text("Seek"),
                            );
                            if response.dragged() {
                                self.seek_preview = Some(value);
                            }
                            if response.drag_stopped()
                                || (response.changed() && !response.dragged())
                            {
                                if let Err(err) = player.seek(value) {
                                    self.error = Some(err);
                                }
                                self.seek_preview = None;
                            }
                        } else {
                            ui.label("Position unavailable for this media");
                        }
                        ui.horizontal(|ui| {
                            previous_clicked = ui.button("Prev").clicked();
                            if ui
                                .add(
                                    egui::Button::new(if player.playing {
                                        "Pause"
                                    } else {
                                        "Play"
                                    })
                                    .fill(egui::Color32::from_rgb(176, 79, 8)),
                                )
                                .clicked()
                            {
                                if let Err(err) = player.toggle() {
                                    self.error = Some(err);
                                } else {
                                    self.status = if player.playing { "Playing" } else { "Paused" }
                                        .to_owned();
                                }
                            }
                            next_clicked = ui.button("Next").clicked();
                            ui.label(format!(
                                "{} / {}",
                                format_time(position),
                                duration
                                    .map(format_time)
                                    .unwrap_or_else(|| "--:--".to_owned())
                            ));
                            ui.separator();
                            ui.label("Volume");
                            let mut volume = self.volume;
                            if ui
                                .add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false))
                                .changed()
                            {
                                self.volume = volume;
                                player.set_volume(volume);
                                self.muted = false;
                                player.set_muted(false);
                                if let Some(pending) = &mut self.pending_subtitle_load {
                                    pending.player.set_muted(false);
                                }
                            }
                            ui.label(format!("{}%", (self.volume * 100.0).round() as i32));
                            let mute_button = ui.button(if self.muted { "Unmute" } else { "Mute" });
                            let speed_button = ui.button(format!("Speed {:.1}×", self.speed));
                            #[cfg(test)]
                            {
                                self.ui_mute_rect = Some(mute_button.rect);
                                self.ui_speed_rect = Some(speed_button.rect);
                            }
                            mute_clicked = mute_button.clicked();
                            speed_clicked = speed_button.clicked();
                            ui.separator();
                            let subtitle_tracks = player.subtitle_tracks().to_vec();
                            let active_subtitle = player.selected_subtitle().map(str::to_owned);
                            let selection_ready = player.subtitle_selection_ready();
                            let external_file = player.external_subtitle().map(file_label);
                            ui.menu_button(
                                if external_file.is_some() {
                                    "Subtitles (file)"
                                } else {
                                    "Subtitles"
                                },
                                |ui| {
                                    if subtitle_tracks.is_empty() {
                                        ui.label("No subtitle tracks discovered");
                                    }
                                    ui.add_enabled_ui(selection_ready, |ui| {
                                        if ui
                                            .selectable_label(active_subtitle.is_none(), "Off")
                                            .clicked()
                                        {
                                            subtitle_choice = Some(None);
                                            ui.close_menu();
                                        }
                                        for track in &subtitle_tracks {
                                            if ui
                                                .selectable_label(
                                                    active_subtitle.as_deref()
                                                        == Some(track.id.as_str()),
                                                    &track.label,
                                                )
                                                .clicked()
                                            {
                                                subtitle_choice = Some(Some(track.id.clone()));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                    ui.separator();
                                    if let Some(file) = &external_file {
                                        ui.label(format!("External: {file}"));
                                    }
                                    if ui.button("Load subtitle file...").clicked() {
                                        choose_external_subtitle = true;
                                        ui.close_menu();
                                    }
                                },
                            );
                            ui.separator();
                            if ui.button(self.queue.repeat().label()).clicked() {
                                self.queue.cycle_repeat();
                            }
                        });
                    } else {
                        ui.label("Open or drop local media to start playback");
                    }
                    ui.label(egui::RichText::new(&self.status).color(MUTED_TEXT));
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::from_rgb(255, 153, 127), error);
                    }
                    ui.add_space(6.0);
                });
        }

        if mute_clicked {
            self.toggle_mute();
        }
        if speed_clicked {
            self.cycle_speed();
        }
        if previous_clicked {
            self.previous_track(ctx.clone());
        }
        if next_clicked {
            self.next_track(ctx.clone());
        }
        if let Some(choice) = subtitle_choice {
            self.choose_subtitle(choice.as_deref());
        }

        if !self.focus_view {
            egui::SidePanel::left("navigation")
                .resizable(false)
                .default_width(158.0)
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(27, 27, 27)))
                .show(ctx, |ui| {
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("YOUR MEDIA").color(MUTED_TEXT).small());
                    ui.add_space(12.0);
                    if ui
                        .selectable_label(!self.show_queue && !self.show_browser, "Now playing")
                        .clicked()
                    {
                        self.show_queue = false;
                        self.show_browser = false;
                    }
                    if ui
                        .selectable_label(self.show_browser, "Browse folder")
                        .clicked()
                    {
                        self.show_queue = false;
                        self.show_browser = true;
                        pick_folder = self.browser_folder.is_none();
                    }
                    if ui
                        .selectable_label(self.show_queue, "Session queue")
                        .clicked()
                    {
                        self.show_queue = true;
                        self.show_browser = false;
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("{} local file(s)", self.queue.items().len()))
                            .color(MUTED_TEXT),
                    );
                });
        }

        let mut browse_play = None;
        if self.show_browser && !self.focus_view {
            egui::SidePanel::left("browser")
                .resizable(true)
                .default_width(290.0)
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(22, 22, 22)))
                .show(ctx, |ui| {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.heading("Browse local media");
                        if ui.button("Choose folder…").clicked() { pick_folder = true; }
                    });
                    if let Some(folder) = &self.browser_folder {
                        ui.label(egui::RichText::new(folder.display().to_string()).color(MUTED_TEXT).small());
                    } else {
                        ui.label(egui::RichText::new("Choose a folder to see its video and audio files.").color(MUTED_TEXT));
                    }
                    ui.add_space(8.0);
                    let search = ui.add(egui::TextEdit::singleline(&mut self.browser_search).hint_text("Search this folder"));
                    if (search.has_focus() || search.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        self.browser_search.clear();
                    }
                    let filter = self.browser_search.trim().to_lowercase();
                    let mut matches: Vec<_> = self.browser_items.iter().enumerate()
                        .filter(|(_, path)| filter.is_empty() || file_label(path).to_lowercase().contains(&filter))
                        .map(|(index, path)| (index, path.clone())).collect();
                    if !self.browser_ascending { matches.reverse(); }
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{} of {} files", matches.len(), self.browser_items.len())).color(MUTED_TEXT));
                        if ui.button(if self.browser_ascending { "Name A–Z" } else { "Name Z–A" }).clicked() {
                            self.browser_ascending = !self.browser_ascending;
                        }
                    });
                    ui.separator();
                    if self.browser_folder.is_some() && self.browser_items.is_empty() {
                        ui.label("No supported video or audio files in this folder.");
                    } else if !self.browser_items.is_empty() && matches.is_empty() {
                        ui.label("No files match this search.");
                        if ui.button("Clear search").clicked() { self.browser_search.clear(); search.request_focus(); }
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let playing_identity = self.player.as_ref().and_then(|player| fs::canonicalize(&player.path).ok());
                            for (index, path) in matches {
                                let selected = self.player.as_ref().is_some_and(|player| {
                                    media_paths_match(&player.path, playing_identity.as_deref(), &path, self.browser_identities[index].as_deref())
                                });
                                let row = ui.selectable_label(selected, file_label(&path));
                                if row.clicked() || (row.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) {
                                    browse_play = Some(path);
                                }
                            }
                        });
                    }
                });
        }
        if let Some(path) = browse_play {
            if let Some(index) = matching_queue_index(self.queue.items(), &path) {
                self.open_index(index, ctx.clone());
            } else {
                self.enqueue_media(vec![path], true, ctx.clone());
            }
            self.show_browser = true;
            self.show_queue = false;
        }

        let mut select_index = None;
        if self.show_queue && !self.focus_view {
            egui::SidePanel::right("queue")
                .resizable(true)
                .default_width(265.0)
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(22, 22, 22)))
                .show(ctx, |ui| {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.heading("Up next");
                        if ui
                            .add_enabled(self.gst_ready, egui::Button::new("Add files"))
                            .clicked()
                        {
                            pick_files = Some(false);
                        }
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "{} items in this session",
                            self.queue.items().len()
                        ))
                        .color(MUTED_TEXT),
                    );
                    ui.add_space(8.0);
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.queue_search)
                            .hint_text("Filter queue"),
                    );
                    #[cfg(test)]
                    {
                        self.queue_filter_rect = Some(search.rect);
                    }
                    if (search.has_focus() || search.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        self.queue_search.clear();
                    }
                    ui.separator();
                    let filter = self.queue_search.trim().to_lowercase();
                    let matching = self
                        .queue
                        .items()
                        .iter()
                        .filter(|path| filter.is_empty() || file_label(path).to_lowercase().contains(&filter))
                        .count();
                    if !filter.is_empty() {
                        ui.label(
                            egui::RichText::new(format!(
                                "{matching} of {} files",
                                self.queue.items().len()
                            ))
                            .color(MUTED_TEXT),
                        );
                    }
                    if self.queue.items().is_empty() {
                        ui.add_space(16.0);
                        ui.label("The session queue is empty.");
                        ui.label(egui::RichText::new("Use Add files to choose local media.").color(MUTED_TEXT));
                    } else if matching == 0 {
                        ui.add_space(16.0);
                        ui.label("No files match this filter.");
                        if ui.button("Clear filter").clicked() {
                            self.queue_search.clear();
                            search.request_focus();
                        }
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (index, path) in self.queue.items().iter().enumerate() {
                                let label = file_label(path);
                                if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                let row = ui.selectable_label(
                                    self.queue.current() == Some(index),
                                    format!("{:02}  {label}", index + 1),
                                );
                                if row.clicked() || (row.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) {
                                    select_index = Some(index);
                                }
                            }
                        });
                    }
                });
        }
        if let Some(index) = select_index {
            self.open_index(index, ctx.clone());
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(8, 8, 9)))
            .show(ctx, |ui| {
                if let Some(texture) = &self.texture {
                    let [width, height] = texture.size();
                    let area = ui.available_size();
                    let scale = (area.x / width as f32).min(area.y / height as f32);
                    let size = egui::vec2(width as f32 * scale, height as f32 * scale);
                    ui.vertical_centered(|ui| {
                        ui.add_space((area.y - size.y).max(0.0) / 2.0);
                        ui.image((texture.id(), size));
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(((ui.available_height() - 220.0) / 2.0).max(0.0));
                        if let Some(texture) = &self.brand_texture {
                            ui.image((texture.id(), egui::vec2(112.0, 112.0)));
                        }
                        if let Some(player) = &self.player {
                            ui.heading("Now playing");
                            ui.label(file_label(&player.path));
                        } else {
                            ui.heading("MediaStrike");
                            ui.label(egui::RichText::new("by PWDS").color(MUTED_TEXT));
                            ui.add_space(12.0);
                            ui.label("Drop local video or audio files here");
                            if ui
                                .add_enabled(self.gst_ready, egui::Button::new("Open files"))
                                .clicked()
                            {
                                pick_files = Some(true);
                            }
                        }
                    });
                }
            });

        if self.show_about {
            egui::Window::new("About MediaStrike")
                .open(&mut self.show_about)
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        if let Some(texture) = &self.brand_texture {
                            ui.image((texture.id(), egui::vec2(96.0, 96.0)));
                        }
                        ui.heading("MediaStrike");
                        ui.label("A local media player by PWDS");
                        ui.label(egui::RichText::new("Version 0.1.0").color(MUTED_TEXT));
                    });
                });
        }

        let shortcuts = player_shortcuts(ctx);
        if shortcuts.focus_view && self.player.is_some() {
            self.set_focus_view(!self.focus_view, ctx);
        }
        if shortcuts.open_files && pick_files.is_none() {
            pick_files = Some(true);
        }
        if self.player.is_some() {
            if shortcuts.toggle {
                self.toggle();
            }
            if shortcuts.mute {
                self.toggle_mute();
            }
            if shortcuts.back {
                self.skip(-5.0);
            }
            if shortcuts.forward {
                self.skip(5.0);
            }
            if shortcuts.previous {
                self.previous_track(ctx.clone());
            }
            if shortcuts.next {
                self.next_track(ctx.clone());
            }
            if shortcuts.toggle
                || shortcuts.mute
                || shortcuts.back
                || shortcuts.forward
                || shortcuts.previous
                || shortcuts.next
            {
                ctx.request_repaint();
            }
        }

        if let Some(play_new) = pick_files {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                self.enqueue_media(paths, play_new, ctx.clone());
            }
        }
        if pick_folder {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                match scan_media_folder(&folder) {
                    Ok(items) => {
                        self.browser_identities = items.iter().map(|path| fs::canonicalize(path).ok()).collect();
                        self.browser_items = items;
                        self.browser_folder = Some(folder);
                        self.browser_search.clear();
                        self.error = None;
                        self.show_browser = true;
                        self.show_queue = false;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if choose_external_subtitle {
            let choice = rfd::FileDialog::new()
                .add_filter("Subtitle files", &["srt", "vtt"])
                .pick_file();
            self.apply_subtitle_file_choice(choice, ctx.clone());
        }
        self.capture_after_frame(ctx);
    }
}

impl eframe::App for MediaStrike {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_ui(ctx);
    }
}

fn media_paths_match(left: &Path, left_identity: Option<&Path>, right: &Path, right_identity: Option<&Path>) -> bool {
    left == right || left_identity.zip(right_identity).is_some_and(|(a, b)| a == b)
}

fn matching_queue_index(items: &[PathBuf], path: &Path) -> Option<usize> {
    let identity = fs::canonicalize(path).ok();
    items.iter().position(|item| {
        media_paths_match(item, fs::canonicalize(item).ok().as_deref(), path, identity.as_deref())
    })
}

fn scan_media_folder(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(folder).map_err(|error| format!("Cannot browse folder: {error}"))?;
    let mut files = Vec::new();
    for (seen, entry) in entries.enumerate() {
        if seen >= 10_000 { return Err("Folder has too many entries to browse safely".to_owned()); }
        let entry = entry.map_err(|error| format!("Cannot read folder entry: {error}"))?;
        if !entry.file_type().map_err(|error| format!("Cannot inspect folder entry: {error}"))?.is_file() {
            continue;
        }
        let path = entry.path();
        if !is_media_path(&path) { continue; }
        files.push(path);
        if files.len() > queue::MAX_QUEUE_ITEMS {
            return Err("Folder has more than 1,000 supported media files".to_owned());
        }
    }
    files.sort_by_key(|path| file_label(path).to_lowercase());
    Ok(files)
}

fn is_media_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp4" | "m4v" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "mpg" | "mpeg"
                | "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "opus" | "aiff"
        )
    })
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn load_brand_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let rgba = image::load_from_memory(include_bytes!(
        "../assets/branding/playstrike-lyrebird-icon.png"
    ))
    .ok()?
    .into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture("mediastrike-brand", image, egui::TextureOptions::LINEAR))
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{}:{:02}", minutes, seconds % 60)
    }
}

struct LaunchArgs {
    media_paths: Vec<PathBuf>,
    subtitle_path: Option<PathBuf>,
    focus_view: bool,
    capture_path: Option<PathBuf>,
}

fn parse_launch_args(args: impl IntoIterator<Item = OsString>) -> Result<LaunchArgs, String> {
    let mut args = args.into_iter().peekable();
    let mut media_paths = Vec::new();
    let mut subtitle_path = None;
    let mut focus_view = false;
    let mut capture_path = None;
    while let Some(arg) = args.next() {
        if arg == "--subtitle" {
            let Some(path) = args.next() else {
                return Err("--subtitle requires a local .srt or .vtt file".to_owned());
            };
            if subtitle_path.is_some()
                || path == "--capture"
                || path == "--subtitle"
                || path == "--focus-view"
            {
                return Err("Pass exactly one subtitle file to --subtitle".to_owned());
            }
            subtitle_path = Some(PathBuf::from(path));
        } else if arg == "--focus-view" {
            if focus_view {
                return Err("Pass --focus-view only once".to_owned());
            }
            focus_view = true;
        } else if arg == "--capture" {
            let Some(output) = args.next() else {
                return Err("--capture requires a new PNG path".to_owned());
            };
            if media_paths.is_empty() || args.peek().is_some() {
                return Err("--capture must follow one or more media files".to_owned());
            }
            let path = PathBuf::from(output);
            if path.extension().is_none_or(|ext| ext != "png") {
                return Err("Capture path must end in .png".to_owned());
            }
            capture_path = Some(path);
        } else {
            media_paths.push(PathBuf::from(arg));
            if media_paths.len() > queue::MAX_QUEUE_ITEMS {
                return Err("Too many files for the session queue".to_owned());
            }
        }
    }
    if subtitle_path.is_some() && media_paths.is_empty() {
        return Err("--subtitle requires at least one media file".to_owned());
    }
    if focus_view && media_paths.is_empty() {
        return Err("--focus-view requires at least one media file".to_owned());
    }
    Ok(LaunchArgs {
        media_paths,
        subtitle_path,
        focus_view,
        capture_path,
    })
}

fn main() -> ExitCode {
    let LaunchArgs {
        media_paths,
        subtitle_path,
        focus_view,
        capture_path,
    } = match parse_launch_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!(
                "{error}\nMediaStrike usage: playstrike [MEDIA_FILE ...] [--subtitle FILE.srt] [--focus-view] [--capture FILE.png]"
            );
            return ExitCode::FAILURE;
        }
    };
    let capture_result = capture_path.as_ref().map(|_| Arc::new(Mutex::new(None)));
    let capture = capture_path
        .zip(capture_result.clone())
        .map(|(path, outcome)| CaptureOnce::new(path, subtitle_path.clone(), outcome));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title("MediaStrike"),
        ..Default::default()
    };
    match eframe::run_native(
        "MediaStrike",
        options,
        Box::new(move |cc| {
            Ok(Box::new(MediaStrike::new(
                cc,
                media_paths,
                subtitle_path,
                focus_view,
                capture,
            )))
        }),
    ) {
        Ok(()) => match capture_result {
            None => ExitCode::SUCCESS,
            Some(result) => match result.lock().unwrap().take() {
                Some(Ok(())) => ExitCode::SUCCESS,
                Some(Err(error)) => {
                    eprintln!("MediaStrike capture: {error}");
                    ExitCode::FAILURE
                }
                None => {
                    eprintln!("MediaStrike capture: window closed before screenshot completed");
                    ExitCode::FAILURE
                }
            },
        },
        Err(error) => {
            eprintln!("MediaStrike window: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaStrike, PlayerShortcuts, format_time, parse_launch_args, player_shortcuts, scan_media_folder, is_media_path, matching_queue_index, media_paths_match};
    use eframe::egui;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn synthetic_app() -> MediaStrike {
        MediaStrike {
            player: None,
            queue: Default::default(),
            show_queue: true,
            queue_search: String::new(),
            show_browser: false,
            browser_folder: None,
            browser_items: Vec::new(),
            browser_identities: Vec::new(),
            browser_search: String::new(),
            browser_ascending: true,
            focus_view: false,
            texture: None,
            brand_texture: None,
            show_about: false,
            seek_preview: None,
            volume: 0.8,
            muted: false,
            speed: 1.0,
            status: String::new(),
            error: None,
            gst_ready: true,
            capture: None,
            pending_subtitle_load: None,
            queue_filter_rect: None,
            ui_mute_rect: None,
            ui_speed_rect: None,
        }
    }

    #[test]
    fn folder_browser_lists_only_regular_supported_media_in_name_order() {
        let folder = std::env::temp_dir().join(format!(
            "mediastrike-browser-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("b.MP4"), b"synthetic").unwrap();
        std::fs::write(folder.join("a.flac"), b"synthetic").unwrap();
        std::fs::write(folder.join("notes.txt"), b"synthetic").unwrap();
        std::fs::create_dir(folder.join("nested.mkv")).unwrap();
        let files = scan_media_folder(&folder).unwrap();
        assert_eq!(files.iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), vec!["a.flac", "b.MP4"]);
        assert!(is_media_path(Path::new("voice.OPUS")));
        assert!(!is_media_path(Path::new("script.exe")));
    }

    #[test]
    fn browser_matches_relative_queue_path_by_file_identity() {
        let folder = PathBuf::from(format!("mediastrike-path-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        let relative = folder.join("clip.mp4");
        std::fs::write(&relative, b"synthetic").unwrap();
        let absolute = std::fs::canonicalize(&relative).unwrap();
        assert_ne!(relative, absolute);
        assert!(media_paths_match(&relative, Some(&absolute), &absolute, Some(&absolute)));
        assert_eq!(matching_queue_index(&[relative.clone(), relative.clone()], &absolute), Some(0));
        assert_eq!(matching_queue_index(&[PathBuf::from("missing.mp4")], &absolute), None);
        std::fs::remove_file(&relative).unwrap();
        std::fs::remove_dir(&folder).unwrap();
    }

    fn wait_for_position_at_least(app: &mut MediaStrike, ctx: &egui::Context, minimum: f64) -> f64 {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(position) = app.player.as_ref().and_then(|player| player.position()) {
                if position >= minimum {
                    return position;
                }
            }
            app.refresh_player(ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("position did not reach {minimum} seconds");
    }

    fn two_track_app(ctx: &egui::Context) -> (MediaStrike, String) {
        let embedded = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_EMBEDDED_SUBTITLE_VIDEO").expect("two-track MKV"),
        );
        let mut app = synthetic_app();
        app.enqueue_media(vec![embedded], true, ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && (!app.player.as_ref().unwrap().subtitle_selection_ready()
                || app.player.as_ref().unwrap().subtitle_tracks().len() < 2)
        {
            app.refresh_player(ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let tracks = app.player.as_ref().unwrap().subtitle_tracks();
        assert_eq!(tracks.len(), 2, "fixture must expose both embedded tracks");
        let second = tracks[1].id.clone();
        (app, second)
    }

    fn wait_for_pending_subtitle(app: &mut MediaStrike, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            app.pending_subtitle_load.is_none(),
            "subtitle load did not settle"
        );
        assert!(
            app.error.is_none(),
            "subtitle choice failed: {:?}",
            app.error
        );
    }

    fn shortcut_key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn filter_frame(
        ctx: &egui::Context,
        filter: &mut String,
        events: Vec<egui::Event>,
    ) -> (egui::Rect, bool, PlayerShortcuts) {
        let modifiers = events
            .iter()
            .rev()
            .find_map(|event| match event {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            events,
            focused: true,
            modifiers,
            ..Default::default()
        };
        let mut rect = egui::Rect::NOTHING;
        let mut focused = false;
        let mut shortcuts = PlayerShortcuts::default();
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.add(egui::TextEdit::singleline(filter).hint_text("Filter queue"));
                rect = response.rect;
                focused = response.has_focus();
            });
            shortcuts = player_shortcuts(ctx);
        });
        (rect, focused, shortcuts)
    }

    fn player_ui_frame(app: &mut MediaStrike, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1120.0, 720.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.update_ui(ctx));
    }

    fn paused_video_app(ctx: &egui::Context) -> MediaStrike {
        gstreamer::init().expect("GStreamer runtime");
        let video = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("AVI"));
        let mut app = synthetic_app();
        app.enqueue_media(vec![video], true, ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && app
                .player
                .as_ref()
                .and_then(|player| player.duration())
                .is_none()
        {
            app.refresh_player(ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let player = app.player.as_mut().expect("opened synthetic media");
        assert!(
            player.duration().is_some(),
            "synthetic media needs a seek range"
        );
        if player.playing {
            player.toggle().expect("pause synthetic media");
        }
        app
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    #[ignore = "requires generated local audio/video via PLAYSTRIKE_MUTE_AUDIO_VIDEO"]
    fn mute_and_speed_buttons_accept_egui_pointer_events() {
        gstreamer::init().expect("GStreamer runtime");
        let media = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_MUTE_AUDIO_VIDEO").expect("audio/video fixture"),
        );
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![media.clone()], true, ctx.clone());
        let caps = gstreamer::Caps::builder("audio/x-raw")
            .field("format", "S16LE")
            .field("rate", 16000i32)
            .field("channels", 1i32)
            .build();
        let audio_sink = gstreamer_app::AppSink::builder()
            .caps(&caps)
            .sync(true)
            .max_buffers(2)
            .drop(true)
            .build();
        app.player = Some(
            super::Player::open_with_test_audio_sink(
                &media, None, ctx.clone(), 0.8, false, 1.0, &audio_sink,
            )
            .expect("open synthetic A/V for UI pointer test"),
        );
        app.error = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && app.player.as_ref().is_none_or(|player| {
                player.duration().is_none() || player.position().is_none()
            })
        {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.player.as_ref().unwrap().position().is_some());
        app.toggle();
        assert!(!app.player.as_ref().unwrap().playing);

        fn click(app: &mut MediaStrike, ctx: &egui::Context, rect: egui::Rect) {
            let position = rect.center();
            eprintln!("egui pointer target: rect={rect:?} center={position:?}");
            player_ui_frame(app, ctx, vec![egui::Event::PointerMoved(position)]);
            player_ui_frame(app, ctx, vec![pointer_button(position, true)]);
            player_ui_frame(app, ctx, vec![pointer_button(position, false)]);
        }

        fn visible_button(app: &mut MediaStrike, ctx: &egui::Context, speed: bool) -> egui::Rect {
            let mut last = None;
            for _ in 0..5 {
                player_ui_frame(app, ctx, vec![]);
                assert!(
                    app.player.is_some(),
                    "player lost before UI click: status={} error={:?}",
                    app.status,
                    app.error
                );
                let rect = if speed { app.ui_speed_rect } else { app.ui_mute_rect };
                if let Some(rect) = rect {
                    last = Some(rect);
                    if rect.min.x >= 0.0
                        && rect.min.y >= 0.0
                        && rect.max.x <= 1120.0
                        && rect.max.y <= 720.0
                    {
                        return rect;
                    }
                }
            }
            panic!("button never settled inside viewport: {last:?}");
        }

        let mute = visible_button(&mut app, &ctx, false);
        click(&mut app, &ctx, mute);
        assert!(app.muted && app.player.as_ref().unwrap().muted());
        eprintln!("normal Mute click: muted={} speed={}", app.muted, app.speed);

        for expected in [1.5, 2.0, 0.5, 1.0] {
            let speed = visible_button(&mut app, &ctx, true);
            click(&mut app, &ctx, speed);
            assert_eq!(app.speed, expected, "Speed click must cycle preset");
            wait_for_speed(&mut app, &ctx, expected);
            assert!(app.player.as_ref().unwrap().muted());
            eprintln!("normal Speed click: muted={} speed={}", app.muted, app.speed);
        }

        app.set_focus_view(true, &ctx);
        let unmute = visible_button(&mut app, &ctx, false);
        click(&mut app, &ctx, unmute);
        assert!(!app.muted && !app.player.as_ref().unwrap().muted());
        let speed = visible_button(&mut app, &ctx, true);
        click(&mut app, &ctx, speed);
        assert_eq!(app.speed, 1.5);
        wait_for_speed(&mut app, &ctx, 1.5);
        assert!(app.focus_view);
        assert!(!app.player.as_ref().unwrap().playing);
        eprintln!("focus UI clicks: muted={} speed={}", app.muted, app.speed);
    }

    #[test]
    #[ignore = "requires generated local video and GStreamer"]
    fn focus_shortcut_cancels_seek_drag_in_both_directions() {
        for focused in [false, true] {
            let ctx = egui::Context::default();
            let mut app = paused_video_app(&ctx);
            app.focus_view = focused;
            player_ui_frame(&mut app, &ctx, vec![]);
            // The slider sets this preview while dragged. Seed that exact app
            // state without depending on renderer-specific widget coordinates.
            app.seek_preview = Some(2.0);
            let position_before = app.player.as_ref().unwrap().position().unwrap_or(0.0);

            player_ui_frame(
                &mut app,
                &ctx,
                vec![shortcut_key(egui::Key::F11, egui::Modifiers::NONE)],
            );
            assert_eq!(app.focus_view, !focused, "F11 must change the view");
            assert_eq!(app.seek_preview, None, "old seek drag must be canceled");
            player_ui_frame(
                &mut app,
                &ctx,
                vec![pointer_button(egui::pos2(80.0, 663.0), false)],
            );
            assert_eq!(app.seek_preview, None, "release must not revive old seek");
            let position_after = app.player.as_ref().unwrap().position().unwrap_or(0.0);
            assert!(
                (position_after - position_before).abs() < 0.5,
                "canceling a seek must not move playback"
            );
            assert!(app.show_queue, "focus change must retain queue visibility");
        }
    }

    #[test]
    #[ignore = "requires generated local video and GStreamer"]
    fn focus_buttons_cancel_pending_seek_in_both_directions() {
        for focused in [false, true] {
            let ctx = egui::Context::default();
            let mut app = paused_video_app(&ctx);
            app.focus_view = focused;
            player_ui_frame(&mut app, &ctx, vec![]);
            app.seek_preview = Some(2.0);
            let button = if focused {
                egui::pos2(1065.0, 704.0)
            } else {
                egui::pos2(1033.0, 18.0)
            };
            player_ui_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(button),
                    pointer_button(button, true),
                    pointer_button(button, false),
                ],
            );
            assert_eq!(app.focus_view, !focused, "button must change the view");
            assert_eq!(app.seek_preview, None, "button must cancel old seek");
            assert!(app.show_queue, "focus change must retain queue visibility");
        }
    }

    #[test]
    fn focused_escape_clears_queue_filter_after_egui_releases_focus() {
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.queue_search = "not-found".to_owned();
        player_ui_frame(&mut app, &ctx, vec![]);

        let filter_position = app.queue_filter_rect.expect("queue filter rect").center();
        player_ui_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(filter_position),
                pointer_button(filter_position, true),
                pointer_button(filter_position, false),
            ],
        );
        assert!(ctx.wants_keyboard_input(), "queue filter should own keyboard focus");

        player_ui_frame(
            &mut app,
            &ctx,
            vec![shortcut_key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(app.queue_search.is_empty(), "Escape must clear the focused filter");
    }

    #[test]
    fn queue_filter_owns_shortcuts_on_first_click_and_while_typing() {
        let ctx = egui::Context::default();
        let mut filter = String::new();
        let (rect, focused, _) = filter_frame(&ctx, &mut filter, vec![]);
        assert!(!focused);

        let position = rect.center();
        let click = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let (_, focused, shortcuts) = filter_frame(
            &ctx,
            &mut filter,
            vec![
                egui::Event::PointerMoved(position),
                click(true),
                click(false),
                shortcut_key(egui::Key::Space, egui::Modifiers::NONE),
            ],
        );
        assert!(focused, "first click should focus the queue filter");
        assert_eq!(shortcuts, PlayerShortcuts::default());

        let (_, focused, shortcuts) = filter_frame(
            &ctx,
            &mut filter,
            vec![
                egui::Event::Text("sample".to_owned()),
                shortcut_key(egui::Key::ArrowLeft, egui::Modifiers::NONE),
                shortcut_key(
                    egui::Key::ArrowRight,
                    egui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                ),
            ],
        );
        assert!(focused, "typing should keep the queue filter focused");
        assert!(filter.contains("sample"));
        assert_eq!(shortcuts, PlayerShortcuts::default());

        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let command = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        for (key, modifiers, expected) in [
            (
                egui::Key::Space,
                egui::Modifiers::NONE,
                PlayerShortcuts {
                    toggle: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::ArrowLeft,
                egui::Modifiers::NONE,
                PlayerShortcuts {
                    back: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::ArrowRight,
                egui::Modifiers::NONE,
                PlayerShortcuts {
                    forward: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::ArrowLeft,
                alt,
                PlayerShortcuts {
                    previous: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::ArrowRight,
                alt,
                PlayerShortcuts {
                    next: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::O,
                command,
                PlayerShortcuts {
                    open_files: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::F11,
                egui::Modifiers::NONE,
                PlayerShortcuts {
                    focus_view: true,
                    ..Default::default()
                },
            ),
            (
                egui::Key::M,
                egui::Modifiers::NONE,
                PlayerShortcuts {
                    mute: true,
                    ..Default::default()
                },
            ),
        ] {
            let other = egui::Context::default();
            let (_, focused, shortcuts) = filter_frame(
                &other,
                &mut String::new(),
                vec![shortcut_key(key, modifiers)],
            );
            assert!(!focused, "no text field should own keyboard focus yet");
            assert_eq!(
                shortcuts, expected,
                "shortcut {key:?} should work outside text"
            );
        }
    }

    fn wait_for_speed(app: &mut MediaStrike, ctx: &egui::Context, expected: f64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            app.refresh_player(ctx);
            if app.player.as_ref().is_some_and(|player| {
                player.rate == expected
                    && player
                        .effective_rate()
                        .is_some_and(|rate| (rate - expected).abs() < 0.01)
            }) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "GStreamer did not apply {expected:.1}×; actual: {:?}, error: {:?}",
            app.player.as_ref().and_then(|player| player.effective_rate()),
            app.error
        );
    }

    #[test]
    fn speed_presets_cycle_within_supported_range() {
        assert_eq!(super::next_speed(1.0), 1.5);
        assert_eq!(super::next_speed(1.5), 2.0);
        assert_eq!(super::next_speed(2.0), 0.5);
        assert_eq!(super::next_speed(0.5), 1.0);
    }

    #[test]
    #[ignore = "requires generated local video and malformed SRT via PLAYSTRIKE_SUBTITLE_VIDEO and PLAYSTRIKE_SUBTITLE_MALFORMED"]
    fn rate_during_malformed_subtitle_recovery() {
        gstreamer::init().expect("GStreamer runtime");
        let media = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("video"));
        let malformed = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_SUBTITLE_MALFORMED").expect("malformed subtitle"),
        );
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![media.clone()], true, ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && app.player.as_ref().is_none_or(|player| {
                player.duration().is_none() || player.position().is_none()
            })
        {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.player.as_ref().unwrap().position().is_some());
        app.toggle();
        app.toggle_mute();
        app.cycle_speed();
        wait_for_speed(&mut app, &ctx, 1.5);

        app.load_external_subtitle(malformed, ctx.clone())
            .expect("start malformed subtitle candidate");
        assert!(app.pending_subtitle_load.is_some());
        app.cycle_speed();
        assert_eq!(app.speed, 2.0);
        assert!(app.pending_subtitle_load.is_some());
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.pending_subtitle_load.is_none(), "candidate did not fail");
        assert!(
            app.error
                .as_ref()
                .is_some_and(|error| error.starts_with("Subtitle loading failed:")),
            "expected failed subtitle recovery: {:?}",
            app.error
        );
        wait_for_speed(&mut app, &ctx, 2.0);
        let original = app.player.as_ref().expect("original player remains");
        assert_eq!(original.path, media);
        assert!(!original.playing);
        assert!(app.muted && original.muted());
        eprintln!("malformed subtitle recovery: speed={} muted={}", app.speed, app.muted);
    }

    #[test]
    #[ignore = "requires generated local videos and SRT via PLAYSTRIKE_SUBTITLE_VIDEO, PLAYSTRIKE_SAMPLE, and PLAYSTRIKE_SUBTITLE_SAMPLE"]
    fn playback_speed_survives_seek_queue_subtitle_and_focus() {
        gstreamer::init().expect("GStreamer runtime");
        let video = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("video"));
        let next = PathBuf::from(std::env::var_os("PLAYSTRIKE_SAMPLE").expect("next video"));
        let subtitle = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![video.clone(), next], true, ctx.clone());
        app.toggle();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && app
                .player
                .as_ref()
                .is_none_or(|player| player.duration().is_none() || player.position().is_none())
        {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        app.cycle_speed();
        assert_eq!(app.speed, 1.5);
        wait_for_speed(&mut app, &ctx, 1.5);
        app.player.as_ref().unwrap().seek(1.0).expect("seek at 1.5×");
        wait_for_speed(&mut app, &ctx, 1.5);
        assert!(app.player.as_mut().unwrap().set_rate(0.0).is_err());
        assert_eq!(app.player.as_ref().unwrap().rate, 1.5);

        app.toggle_mute();
        app.set_focus_view(true, &ctx);
        app.next_track(ctx.clone());
        app.toggle();
        wait_for_speed(&mut app, &ctx, 1.5);
        assert!(app.player.as_ref().unwrap().muted());
        assert!(app.focus_view && app.show_queue);
        app.previous_track(ctx.clone());
        app.toggle();
        wait_for_speed(&mut app, &ctx, 1.5);
        assert_eq!(app.player.as_ref().unwrap().path, video);
        assert!(app.player.as_ref().unwrap().muted());

        app.load_external_subtitle(subtitle, ctx.clone()).expect("load SRT");
        wait_for_pending_subtitle(&mut app, &ctx);
        wait_for_speed(&mut app, &ctx, 1.5);
        assert!(app.player.as_ref().unwrap().external_subtitle().is_some());
        assert!(!app.player.as_ref().unwrap().playing);
        assert!(app.player.as_ref().unwrap().muted());
        assert!(app.focus_view && app.show_queue);

        app.queue.cycle_repeat();
        app.queue.cycle_repeat();
        app.toggle();
        app.player.as_ref().unwrap().seek(5.5).expect("seek near end");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !app.status.starts_with("Replaying ") {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.status.starts_with("Replaying "), "repeat one did not replay");
        wait_for_speed(&mut app, &ctx, 1.5);
        assert!(app.player.as_ref().unwrap().muted());
        assert!(app.focus_view && app.show_queue);
    }

    #[test]
    #[ignore = "requires generated PCM Matroska and malformed SRT via PLAYSTRIKE_MUTE_AUDIO_VIDEO and PLAYSTRIKE_SUBTITLE_MALFORMED"]
    fn muted_pcm_survives_app_subtitle_failure() {
        use gstreamer as gst;
        use gstreamer_app as gst_app;

        fn drain(sink: &gst_app::AppSink) {
            while sink.try_pull_sample(gst::ClockTime::ZERO).is_some() {}
        }

        fn assert_silent(sample: &gst::Sample, phase: &str) {
            let buffer = sample.buffer().expect("audio sample buffer");
            let map = buffer.map_readable().expect("readable PCM");
            let bytes = map.as_slice();
            assert!(!bytes.is_empty(), "{phase}: empty PCM sample");
            assert!(
                bytes.iter().all(|byte| *byte == 0),
                "{phase}: nonzero PCM reached the original player sink"
            );
        }

        fn collect_silence(
            app: &mut MediaStrike,
            sink: &gst_app::AppSink,
            ctx: &egui::Context,
            phase: &str,
        ) {
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut checked = 0;
            while Instant::now() < deadline && checked < 3 {
                app.refresh_player(ctx);
                if let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(200)) {
                    assert_silent(&sample, phase);
                    checked += 1;
                }
            }
            assert_eq!(checked, 3, "{phase}: no bounded PCM output to inspect");
        }

        gst::init().expect("GStreamer runtime");
        let media = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_MUTE_AUDIO_VIDEO").expect("PCM fixture"),
        );
        let malformed = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_SUBTITLE_MALFORMED").expect("malformed SRT"),
        );
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", "S16LE")
            .field("rate", 16000i32)
            .field("channels", 1i32)
            .build();
        let sink = gst_app::AppSink::builder()
            .caps(&caps)
            .sync(true)
            .max_buffers(2)
            .drop(true)
            .build();
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.muted = true;
        app.enqueue_media(vec![media.clone()], true, ctx.clone());
        assert!(app.error.is_none(), "initial open failed: {:?}", app.error);
        assert_eq!(app.player.as_ref().unwrap().path, media);
        // Replace only the test player's sink; app subtitle selection and
        // failure handling below still run through the normal MediaStrike path.
        app.player = Some(
            super::Player::open_with_test_audio_sink(&media, None, ctx.clone(), 0.8, true, 1.0, &sink)
                .expect("instrument original player"),
        );
        assert_eq!(app.player.as_ref().unwrap().volume, 0.8);
        assert!(app.player.as_ref().unwrap().muted());
        collect_silence(&mut app, &sink, &ctx, "before subtitle load");

        app.load_external_subtitle(malformed, ctx.clone())
            .expect("start malformed subtitle candidate");
        // Clear already delivered PCM, then flush-seek the original pipeline.
        // The next sink samples belong to output after the app began loading.
        drain(&sink);
        app.player
            .as_ref()
            .unwrap()
            .seek(0.0)
            .expect("flush original during candidate load");
        drain(&sink);
        assert!(app.pending_subtitle_load.is_some());
        assert!(app.pending_subtitle_load.as_ref().unwrap().player.muted());
        assert_eq!(
            app.pending_subtitle_load.as_ref().unwrap().player.volume,
            0.0,
            "candidate starts at zero volume to prevent transient output"
        );
        // Inspect the original sink while the app handles the asynchronous
        // candidate failure. Candidate volume is not used as mute evidence.
        let deadline = Instant::now() + Duration::from_secs(6);
        let mut sampled_after_request = false;
        while Instant::now() < deadline
            && (!sampled_after_request || app.pending_subtitle_load.is_some())
        {
            // Keep the candidate pending until a post-request buffer arrives.
            // A quick malformed-file failure must not end the sampling window.
            if !sampled_after_request {
                if let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                    assert_silent(&sample, "after subtitle load request");
                    sampled_after_request = true;
                }
            }
            if sampled_after_request && app.pending_subtitle_load.is_some() {
                app.refresh_player(&ctx);
            }
        }
        assert!(
            sampled_after_request,
            "original sink produced no post-request PCM within six seconds"
        );
        assert!(app.pending_subtitle_load.is_none(), "candidate did not settle");
        assert!(
            app.error
                .as_ref()
                .is_some_and(|error| error.starts_with("Subtitle loading failed:")),
            "expected app-level asynchronous failure: {:?}",
            app.error
        );
        assert_eq!(app.status, "Kept the current playback");
        let original = app.player.as_ref().expect("original player remains");
        assert_eq!(original.path, media);
        assert_eq!(original.external_subtitle(), None);
        assert_eq!(original.volume, 0.8);
        assert!(app.muted && original.muted());

        original.seek(0.0).expect("restart original after failure");
        drain(&sink);
        collect_silence(&mut app, &sink, &ctx, "after candidate failure");

        app.toggle_mute();
        assert!(!app.muted && !app.player.as_ref().unwrap().muted());
        app.player.as_ref().unwrap().seek(0.0).expect("positive control");
        drain(&sink);
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut saw_nonzero = false;
        while Instant::now() < deadline && !saw_nonzero {
            app.refresh_player(&ctx);
            if let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(200)) {
                let buffer = sample.buffer().expect("audio sample buffer");
                let map = buffer.map_readable().expect("readable PCM");
                saw_nonzero = map.as_slice().iter().any(|byte| *byte != 0);
            }
        }
        assert!(saw_nonzero, "unmuted original produced no nonzero PCM");
    }

    #[test]
    #[ignore = "requires generated local videos and SRT via PLAYSTRIKE_SUBTITLE_VIDEO, PLAYSTRIKE_SAMPLE, and PLAYSTRIKE_SUBTITLE_SAMPLE"]
    fn mute_survives_queue_and_subtitle_replacement() {
        gstreamer::init().expect("GStreamer runtime");
        let video = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("video"));
        let next = PathBuf::from(std::env::var_os("PLAYSTRIKE_SAMPLE").expect("next video"));
        let subtitle = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![video.clone(), next], true, ctx.clone());
        app.toggle_mute();
        assert!(app.muted);
        assert!(app.player.as_ref().unwrap().muted());
        assert_eq!(app.player.as_ref().unwrap().volume, 0.8);
        app.next_track(ctx.clone());
        assert!(app.player.as_ref().unwrap().muted());
        app.previous_track(ctx.clone());
        assert_eq!(app.player.as_ref().unwrap().path, video);
        assert!(app.player.as_ref().unwrap().muted());
        // Let the replacement after Previous finish preroll before opening
        // another pipeline; the queue transition itself was checked above.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && app.player.as_ref().is_some_and(|player| {
                !player.prerolled() || player.duration().is_none()
            })
        {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            app.player
                .as_ref()
                .is_some_and(|player| player.prerolled() && player.duration().is_some()),
            "queue replacement did not preroll before subtitle load"
        );
        app.load_external_subtitle(subtitle, ctx.clone()).expect("load SRT");
        assert!(app.pending_subtitle_load.as_ref().unwrap().player.muted());
        app.toggle_mute();
        assert!(!app.pending_subtitle_load.as_ref().unwrap().player.muted());
        app.toggle_mute();
        assert!(app.pending_subtitle_load.as_ref().unwrap().player.muted());
        wait_for_pending_subtitle(&mut app, &ctx);
        assert!(app.player.as_ref().unwrap().muted());
        app.toggle_mute();
        assert!(!app.player.as_ref().unwrap().muted());
        assert_eq!(app.player.as_ref().unwrap().volume, 0.8);
    }

    #[test]
    #[ignore = "requires two generated local AVIs via PLAYSTRIKE_SAMPLE and PLAYSTRIKE_SAMPLE_SECOND"]
    fn native_queue_controls_and_end_of_file_advance() {
        gstreamer::init().expect("GStreamer runtime");
        let first = PathBuf::from(std::env::var_os("PLAYSTRIKE_SAMPLE").expect("first AVI"));
        let second =
            PathBuf::from(std::env::var_os("PLAYSTRIKE_SAMPLE_SECOND").expect("second AVI"));
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![first.clone(), second.clone()], true, ctx.clone());
        assert_eq!(app.queue.current(), Some(0));
        assert_eq!(app.player.as_ref().unwrap().path, first);
        assert!(app.error.is_none());

        app.next_track(ctx.clone());
        assert_eq!(app.queue.current(), Some(1));
        assert_eq!(app.player.as_ref().unwrap().path, second);
        app.previous_track(ctx.clone());
        assert_eq!(app.queue.current(), Some(0));
        assert_eq!(app.player.as_ref().unwrap().path, first);

        app.skip(0.5);
        assert!(
            app.error.is_none(),
            "early seek should wait for file discovery"
        );
        let ready_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < ready_deadline && app.player.as_ref().unwrap().duration().is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.player.as_ref().unwrap().duration().is_some());
        app.toggle();
        assert!(!app.player.as_ref().unwrap().playing);
        app.skip(0.5);
        assert!(app.error.is_none(), "seek should succeed on the AVI");
        app.volume = 0.4;
        app.player.as_mut().unwrap().set_volume(app.volume);
        assert_eq!(app.player.as_ref().unwrap().volume, 0.4);
        app.toggle();
        assert!(app.player.as_ref().unwrap().playing);

        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline && app.queue.current() != Some(1) {
            app.refresh_player(&ctx);
            assert!(app.error.is_none(), "first AVI should not fail");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.queue.current(), Some(1), "EOS should open second AVI");
        assert_eq!(app.player.as_ref().unwrap().path, second);

        while Instant::now() < deadline && app.status != "Finished" {
            app.refresh_player(&ctx);
            assert!(app.error.is_none(), "second AVI should not fail");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            app.status, "Finished",
            "repeat-off should stop after second AVI"
        );
        assert_eq!(app.queue.current(), Some(1));
        assert!(!app.player.as_ref().unwrap().playing);
    }

    #[test]
    #[ignore = "requires synthetic AVI, SRT, VTT, and malformed SRT fixture environment variables"]
    fn subtitle_file_choice_keeps_playback_on_cancel_and_async_failure() {
        gstreamer::init().expect("GStreamer runtime");
        let video = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("video"));
        let srt = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let vtt = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VTT").expect("VTT"));
        let malformed = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_SUBTITLE_MALFORMED").expect("malformed SRT"),
        );
        let ctx = egui::Context::default();
        let mut app = synthetic_app();
        app.enqueue_media(vec![video.clone()], true, ctx.clone());
        let ready = Instant::now() + Duration::from_secs(5);
        while Instant::now() < ready && app.player.as_ref().unwrap().duration().is_none() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.player.as_ref().unwrap().duration().is_some());
        app.player.as_ref().unwrap().seek(2.0).unwrap();
        let before = wait_for_position_at_least(&mut app, &ctx, 1.5);
        assert!(before > 1.0);

        let status = app.status.clone();
        app.apply_subtitle_file_choice(None, ctx.clone());
        assert!(app.pending_subtitle_load.is_none());
        assert_eq!(app.status, status);
        assert!(app.player.as_ref().unwrap().playing);

        let missing = video.with_extension("missing.srt");
        app.apply_subtitle_file_choice(Some(missing), ctx.clone());
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .starts_with("Subtitle file not found:")
        );
        assert!(app.pending_subtitle_load.is_none());
        assert!(app.player.as_ref().unwrap().playing);

        app.apply_subtitle_file_choice(Some(malformed.clone()), ctx.clone());
        assert!(
            app.pending_subtitle_load.is_some(),
            "malformed nonempty SRT loads asynchronously"
        );
        assert!(app.player.as_ref().unwrap().playing);
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.pending_subtitle_load.is_none());
        assert!(app.error.is_some(), "asynchronous failure must be visible");
        let current = app.player.as_ref().unwrap();
        assert_eq!(current.path, video);
        assert_eq!(current.external_subtitle(), None);
        assert!(current.playing, "working player must keep playing");
        assert!(wait_for_position_at_least(&mut app, &ctx, before - 0.5) >= before - 0.5);

        app.apply_subtitle_file_choice(Some(srt.clone()), ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.pending_subtitle_load.is_none());
        assert!(
            app.error.is_none(),
            "valid SRT should replace after preroll: {:?}",
            app.error
        );
        let current = app.player.as_ref().unwrap();
        assert_eq!(
            current.external_subtitle(),
            Some(srt.canonicalize().unwrap().as_path())
        );
        assert!(current.playing);
        assert!(wait_for_position_at_least(&mut app, &ctx, before - 0.5) >= before - 0.5);

        app.player.as_mut().unwrap().toggle().unwrap();
        app.player.as_ref().unwrap().seek(3.0).unwrap();
        let paused_at = wait_for_position_at_least(&mut app, &ctx, 2.5);
        assert!(!app.player.as_ref().unwrap().playing);
        app.apply_subtitle_file_choice(Some(vtt.clone()), ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.pending_subtitle_load.is_none());
        assert!(
            app.error.is_none(),
            "valid VTT should replace after preroll: {:?}",
            app.error
        );
        let current = app.player.as_ref().unwrap();
        assert_eq!(
            current.external_subtitle(),
            Some(vtt.canonicalize().unwrap().as_path())
        );
        assert!(!current.playing, "paused state must survive replacement");
        assert!((wait_for_position_at_least(&mut app, &ctx, 2.5) - paused_at).abs() < 0.6);

        app.apply_subtitle_file_choice(Some(malformed), ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline && app.pending_subtitle_load.is_some() {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.pending_subtitle_load.is_none());
        assert!(app.error.is_some());
        let current = app.player.as_ref().unwrap();
        assert_eq!(
            current.external_subtitle(),
            Some(vtt.canonicalize().unwrap().as_path())
        );
        assert!(!current.playing, "asynchronous failure must preserve pause");
        assert!((wait_for_position_at_least(&mut app, &ctx, 2.5) - paused_at).abs() < 0.6);
    }

    #[test]
    #[ignore = "requires generated two-track Matroska and SRT fixture environment variables"]
    fn pending_subtitle_choice_off_survives() {
        gstreamer::init().expect("GStreamer runtime");
        let srt = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let ctx = egui::Context::default();
        let (mut app, _) = two_track_app(&ctx);

        app.apply_subtitle_file_choice(Some(srt), ctx.clone());
        assert!(app.pending_subtitle_load.is_some());
        app.choose_subtitle(None);
        assert!(app.error.is_none());

        wait_for_pending_subtitle(&mut app, &ctx);
        assert_eq!(app.player.as_ref().unwrap().selected_subtitle(), None);
    }

    #[test]
    #[ignore = "requires generated two-track Matroska and SRT fixture environment variables"]
    fn pending_subtitle_choice_other_track_survives() {
        gstreamer::init().expect("GStreamer runtime");
        let srt = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let ctx = egui::Context::default();
        let (mut app, second) = two_track_app(&ctx);

        app.apply_subtitle_file_choice(Some(srt), ctx.clone());
        assert!(app.pending_subtitle_load.is_some());
        app.choose_subtitle(Some(&second));
        assert!(app.error.is_none());

        wait_for_pending_subtitle(&mut app, &ctx);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && app.player.as_ref().unwrap().selected_subtitle() != Some(second.as_str())
        {
            app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            app.player.as_ref().unwrap().selected_subtitle(),
            Some(second.as_str())
        );
    }

    #[test]
    #[ignore = "requires generated AVI, SRT, and two-track Matroska fixture environment variables"]
    fn repeat_one_keeps_external_and_embedded_subtitle_choices() {
        fn replay_from_near_end(app: &mut MediaStrike, ctx: &egui::Context) {
            app.status.clear();
            // GStreamer can expose subtitle tracks before duration queries
            // settle. Wait for the seekable fixture as the queue test does.
            let ready_deadline = Instant::now() + Duration::from_secs(5);
            let duration = loop {
                if let Some(duration) = app.player.as_ref().unwrap().duration() {
                    break duration;
                }
                assert!(Instant::now() < ready_deadline, "synthetic video duration did not settle");
                app.refresh_player(ctx);
                assert!(app.error.is_none(), "fixture playback failed: {:?}", app.error);
                std::thread::sleep(Duration::from_millis(20));
            };
            assert!(duration > 1.0, "repeat fixture needs a seekable duration");
            app.player.as_ref().unwrap().seek(duration - 0.9).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && !app.status.starts_with("Replaying ") {
                app.refresh_player(ctx);
                assert!(app.error.is_none(), "replay failed: {:?}", app.error);
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                app.status.starts_with("Replaying "),
                "same-item repeat did not occur"
            );
            assert!(app.player.as_ref().unwrap().playing);
        }

        gstreamer::init().expect("GStreamer runtime");
        let video = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_VIDEO").expect("AVI"));
        let srt = PathBuf::from(std::env::var_os("PLAYSTRIKE_SUBTITLE_SAMPLE").expect("SRT"));
        let embedded = PathBuf::from(
            std::env::var_os("PLAYSTRIKE_EMBEDDED_SUBTITLE_VIDEO").expect("two-track MKV"),
        );
        let ctx = egui::Context::default();

        let mut external_app = synthetic_app();
        external_app.enqueue_media(vec![video], true, ctx.clone());
        external_app.apply_subtitle_file_choice(Some(srt.clone()), ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && external_app.pending_subtitle_load.is_some() {
            external_app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(external_app.pending_subtitle_load.is_none());
        assert!(
            external_app.error.is_none(),
            "external subtitle failed: {:?}",
            external_app.error
        );
        external_app.queue.cycle_repeat();
        external_app.queue.cycle_repeat();
        replay_from_near_end(&mut external_app, &ctx);
        assert_eq!(
            external_app.player.as_ref().unwrap().external_subtitle(),
            Some(srt.canonicalize().unwrap().as_path()),
        );

        let mut embedded_app = synthetic_app();
        embedded_app.enqueue_media(vec![embedded], true, ctx.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && embedded_app
                .player
                .as_ref()
                .unwrap()
                .subtitle_tracks()
                .len()
                < 2
        {
            embedded_app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let tracks = embedded_app.player.as_ref().unwrap().subtitle_tracks();
        assert_eq!(tracks.len(), 2);
        let second = tracks[1].id.clone();
        embedded_app
            .player
            .as_mut()
            .unwrap()
            .select_subtitle(Some(&second))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && embedded_app.player.as_ref().unwrap().selected_subtitle() != Some(second.as_str())
        {
            embedded_app.refresh_player(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            embedded_app.player.as_ref().unwrap().selected_subtitle(),
            Some(second.as_str())
        );
        embedded_app.queue.cycle_repeat();
        embedded_app.queue.cycle_repeat();
        replay_from_near_end(&mut embedded_app, &ctx);
        assert_eq!(
            embedded_app.player.as_ref().unwrap().selected_subtitle(),
            Some(second.as_str())
        );

        embedded_app
            .player
            .as_mut()
            .unwrap()
            .select_subtitle(None)
            .unwrap();
        assert_eq!(
            embedded_app.player.as_ref().unwrap().selected_subtitle(),
            None
        );
        replay_from_near_end(&mut embedded_app, &ctx);
        assert_eq!(
            embedded_app.player.as_ref().unwrap().selected_subtitle(),
            None
        );
    }

    #[test]
    fn time_label_handles_long_media() {
        assert_eq!(format_time(65.9), "1:05");
        assert_eq!(format_time(3661.0), "1:01:01");
    }

    #[test]
    fn accepts_multiple_local_files_before_capture_option() {
        let args = ["first.avi", "second.avi", "--capture", "new.png"]
            .into_iter()
            .map(OsString::from);
        let parsed = parse_launch_args(args).expect("valid multi-file launch");
        assert_eq!(
            parsed.media_paths,
            [PathBuf::from("first.avi"), PathBuf::from("second.avi")]
        );
        assert_eq!(parsed.capture_path, Some(PathBuf::from("new.png")));
        assert_eq!(parsed.subtitle_path, None);
        assert!(
            parse_launch_args([OsString::from("--capture"), OsString::from("new.png")]).is_err()
        );
    }

    #[test]
    fn accepts_one_external_subtitle_before_capture() {
        let args = [
            "first.avi",
            "--subtitle",
            "captions.srt",
            "--capture",
            "new.png",
        ]
        .into_iter()
        .map(OsString::from);
        let parsed = parse_launch_args(args).expect("valid subtitle launch");
        assert_eq!(parsed.subtitle_path, Some(PathBuf::from("captions.srt")));
        assert!(
            parse_launch_args([OsString::from("--subtitle"), OsString::from("captions.srt")])
                .is_err()
        );
    }

    #[test]
    fn focus_view_launch_keeps_the_media_list_intact() {
        let args = ["first.avi", "--focus-view", "--capture", "focus.png"]
            .into_iter()
            .map(OsString::from);
        let parsed = parse_launch_args(args).expect("valid focus-view capture launch");
        assert_eq!(parsed.media_paths, [PathBuf::from("first.avi")]);
        assert_eq!(parsed.capture_path, Some(PathBuf::from("focus.png")));
        assert!(parsed.focus_view);
        assert!(parse_launch_args([OsString::from("--focus-view")]).is_err());
        assert!(
            parse_launch_args([
                OsString::from("first.avi"),
                OsString::from("--focus-view"),
                OsString::from("--focus-view"),
            ])
            .is_err()
        );
    }
}

