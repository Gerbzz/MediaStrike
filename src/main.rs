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

struct CaptureOnce {
    path: PathBuf,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
    started: Instant,
    settle_frames: u8,
    requested: bool,
}

impl CaptureOnce {
    fn new(path: PathBuf, outcome: Arc<Mutex<Option<Result<(), String>>>>) -> Self {
        Self {
            path,
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
    texture: Option<egui::TextureHandle>,
    brand_texture: Option<egui::TextureHandle>,
    show_about: bool,
    seek_preview: Option<f64>,
    volume: f64,
    status: String,
    error: Option<String>,
    gst_ready: bool,
    capture: Option<CaptureOnce>,
}

impl MediaStrike {
    fn new(
        cc: &eframe::CreationContext<'_>,
        media_paths: Vec<PathBuf>,
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
            texture: None,
            brand_texture: load_brand_texture(&cc.egui_ctx),
            show_about: false,
            seek_preview: None,
            volume: 0.8,
            status: "Open or drop a local media file".to_owned(),
            error,
            gst_ready,
            capture,
        };
        if app.gst_ready && !media_paths.is_empty() {
            app.enqueue_media(media_paths, true, cc.egui_ctx.clone());
        }
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
                self.error = None;
                self.status = format!("Playing {}", file_label(&path));
            }
            Err(err) => self.error = Some(err),
        }
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
            let current = player.position().unwrap_or(0.0);
            let target = if let Some(end) = player.duration() {
                (current + delta).clamp(0.0, end)
            } else {
                (current + delta).max(0.0)
            };
            if let Err(err) = player.seek(target) {
                self.error = Some(err);
            }
        }
    }

    fn refresh_player(&mut self, ctx: &egui::Context) {
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
        } else if finished {
            if let Some(index) = self.queue.finished_index() {
                self.open_index(index, ctx.clone());
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

impl eframe::App for MediaStrike {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_player(ctx);
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

        let open_shortcut =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::O));
        let mut pick_files = open_shortcut.then_some(true);
        if self.player.is_some() {
            let (toggle, back, forward, previous, next) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::Space),
                    !input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft),
                    !input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight),
                    input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft),
                    input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight),
                )
            });
            if toggle {
                self.toggle();
            }
            if back {
                self.skip(-5.0);
            }
            if forward {
                self.skip(5.0);
            }
            if previous {
                self.previous_track(ctx.clone());
            }
            if next {
                self.next_track(ctx.clone());
            }
        }

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
                        ui.label(egui::RichText::new(file_label(&player.path)).weak());
                    } else {
                        ui.label(egui::RichText::new("Your media, in one place").weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("About").clicked() {
                            self.show_about = true;
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

        let mut previous_clicked = false;
        let mut next_clicked = false;
        egui::TopBottomPanel::bottom("transport")
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(24, 24, 24)))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                if let Some(player) = &mut self.player {
                    let position = player.position().unwrap_or(0.0);
                    let duration = player.duration().filter(|t| *t > 0.0);
                    if let Some(duration) = duration {
                        let mut value = self.seek_preview.unwrap_or(position).clamp(0.0, duration);
                        let response = ui.add_sized(
                            [ui.available_width(), 18.0],
                            egui::Slider::new(&mut value, 0.0..=duration)
                                .show_value(false)
                                .text("Seek"),
                        );
                        if response.dragged() {
                            self.seek_preview = Some(value);
                        }
                        if response.drag_stopped() || (response.changed() && !response.dragged()) {
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
                                egui::Button::new(if player.playing { "Pause" } else { "Play" })
                                    .fill(egui::Color32::from_rgb(176, 79, 8)),
                            )
                            .clicked()
                        {
                            if let Err(err) = player.toggle() {
                                self.error = Some(err);
                            } else {
                                self.status =
                                    if player.playing { "Playing" } else { "Paused" }.to_owned();
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
                        if ui.button(self.queue.repeat().label()).clicked() {
                            self.queue.cycle_repeat();
                        }
                    });
                } else {
                    ui.label("Open or drop local media to start playback");
                }
                ui.label(egui::RichText::new(&self.status).weak());
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(255, 153, 127), error);
                }
                ui.add_space(6.0);
            });

        if previous_clicked {
            self.previous_track(ctx.clone());
        }
        if next_clicked {
            self.next_track(ctx.clone());
        }

        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(158.0)
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(27, 27, 27)))
            .show(ctx, |ui| {
                ui.add_space(14.0);
                ui.label(egui::RichText::new("YOUR MEDIA").weak().small());
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
                        .weak(),
                );
            });

        let mut select_index = None;
        if self.show_queue {
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
                        .weak(),
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
                            ui.label(egui::RichText::new("by PWDS").weak());
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
                        ui.label(egui::RichText::new("Version 0.1.0").weak());
                    });
                });
        }

        if let Some(play_new) = pick_files {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                self.enqueue_media(paths, play_new, ctx.clone());
            }
        }
        self.capture_after_frame(ctx);
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
    capture_path: Option<PathBuf>,
}

fn parse_launch_args(args: impl IntoIterator<Item = OsString>) -> Result<LaunchArgs, String> {
    let mut args = args.into_iter().peekable();
    let mut media_paths = Vec::new();
    let mut capture_path = None;
    while let Some(arg) = args.next() {
        if arg == "--capture" {
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
    Ok(LaunchArgs {
        media_paths,
        capture_path,
    })
}

fn main() -> ExitCode {
    let LaunchArgs {
        media_paths,
        capture_path,
    } = match parse_launch_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!(
                "{error}\nMediaStrike usage: playstrike [MEDIA_FILE ...] [--capture FILE.png]"
            );
            return ExitCode::FAILURE;
        }
    };
    let capture_result = capture_path.as_ref().map(|_| Arc::new(Mutex::new(None)));
    let capture = capture_path
        .zip(capture_result.clone())
        .map(|(path, outcome)| CaptureOnce::new(path, outcome));
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
        Box::new(move |cc| Ok(Box::new(MediaStrike::new(cc, media_paths, capture)))),
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
    use super::{format_time, parse_launch_args};
    use std::ffi::OsString;
    use std::path::PathBuf;

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
        assert!(
            parse_launch_args([OsString::from("--capture"), OsString::from("new.png")]).is_err()
        );
    }
}
