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

/// Everything the rail needs to draw a row, grouped so the row helper does not
/// take eight loose arguments.
struct RailView<'a> {
    theme: &'a AppTheme,
    badges: &'a std::collections::BTreeMap<String, (u32, bool)>,
    folders: &'a [(String, String)],
    selected: Option<&'a str>,
    /// Rail index that arrow-key navigation is on, when it is on the rail.
    focus: Option<usize>,
}

#[derive(Debug, Clone)]
enum RailAction {
    Select(String),
    ToggleFolder(String),
    MoveGuild(String, i32),
    MoveFolder(String, i32),
    NewFolder(String),
    AddToFolder(String, String),
    RemoveFromFolder(String),
    DeleteFolder(String),
}

fn guild_rail(app: &mut RustcordApp, ui: &mut egui::Ui) {
    let theme = app.plugins.theme().clone();
    let guilds = app.guilds.clone();
    let items = app.settings.guild_rail.items(&guilds);
    let expanded = app.rail_expanded.clone();
    let selected = app.selected_guild.clone();

    // Badges are computed once per guild instead of rescanning channels per icon.
    let badges: std::collections::BTreeMap<String, (u32, bool)> = guilds
        .iter()
        .map(|guild| {
            let ids = app
                .channels
                .iter()
                .filter(|channel| channel.guild_id == guild.id)
                .map(|channel| channel.id.as_str());
            (
                guild.id.clone(),
                (
                    app.unread.guild_mentions(ids.clone()),
                    app.unread.guild_unread(ids),
                ),
            )
        })
        .collect();
    let folders: Vec<(String, String)> = app
        .settings
        .guild_rail
        .folders
        .iter()
        .map(|folder| (folder.id.clone(), folder.name.clone()))
        .collect();

    let focus = if app.navigator.active && app.navigator.area == crate::nav::NavArea::Rail {
        Some(app.navigator.rail)
    } else {
        None
    };
    let view = RailView {
        theme: &theme,
        badges: &badges,
        folders: &folders,
        selected: selected.as_deref(),
        focus,
    };
    let mut actions: Vec<RailAction> = Vec::new();

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
                        for (index, item) in items.iter().enumerate() {
                            let focused = view.focus == Some(index);
                            match item {
                                crate::rail::RailItem::Folder(folder) => {
                                    let open = expanded.contains(&folder.id);
                                    let active = selected
                                        .as_deref()
                                        .map(|id| folder.contains(id))
                                        .unwrap_or(false);
                                    let response = folder_icon(
                                        ui, &theme, folder, &guilds, open, active, focused,
                                    );

                                    if response.clicked() {
                                        actions.push(RailAction::ToggleFolder(folder.id.clone()));
                                    }
                                    response.context_menu(|ui| {
                                        if ui.button("Move up").clicked() {
                                            actions.push(RailAction::MoveFolder(
                                                folder.id.clone(),
                                                -1,
                                            ));
                                            ui.close();
                                        }
                                        if ui.button("Move down").clicked() {
                                            actions
                                                .push(RailAction::MoveFolder(folder.id.clone(), 1));
                                            ui.close();
                                        }
                                        ui.separator();
                                        if ui.button("Ungroup folder").clicked() {
                                            actions
                                                .push(RailAction::DeleteFolder(folder.id.clone()));
                                            ui.close();
                                        }
                                    });

                                    if open {
                                        for member in &folder.guilds {
                                            let Some(guild) =
                                                guilds.iter().find(|guild| &guild.id == member)
                                            else {
                                                continue;
                                            };
                                            let mut member_actions = Vec::new();
                                            ui.horizontal(|ui| {
                                                ui.add_space(6.0);
                                                guild_row(
                                                    ui,
                                                    &view,
                                                    guild,
                                                    true,
                                                    focused,
                                                    &mut member_actions,
                                                );
                                            });
                                            actions.extend(member_actions);
                                        }
                                    }
                                    ui.add_space(8.0);
                                }
                                crate::rail::RailItem::Guild(id) => {
                                    let Some(guild) = guilds.iter().find(|guild| &guild.id == id)
                                    else {
                                        continue;
                                    };
                                    guild_row(ui, &view, guild, false, focused, &mut actions);
                                }
                            }
                        }
                    });
                });
        });

    let mut layout_changed = false;
    let mut selected_guild: Option<String> = None;

    for action in actions {
        match action {
            RailAction::Select(id) => {
                app.navigator.stop();
                selected_guild = Some(id);
            }
            RailAction::ToggleFolder(id) => {
                if !app.rail_expanded.remove(&id) {
                    app.rail_expanded.insert(id);
                }
            }
            RailAction::MoveGuild(id, delta) => {
                layout_changed |= app.settings.guild_rail.move_guild(&app.guilds, &id, delta);
            }
            RailAction::MoveFolder(id, delta) => {
                layout_changed |= app.settings.guild_rail.move_folder(&app.guilds, &id, delta);
            }
            RailAction::NewFolder(id) => {
                let name = app
                    .guilds
                    .iter()
                    .find(|guild| guild.id == id)
                    .map(|guild| guild.name.clone())
                    .unwrap_or_else(|| "Folder".to_string());
                let folder = app.settings.guild_rail.new_folder(&id, &name);
                app.rail_expanded.insert(folder);
                app.status_line = Some(format!("created folder {name}"));
                layout_changed = true;
            }
            RailAction::AddToFolder(guild, folder) => {
                layout_changed |= app.settings.guild_rail.add_to_folder(&guild, &folder);
                app.rail_expanded.insert(folder);
            }
            RailAction::RemoveFromFolder(id) => {
                layout_changed |= app.settings.guild_rail.remove_from_folder(&id);
            }
            RailAction::DeleteFolder(id) => {
                layout_changed |= app.settings.guild_rail.delete_folder(&id);
                app.rail_expanded.remove(&id);
            }
        }
    }

    if layout_changed {
        app.settings_changed();
    }

    if let Some(id) = selected_guild {
        app.selected_guild = Some(id.clone());
        if let Some(first) = app
            .channels
            .iter()
            .find(|channel| channel.guild_id == id && channel.kind == ChannelKind::Text)
            .map(|channel| channel.id.clone())
        {
            app.select_channel(first);
        }
    }
}

fn guild_row(
    ui: &mut egui::Ui,
    view: &RailView<'_>,
    guild: &crate::models::Guild,
    in_folder: bool,
    focused: bool,
    actions: &mut Vec<RailAction>,
) {
    let theme = view.theme;
    let folders = view.folders;
    let selected = view.selected == Some(guild.id.as_str());

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
        actions.push(RailAction::Select(guild.id.clone()));
    }
    if focused {
        ui.painter().rect_stroke(
            response.rect.expand(1.0),
            egui::CornerRadius::same(radius),
            egui::Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }

    response.context_menu(|ui| {
        ui.set_min_width(200.0);
        if ui.button("Move up").clicked() {
            actions.push(RailAction::MoveGuild(guild.id.clone(), -1));
            ui.close();
        }
        if ui.button("Move down").clicked() {
            actions.push(RailAction::MoveGuild(guild.id.clone(), 1));
            ui.close();
        }

        ui.separator();
        if folders.is_empty() {
            ui.add_enabled(false, egui::Button::new("Add to folder"))
                .on_disabled_hover_text("Right-click a server and create a folder first");
        } else {
            ui.menu_button("Add to folder", |ui| {
                for (id, name) in folders {
                    if ui.button(name).clicked() {
                        actions.push(RailAction::AddToFolder(guild.id.clone(), id.clone()));
                        ui.close();
                    }
                }
            });
        }
        if ui.button("New folder with this server").clicked() {
            actions.push(RailAction::NewFolder(guild.id.clone()));
            ui.close();
        }
        if in_folder && ui.button("Remove from folder").clicked() {
            actions.push(RailAction::RemoveFromFolder(guild.id.clone()));
            ui.close();
        }
    });

    if let Some((mentions, unread)) = view.badges.get(&guild.id) {
        draw_badges(ui, response.rect, *mentions, *unread);
    }

    ui.add_space(8.0);
}

fn draw_badges(ui: &egui::Ui, rect: egui::Rect, mentions: u32, unread: bool) {
    if mentions > 0 {
        let label = if mentions > crate::models::UNREAD_BADGE_CAP {
            format!("{}+", crate::models::UNREAD_BADGE_CAP)
        } else {
            mentions.to_string()
        };
        let width = 14.0 + label.chars().count() as f32 * 4.0;
        let pill = egui::Rect::from_center_size(
            rect.right_bottom() - egui::vec2(2.0, 2.0),
            egui::vec2(width.max(16.0), 16.0),
        );
        ui.painter()
            .rect_filled(pill, 8.0, Color32::from_rgb(242, 63, 66));
        ui.painter().text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    } else if unread {
        ui.painter().circle_filled(
            rect.right_bottom() - egui::vec2(6.0, 6.0),
            4.0,
            Color32::WHITE,
        );
    }
}

/// A folder tile: a rounded square holding up to four member initials in a 2×2
/// grid, the way Discord draws them.
fn folder_icon(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    folder: &crate::rail::GuildFolder,
    guilds: &[crate::models::Guild],
    open: bool,
    active: bool,
    focused: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(GUILD_ICON, GUILD_ICON), egui::Sense::click());
    let fill = if open || active {
        theme.accent
    } else {
        theme.surface
    };
    ui.painter().rect_filled(rect, 16.0, fill);
    if focused {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            16.0,
            egui::Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }

    let members: Vec<&crate::models::Guild> = folder
        .guilds
        .iter()
        .filter_map(|id| guilds.iter().find(|guild| &guild.id == id))
        .take(4)
        .collect();

    let tile = 17.0;
    let gap = 3.0;
    let span = tile * 2.0 + gap;
    let start = rect.center() - egui::vec2(span / 2.0, span / 2.0);
    for (index, guild) in members.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            start
                + egui::vec2(
                    (index % 2) as f32 * (tile + gap),
                    (index / 2) as f32 * (tile + gap),
                ),
            egui::vec2(tile, tile),
        );
        ui.painter()
            .rect_filled(cell, 5.0, theme.surface_alt.gamma_multiply(1.6));
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            initial(&guild.name),
            egui::FontId::proportional(9.0),
            Color32::WHITE,
        );
    }

    let hint = format!(
        "{} · {} server(s) · click to {}",
        folder.name,
        folder.guilds.len(),
        if open { "collapse" } else { "expand" }
    );
    response.on_hover_text(hint)
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
                    let mut nav_index = 0usize;
                    category_label(ui, &theme, "TEXT CHANNELS");
                    for channel in app.channels.iter().filter(|channel| {
                        Some(&channel.guild_id) == app.selected_guild.as_ref()
                            && channel.kind == ChannelKind::Text
                    }) {
                        let selected = app.selected_channel.as_deref() == Some(channel.id.as_str());
                        let focused = app.navigator.on_channel(nav_index);
                        nav_index += 1;
                        if channel_row(
                            ui,
                            &theme,
                            selected,
                            "#",
                            &channel.name,
                            app.unread.get(&channel.id),
                            focused,
                        ) {
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
                        let focused = app.navigator.on_channel(nav_index);
                        nav_index += 1;
                        if channel_row(
                            ui,
                            &theme,
                            selected,
                            "🔊",
                            &channel.name,
                            app.unread.get(&channel.id),
                            focused,
                        ) {
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
        // A click takes over from the keyboard focus.
        app.navigator.stop();
        app.select_channel(id);
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
    unread: crate::models::Unread,
    focused: bool,
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

    if focused {
        ui.painter().rect_stroke(
            body,
            4.0,
            egui::Stroke::new(1.5, theme.accent),
            egui::StrokeKind::Inside,
        );
    }

    // Unread channels keep Discord's brighter label; a mention adds the red pill.
    let text_color = if selected || unread.is_unread() {
        theme.text
    } else {
        theme.muted
    };

    if unread.is_unread() && !selected {
        ui.painter()
            .circle_filled(rect.left_center() + egui::vec2(4.0, 0.0), 3.0, theme.text);
    }

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

    if let Some(label) = unread.badge() {
        let width = 16.0 + label.chars().count() as f32 * 4.0;
        let pill = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - ROW_INSET - width - 4.0,
                rect.center().y - 8.0,
            ),
            egui::vec2(width, 16.0),
        );
        ui.painter()
            .rect_filled(pill, 8.0, Color32::from_rgb(242, 63, 66));
        ui.painter().text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }

    response.clicked()
}
