mod player;

use eframe::egui;
use player::Player;
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
        media_path: Option<PathBuf>,
        capture: Option<CaptureOnce>,
    ) -> Self {
        let (gst_ready, error) = match gstreamer::init() {
            Ok(()) => (true, None),
            Err(err) => (false, Some(format!("GStreamer could not start: {err}"))),
        };
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = Self {
            player: None,
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
        if app.gst_ready {
            if let Some(path) = media_path {
                app.open(path, cc.egui_ctx.clone());
            }
        }
        app
    }

    fn open(&mut self, path: PathBuf, ctx: egui::Context) {
        if !self.gst_ready {
            return;
        }
        match Player::open(&path, ctx, self.volume) {
            Ok(player) => {
                self.player = Some(player);
                self.texture = None;
                self.seek_preview = None;
                self.error = None;
                self.status = format!("Playing {}", file_label(&path));
            }
            Err(err) => self.error = Some(err),
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
                    Ok(()) => self.status = "Finished".to_owned(),
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
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .find_map(|file| file.path.clone())
        });
        if let Some(path) = dropped {
            self.open(path, ctx.clone());
        }

        let open_shortcut =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::O));
        if self.player.is_some() {
            let (toggle, back, forward) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::Space),
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
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
        }

        let mut pick_file = open_shortcut;
        egui::TopBottomPanel::top("identity").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(player) = &self.player {
                    ui.label(file_label(&player.path));
                } else {
                    ui.label(egui::RichText::new("Local media player").weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = true;
                    }
                    if ui
                        .add_enabled(self.gst_ready, egui::Button::new("Open file…"))
                        .clicked()
                    {
                        pick_file = true;
                    }
                });
            });
            ui.add_space(8.0);
        });

        egui::TopBottomPanel::bottom("transport").show(ctx, |ui| {
            ui.add_space(8.0);
            if let Some(player) = &mut self.player {
                let position = player.position().unwrap_or(0.0);
                let duration = player.duration().filter(|t| *t > 0.0);
                if let Some(duration) = duration {
                    let mut value = self.seek_preview.unwrap_or(position).clamp(0.0, duration);
                    let response = ui.add(
                        egui::Slider::new(&mut value, 0.0..=duration)
                            .show_value(false)
                            .text("Position"),
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
                    if ui
                        .button(if player.playing { "Pause" } else { "Play" })
                        .clicked()
                    {
                        if let Err(err) = player.toggle() {
                            self.error = Some(err);
                        } else {
                            self.status =
                                if player.playing { "Playing" } else { "Paused" }.to_owned();
                        }
                    }
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
                });
            } else {
                ui.label("Open a file to show playback controls");
            }
            ui.label(egui::RichText::new(&self.status).weak());
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(255, 153, 127), error);
            }
            ui.add_space(8.0);
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(12, 16, 23)))
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
                            ui.label("Drop a video or audio file here");
                            if ui
                                .add_enabled(self.gst_ready, egui::Button::new("Open file"))
                                .clicked()
                            {
                                pick_file = true;
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

        if pick_file {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                self.open(path, ctx.clone());
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

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let media_path = args.next().map(PathBuf::from);
    let capture_path = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(flag), Some(output), None) if flag == "--capture" && media_path.is_some() => {
            let path = PathBuf::from(output);
            if path.extension().is_none_or(|ext| ext != "png") {
                eprintln!("Capture path must end in .png");
                return ExitCode::FAILURE;
            }
            Some(path)
        }
        _ => {
            eprintln!("MediaStrike usage: playstrike [MEDIA_FILE [--capture FILE.png]]");
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
        Box::new(move |cc| Ok(Box::new(MediaStrike::new(cc, media_path, capture)))),
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
    use super::format_time;

    #[test]
    fn time_label_handles_long_media() {
        assert_eq!(format_time(65.9), "1:05");
        assert_eq!(format_time(3661.0), "1:01:01");
    }
}
