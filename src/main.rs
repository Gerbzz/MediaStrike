mod player;

use eframe::egui;
use player::Player;
use std::path::PathBuf;
use std::time::Duration;

struct PlayStrike {
    player: Option<Player>,
    texture: Option<egui::TextureHandle>,
    seek_preview: Option<f64>,
    volume: f64,
    status: String,
    error: Option<String>,
    gst_ready: bool,
}

impl PlayStrike {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (gst_ready, error) = match gstreamer::init() {
            Ok(()) => (true, None),
            Err(err) => (false, Some(format!("GStreamer could not start: {err}"))),
        };
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = Self {
            player: None,
            texture: None,
            seek_preview: None,
            volume: 0.8,
            status: "Open or drop a local media file".to_owned(),
            error,
            gst_ready,
        };
        if app.gst_ready {
            if let Some(path) = std::env::args_os().nth(1) {
                app.open(PathBuf::from(path), cc.egui_ctx.clone());
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
                        "playstrike-video",
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
}

impl eframe::App for PlayStrike {
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
                ui.heading("PlayStrike");
                ui.label(egui::RichText::new("by PWDS").weak());
                if let Some(player) = &self.player {
                    ui.separator();
                    ui.label(file_label(&player.path));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("Drop a video or audio file here")
                                .size(23.0)
                                .weak(),
                        );
                    });
                }
            });

        if pick_file {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                self.open(path, ctx.clone());
            }
        }
    }
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
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

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title("PlayStrike"),
        ..Default::default()
    };
    eframe::run_native(
        "PlayStrike",
        options,
        Box::new(|cc| Ok(Box::new(PlayStrike::new(cc)))),
    )
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
