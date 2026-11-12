use crate::app::RustcordApp;
use crate::models::{ChannelKind, Presence};
use crate::utils::{format_duration, initial};
use egui::{Color32, RichText};

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    egui::Panel::left("guild_rail")
        .exact_size(72.0)
        .resizable(false)
        .show(ui, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                for guild in &app.guilds {
                    let selected = app.selected_guild.as_deref() == Some(guild.id.as_str());
                    let text = RichText::new(initial(&guild.name))
                        .size(18.0)
                        .color(Color32::WHITE);
                    let fill = if selected {
                        app.plugins.theme().accent
                    } else {
                        app.plugins.theme().surface_alt
                    };
                    let response = ui.add(
                        egui::Button::new(text)
                            .min_size(egui::vec2(48.0, 48.0))
                            .fill(fill),
                    );
                    if response.clicked() {
                        app.selected_guild = Some(guild.id.clone());
                        if let Some(first) = app.channels.iter().find(|channel| {
                            channel.guild_id == guild.id && channel.kind == ChannelKind::Text
                        }) {
                            app.selected_channel = Some(first.id.clone());
                        }
                    }
                    ui.add_space(6.0);
                }
            });
        });

    egui::Panel::left("channel_list")
        .default_size(250.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.add_space(8.0);
            let guild_name = app
                .selected_guild
                .as_ref()
                .and_then(|id| app.guilds.iter().find(|guild| &guild.id == id))
                .map(|guild| guild.name.as_str())
                .unwrap_or("Rustcord");

            ui.horizontal(|ui| {
                ui.heading(RichText::new(guild_name).size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").on_hover_text("Settings (Ctrl+,)").clicked() {
                        app.show_settings = true;
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("TEXT CHANNELS")
                            .small()
                            .color(app.plugins.theme().muted),
                    );
                    ui.add_space(2.0);
                    for channel in app.channels.iter().filter(|channel| {
                        Some(&channel.guild_id) == app.selected_guild.as_ref()
                            && channel.kind == ChannelKind::Text
                    }) {
                        let selected = app.selected_channel.as_deref() == Some(channel.id.as_str());
                        let label = format!("#  {}", channel.name);
                        if ui.selectable_label(selected, label).clicked() {
                            app.selected_channel = Some(channel.id.clone());
                        }
                    }

                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("VOICE CHANNELS")
                            .small()
                            .color(app.plugins.theme().muted),
                    );
                    ui.add_space(2.0);
                    for channel in app.channels.iter().filter(|channel| {
                        Some(&channel.guild_id) == app.selected_guild.as_ref()
                            && channel.kind == ChannelKind::Voice
                    }) {
                        let selected = app.selected_channel.as_deref() == Some(channel.id.as_str());
                        let label = format!("🔊  {}", channel.name);
                        if ui.selectable_label(selected, label).clicked() {
                            app.selected_channel = Some(channel.id.clone());
                        }
                    }
                });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                if let Some(banner) = app.plugins.blyat_banner() {
                    ui.label(
                        RichText::new(banner)
                            .italics()
                            .color(app.plugins.theme().muted),
                    );
                }

                if let Some(activity) = app.plugins.spotify_activity() {
                    ui.add_space(4.0);
                    let artist = activity.details.as_deref().unwrap_or("unknown artist");
                    ui.label(format!("🎵 {} — {}", activity.name, artist));

                    let duration = app.plugins.spotify_duration_ms();
                    if duration > 0 {
                        let progress = app.plugins.spotify_progress_ms() as f32 / duration as f32;
                        ui.add(
                            egui::ProgressBar::new(progress.clamp(0.0, 1.0))
                                .desired_width(220.0)
                                .show_percentage(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                format_duration(app.plugins.spotify_progress_ms()),
                                format_duration(duration)
                            ))
                            .small()
                            .color(app.plugins.theme().muted),
                        );
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 16.0, app.plugins.theme().accent);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        initial(&app.current_user.username),
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );

                    ui.vertical(|ui| {
                        ui.label(RichText::new(&app.current_user.username).strong());
                        let (status, color) = match app.current_user.status {
                            Presence::Online => ("Online", app.plugins.theme().success),
                            Presence::Idle => ("Idle", Color32::YELLOW),
                            Presence::Dnd => ("Do not disturb", app.plugins.theme().danger),
                            Presence::Offline => ("Offline", app.plugins.theme().muted),
                        };
                        ui.label(RichText::new(status).small().color(color));
                    });
                });
            });
        });
}
