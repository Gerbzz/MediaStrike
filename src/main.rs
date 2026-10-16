mod player;
mod queue;

use eframe::egui;
use player::Player;
use queue::MediaQueue;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MUTED_TEXT: egui::Color32 = egui::Color32::from_rgb(174, 174, 174);

#[derive(Default, Debug, PartialEq, Eq)]
struct PlayerShortcuts {
    open_files: bool,
    toggle: bool,
    back: bool,
    forward: bool,
    previous: bool,
    next: bool,
    focus_view: bool,
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
    focus_view: bool,
    texture: Option<egui::TextureHandle>,
    brand_texture: Option<egui::TextureHandle>,
    show_about: bool,
    seek_preview: Option<f64>,
    volume: f64,
    status: String,
    error: Option<String>,
    gst_ready: bool,
    capture: Option<CaptureOnce>,
    pending_subtitle_load: Option<PendingSubtitleLoad>,
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
            focus_view: false,
            texture: None,
            brand_texture: load_brand_texture(&cc.egui_ctx),
            show_about: false,
            seek_preview: None,
            volume: 0.8,
            status: "Open or drop a local media file".to_owned(),
            error,
            gst_ready,
            capture,
            pending_subtitle_load: None,
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
        match Player::open(&path, ctx, self.volume) {
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
        let candidate = Player::open_with_subtitle(&media_path, Some(&subtitle_path), ctx, 0.0)?;
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
        pending.player.set_volume(self.volume);
        self.player = Some(pending.player);
        self.texture = None;
        self.seek_preview = None;
        self.error = None;
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

        if !self.focus_view {
            egui::TopBottomPanel::top("identity")
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 24, 24)))
                .show(ctx, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if let Some(texture) = &self.brand_texture {
                            ui.image((texture.id(), egui::vec2(24.0, 24.0)));
                        }
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
                            }
                            ui.label(format!("{}%", (self.volume * 100.0).round() as i32));
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
                        .selectable_label(!self.show_queue, "Now playing")
                        .clicked()
                    {
                        self.show_queue = false;
                    }
                    if ui
                        .selectable_label(self.show_queue, "Session queue")
                        .clicked()
                    {
                        self.show_queue = true;
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("{} local file(s)", self.queue.items().len()))
                            .color(MUTED_TEXT),
                    );
                });
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
                    ui.add(
                        egui::TextEdit::singleline(&mut self.queue_search)
                            .hint_text("Filter queue"),
                    );
                    ui.separator();
                    let filter = self.queue_search.trim().to_lowercase();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, path) in self.queue.items().iter().enumerate() {
                            let label = file_label(path);
                            if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                                continue;
                            }
                            if ui
                                .selectable_label(
                                    self.queue.current() == Some(index),
                                    format!("{:02}  {label}", index + 1),
                                )
                                .clicked()
                            {
                                select_index = Some(index);
                            }
                        }
                    });
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
    use super::{MediaStrike, PlayerShortcuts, format_time, parse_launch_args, player_shortcuts};
    use eframe::egui;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn synthetic_app() -> MediaStrike {
        MediaStrike {
            player: None,
            queue: Default::default(),
            show_queue: true,
            queue_search: String::new(),
            focus_view: false,
            texture: None,
            brand_texture: None,
            show_about: false,
            seek_preview: None,
            volume: 0.8,
            status: String::new(),
            error: None,
            gst_ready: true,
            capture: None,
            pending_subtitle_load: None,
        }
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
        fn replay_from_near_end(app: &mut MediaStrike, ctx: &egui::Context, seconds: f64) {
            app.status.clear();
            app.player.as_ref().unwrap().seek(seconds).unwrap();
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
        replay_from_near_end(&mut external_app, &ctx, 5.1);
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
        replay_from_near_end(&mut embedded_app, &ctx, 7.1);
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
        replay_from_near_end(&mut embedded_app, &ctx, 7.1);
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
