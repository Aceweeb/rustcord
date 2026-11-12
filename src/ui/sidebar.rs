//! Guild rail and channel list, laid out to Discord's own measurements:
//! a 72 px rail, 48 px circular guild icons that become rounded squares when
//! selected (with the white pill beside them), a 240 px channel list, 48 px guild
//! header and 34 px channel rows.

use crate::app::RustcordApp;
use crate::models::{ChannelKind, Presence};
use crate::ui::theme::AppTheme;
use crate::utils::{format_duration, initial, lift};
use egui::{Color32, RichText};

const RAIL_WIDTH: f32 = 72.0;
const CHANNEL_LIST_WIDTH: f32 = 240.0;
const GUILD_ICON: f32 = 48.0;
const CHANNEL_ROW_HEIGHT: f32 = 34.0;
/// Rows are inset from the panel edge, as Discord's are.
const ROW_INSET: f32 = 8.0;

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    guild_rail(app, ui);
    channel_list(app, ui);
}

fn guild_rail(app: &mut RustcordApp, ui: &mut egui::Ui) {
    let theme = app.plugins.theme().clone();
    let mut selected_guild: Option<String> = None;

    egui::Panel::left("guild_rail")
        .exact_size(RAIL_WIDTH)
        .resizable(false)
        .frame(egui::Frame::new().fill(theme.surface_alt))
        .show(ui, |ui| {
            ui.add_space(12.0);
            // A bot in 23 guilds does not fit an 800 px window; without this the
            // icons past the bottom edge were unreachable.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        for guild in &app.guilds {
                            let selected = app.selected_guild.as_deref() == Some(guild.id.as_str());

                            if selected {
                                // Discord's white pill, hugging the left edge.
                                let top = ui.cursor().top();
                                let rail_left = ui.max_rect().left();
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(rail_left, top + 10.0),
                                        egui::vec2(4.0, GUILD_ICON - 20.0),
                                    ),
                                    2.0,
                                    Color32::WHITE,
                                );
                            }

                            // Circle when idle, rounded square when active.
                            let radius = if selected {
                                16
                            } else {
                                (GUILD_ICON / 2.0) as u8
                            };
                            let fill = if selected {
                                theme.accent
                            } else {
                                theme.surface
                            };
                            let response = ui.add(
                                egui::Button::new(
                                    RichText::new(initial(&guild.name))
                                        .size(18.0)
                                        .color(Color32::WHITE),
                                )
                                .min_size(egui::vec2(GUILD_ICON, GUILD_ICON))
                                .corner_radius(egui::CornerRadius::same(radius))
                                .fill(fill),
                            );
                            if response.clicked() {
                                selected_guild = Some(guild.id.clone());
                            }
                            ui.add_space(8.0);
                        }
                    });
                });
        });

    if let Some(id) = selected_guild {
        app.selected_guild = Some(id.clone());
        if let Some(first) = app
            .channels
            .iter()
            .find(|channel| channel.guild_id == id && channel.kind == ChannelKind::Text)
        {
            app.selected_channel = Some(first.id.clone());
        }
    }
}

fn channel_list(app: &mut RustcordApp, ui: &mut egui::Ui) {
    let theme = app.plugins.theme().clone();
    let mut clicked: Option<String> = None;
    let mut open_settings = false;

    let guild_name = app
        .selected_guild
        .as_ref()
        .and_then(|id| app.guilds.iter().find(|guild| &guild.id == id))
        .map(|guild| guild.name.clone())
        .unwrap_or_else(|| "Rustcord".to_string());

    egui::Panel::left("channel_list")
        .exact_size(CHANNEL_LIST_WIDTH)
        .resizable(false)
        .frame(egui::Frame::new().fill(theme.surface))
        .show(ui, |ui| {
            // Guild header: name, chevron, settings, 48 px like Discord's.
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 0))
                .show(ui, |ui| {
                    ui.set_height(48.0);
                    ui.horizontal_centered(|ui| {
                        ui.label(
                            RichText::new(&guild_name)
                                .size(15.0)
                                .strong()
                                .color(theme.text),
                        );
                        ui.label(RichText::new("⌄").size(11.0).color(theme.muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("⚙").color(theme.muted))
                                        .frame(false),
                                )
                                .on_hover_text("Settings (Ctrl+,)")
                                .clicked()
                            {
                                open_settings = true;
                            }
                        });
                    });
                });

            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                egui::Stroke::new(1.0, lift(theme.surface, -8)),
            );
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    category_label(ui, &theme, "TEXT CHANNELS");
                    for channel in app.channels.iter().filter(|channel| {
                        Some(&channel.guild_id) == app.selected_guild.as_ref()
                            && channel.kind == ChannelKind::Text
                    }) {
                        let selected = app.selected_channel.as_deref() == Some(channel.id.as_str());
                        if channel_row(ui, &theme, selected, "#", &channel.name) {
                            clicked = Some(channel.id.clone());
                        }
                    }

                    ui.add_space(14.0);
                    category_label(ui, &theme, "VOICE CHANNELS");
                    for channel in app.channels.iter().filter(|channel| {
                        Some(&channel.guild_id) == app.selected_guild.as_ref()
                            && channel.kind == ChannelKind::Voice
                    }) {
                        let selected = app.selected_channel.as_deref() == Some(channel.id.as_str());
                        if channel_row(ui, &theme, selected, "🔊", &channel.name) {
                            clicked = Some(channel.id.clone());
                        }

                        // Who is in the channel, as Discord lists them under it.
                        // Live gateway data only: there is no audio here (D24).
                        for member in app.voice.members(&channel.id) {
                            voice_member_row(ui, &theme, &member.username);
                        }
                    }
                    ui.add_space(8.0);
                });

            // Account panel, pinned to the bottom like Discord's.
            egui::Frame::new()
                .fill(theme.surface_alt)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    bottom_panels(app, ui, &theme);
                });
        });

    if let Some(id) = clicked {
        app.selected_channel = Some(id);
    }
    if open_settings {
        app.show_settings = true;
    }
}

fn bottom_panels(app: &mut RustcordApp, ui: &mut egui::Ui, theme: &AppTheme) {
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        if let Some(banner) = app.plugins.blyat_banner() {
            ui.label(RichText::new(banner).italics().small().color(theme.muted));
        }

        ui.separator();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 16.0, theme.accent);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial(&app.current_user.username),
                egui::FontId::proportional(13.0),
                Color32::WHITE,
            );

            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&app.current_user.username)
                        .size(13.0)
                        .strong()
                        .color(theme.text),
                );
                let (status, color) = match app.current_user.status {
                    Presence::Online => ("Online", theme.success),
                    Presence::Idle => ("Idle", Color32::YELLOW),
                    Presence::Dnd => ("Do not disturb", theme.danger),
                    Presence::Offline => ("Offline", theme.muted),
                };
                ui.label(RichText::new(status).size(11.0).color(color));
            });
        });

        if let Some(activity) = app.plugins.spotify_activity() {
            let artist = activity.details.as_deref().unwrap_or("unknown artist");
            ui.label(
                RichText::new(format!("🎵 {} — {}", activity.name, artist))
                    .size(12.0)
                    .color(theme.text),
            );

            let duration = app.plugins.spotify_duration_ms();
            if duration > 0 {
                let progress = app.plugins.spotify_progress_ms() as f32 / duration as f32;
                ui.add(
                    egui::ProgressBar::new(progress.clamp(0.0, 1.0))
                        .desired_width(200.0)
                        .fill(theme.accent),
                );
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        format_duration(app.plugins.spotify_progress_ms()),
                        format_duration(duration)
                    ))
                    .size(10.0)
                    .color(theme.muted),
                );
            }
        }
    });
}

/// A person sitting in a voice channel: a small circle and their name, indented
/// under the channel like Discord's.
fn voice_member_row(ui: &mut egui::Ui, theme: &AppTheme, username: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    let indent = ROW_INSET + 30.0;

    let centre = rect.left_center() + egui::vec2(indent + 6.0, 0.0);
    ui.painter().circle_filled(centre, 4.0, theme.success);
    ui.painter().text(
        centre + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        username,
        egui::FontId::proportional(13.0),
        theme.muted,
    );
}

fn category_label(ui: &mut egui::Ui, theme: &AppTheme, label: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(16.0, 2.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        theme.muted,
    );
}

/// One channel row. Painted rather than built from widgets so the row height,
/// hover fill and inset match Discord's exactly; `selected` rows get a lifted
/// surface and full-strength text.
fn channel_row(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    selected: bool,
    glyph: &str,
    name: &str,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CHANNEL_ROW_HEIGHT),
        egui::Sense::click(),
    );

    let body = rect.shrink2(egui::vec2(ROW_INSET, 1.0));
    if selected {
        ui.painter().rect_filled(body, 4.0, lift(theme.surface, 22));
    } else if response.hovered() {
        ui.painter().rect_filled(body, 4.0, lift(theme.surface, 10));
    }

    let text_color = if selected { theme.text } else { theme.muted };
    ui.painter().text(
        rect.left_center() + egui::vec2(ROW_INSET + 8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        glyph,
        egui::FontId::proportional(16.0),
        text_color,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(ROW_INSET + 30.0, 0.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(15.0),
        text_color,
    );

    response.clicked()
}
