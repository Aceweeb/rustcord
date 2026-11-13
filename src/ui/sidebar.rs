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
    RenameFolder(String),
    SetFolderColor(String, Option<String>),
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
    let mut hover: Option<(String, egui::Rect)> = None;

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
                                        ui.menu_button("Folder colour", |ui| {
                                            for (name, hex) in crate::rail::FOLDER_COLORS {
                                                let swatch = crate::utils::parse_css_color(hex)
                                                    .unwrap_or(Color32::WHITE);
                                                if ui
                                                    .button(
                                                        RichText::new(format!("● {name}"))
                                                            .color(swatch),
                                                    )
                                                    .clicked()
                                                {
                                                    actions.push(RailAction::SetFolderColor(
                                                        folder.id.clone(),
                                                        Some(hex.to_string()),
                                                    ));
                                                    ui.close();
                                                }
                                            }
                                            ui.separator();
                                            if ui.button("No colour").clicked() {
                                                actions.push(RailAction::SetFolderColor(
                                                    folder.id.clone(),
                                                    None,
                                                ));
                                                ui.close();
                                            }
                                        });
                                        if ui.button("Rename folder…").clicked() {
                                            actions
                                                .push(RailAction::RenameFolder(folder.id.clone()));
                                            ui.close();
                                        }
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
                                            let tint = folder
                                                .color
                                                .as_deref()
                                                .and_then(crate::utils::parse_css_color);
                                            ui.horizontal(|ui| {
                                                ui.add_space(6.0);
                                                if let Some(rect) = guild_row(
                                                    ui,
                                                    &view,
                                                    guild,
                                                    true,
                                                    tint,
                                                    focused,
                                                    &mut member_actions,
                                                ) {
                                                    hover = Some((guild.id.clone(), rect));
                                                }
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
                                    if let Some(rect) = guild_row(
                                        ui,
                                        &view,
                                        guild,
                                        false,
                                        None,
                                        focused,
                                        &mut actions,
                                    ) {
                                        hover = Some((guild.id.clone(), rect));
                                    }
                                }
                            }
                        }
                    });
                });
        });

    app.rail_hover = hover;

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
            RailAction::SetFolderColor(id, colour) => {
                layout_changed |= app
                    .settings
                    .guild_rail
                    .set_folder_color(&id, colour.as_deref());
            }
            RailAction::RenameFolder(id) => {
                if let Some(folder) = app
                    .settings
                    .guild_rail
                    .folders
                    .iter()
                    .find(|folder| folder.id == id)
                {
                    app.rail_rename = Some(crate::rail::FolderRename::new(folder));
                }
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
    tint: Option<Color32>,
    focused: bool,
    actions: &mut Vec<RailAction>,
) -> Option<egui::Rect> {
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
    // A server grouped in a folder carries that folder's colour, so an expanded
    // folder reads as one block instead of a stack of unrelated circles. The tint
    // is a blend with the surface, which keeps white initials legible; the ring
    // is the folder's own colour so the group survives selection too.
    let fill = if selected {
        theme.accent
    } else {
        tint.map_or(theme.surface, |colour| {
            crate::utils::mix(theme.surface, colour, 0.55)
        })
    };
    let response = ui.add(
        egui::Button::new(
            RichText::new(if guild.id == crate::models::DM_GUILD_ID {
                "@".to_string()
            } else {
                initial(&guild.name)
            })
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
    if let Some(colour) = tint {
        ui.painter().rect_stroke(
            response.rect.expand(1.0),
            egui::CornerRadius::same(radius),
            egui::Stroke::new(1.5, colour),
            egui::StrokeKind::Inside,
        );
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

    // The caller turns a hovered icon into the channel peek, which has to be
    // drawn outside this 72 px panel.
    response.hovered().then_some(response.rect)
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
    // The folder's own colour is its identity in the rail; expansion and the
    // selected member are shown with a ring instead of repainting it.
    let fill = folder
        .color
        .as_deref()
        .and_then(crate::utils::parse_css_color)
        .unwrap_or(theme.surface);
    ui.painter().rect_filled(rect, 16.0, fill);
    if open || active {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            16.0,
            egui::Stroke::new(if active { 2.0 } else { 1.5 }, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
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
        // The mini tiles inside a closed folder use the same tint, so the folder
        // itself previews the group colour.
        let cell_fill = crate::utils::mix(theme.surface_alt.gamma_multiply(1.6), fill, 0.35);
        ui.painter().rect_filled(cell, 5.0, cell_fill);
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
                    let is_dm_guild =
                        app.selected_guild.as_deref() == Some(crate::models::DM_GUILD_ID);
                    category_label(
                        ui,
                        &theme,
                        if is_dm_guild {
                            "DIRECT MESSAGES"
                        } else {
                            "TEXT CHANNELS"
                        },
                    );
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
                            if is_dm_guild { "@" } else { "#" },
                            &channel.name,
                            app.unread.get(&channel.id),
                            focused,
                        ) {
                            clicked = Some(channel.id.clone());
                        }
                    }

                    if !is_dm_guild {
                        ui.add_space(14.0);
                        category_label(ui, &theme, "VOICE CHANNELS");
                        for channel in app.channels.iter().filter(|channel| {
                            Some(&channel.guild_id) == app.selected_guild.as_ref()
                                && channel.kind == ChannelKind::Voice
                        }) {
                            let selected =
                                app.selected_channel.as_deref() == Some(channel.id.as_str());
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
                    }
                    if is_dm_guild
                        && !app
                            .channels
                            .iter()
                            .any(|channel| channel.guild_id == crate::models::DM_GUILD_ID)
                    {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No direct messages yet — open one with DM on a member.")
                                .size(11.0)
                                .color(theme.muted),
                        );
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
        // The signed-in account's avatar (or its initial while loading), with the
        // same profile card the message rows use.
        if let Some(url) = app.current_user.avatar_url() {
            app.images.prepare(&[url]);
        }
        ui.horizontal(|ui| {
            let _ = crate::ui::chat::avatar(app, ui, &app.current_user, 32.0);

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

        if let Some(activity) = app.plugins.spotify_activity().cloned() {
            let playing = app.plugins.spotify_playing();
            // Album art is fetched through the same bounded cache as attachments;
            // prepare deduplicates, so a paused track is not re-requested.
            let art = activity.large_image.clone();
            if let Some(url) = &art {
                app.images.prepare(std::slice::from_ref(url));
            }

            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                match art.as_deref().and_then(|url| app.images.peek(url)) {
                    Some(texture) => {
                        ui.painter().image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(4),
                            crate::utils::lift(theme.surface_alt, 12),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "♪",
                            egui::FontId::proportional(16.0),
                            theme.muted,
                        );
                    }
                }

                ui.vertical(|ui| {
                    let artist = activity.details.as_deref().unwrap_or("unknown artist");
                    let glyph = if playing { "🎵" } else { "⏸" };
                    ui.label(
                        RichText::new(format!("{glyph} {} — {}", activity.name, artist))
                            .size(12.0)
                            .color(if playing { theme.text } else { theme.muted }),
                    )
                    .on_hover_text(if playing {
                        "Playing on Spotify"
                    } else {
                        "Paused on Spotify"
                    });

                    let duration = app.plugins.spotify_duration_ms();
                    if duration > 0 {
                        let progress = app.plugins.spotify_progress_ms() as f32 / duration as f32;
                        let fill = if playing {
                            theme.accent
                        } else {
                            theme.muted.gamma_multiply(0.6)
                        };
                        ui.add(
                            egui::ProgressBar::new(progress.clamp(0.0, 1.0))
                                .desired_width(150.0)
                                .fill(fill),
                        );
                        let clock = format!(
                            "{} / {}",
                            format_duration(app.plugins.spotify_progress_ms()),
                            format_duration(duration)
                        );
                        ui.label(
                            RichText::new(if playing {
                                clock
                            } else {
                                format!("{clock} · paused")
                            })
                            .size(10.0)
                            .color(theme.muted),
                        );
                    }
                });
            });
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

/// Rename dialog for a folder.
///
/// The rail is 72 px wide, so a name cannot be edited in place there; a small
/// dialog is the honest place for a text field. The draft lives in the app only
/// until it is saved, and a blank name keeps the dialog open with a reason.
pub(crate) fn rename_dialog(app: &mut RustcordApp, ctx: &egui::Context) {
    let Some(mut state) = app.rail_rename.clone() else {
        return;
    };
    let theme = app.plugins.theme().clone();
    let mut save = false;
    let mut cancel = false;

    egui::Window::new("Rename folder")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .fixed_size(egui::vec2(360.0, 132.0))
        .show(ctx, |ui| {
            ui.label(
                RichText::new("Rename folder")
                    .size(15.0)
                    .strong()
                    .color(theme.text),
            );
            ui.add_space(4.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut state.draft)
                    .hint_text("Folder name")
                    .desired_width(ui.available_width()),
            );
            if state.focus {
                response.request_focus();
                state.focus = false;
            }

            ui.add_space(6.0);
            let valid = !state.draft.trim().is_empty();
            ui.horizontal(|ui| {
                if ui.add_enabled(valid, egui::Button::new("Save")).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.label(
                    RichText::new("Enter saves · Esc cancels")
                        .small()
                        .color(theme.muted),
                );
            });
            if !valid {
                ui.label(
                    RichText::new("A folder needs a name.")
                        .small()
                        .color(theme.muted),
                );
            }

            if ui.input(|input| input.key_pressed(egui::Key::Enter)) && valid {
                save = true;
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });

    app.rail_rename = Some(state);

    if save {
        if let Some(state) = app.rail_rename.take() {
            let name = state.draft.trim().to_string();
            if app
                .settings
                .guild_rail
                .rename_folder(&state.folder_id, &name)
            {
                app.settings_changed();
                app.status_line = Some(format!("folder renamed to {name}"));
            }
        }
    } else if cancel {
        app.rail_rename = None;
    }
}

/// The peek as it was drawn last frame.
///
/// The flyout sits beside the icon and overlaps the channel list, so the pointer
/// necessarily leaves the icon to reach it. Keeping the last icon and flyout
/// rects lets the peek survive that trip (and the small gap between the two).
#[derive(Debug, Clone)]
pub struct Peek {
    pub guild_id: String,
    pub icon: egui::Rect,
    pub area: egui::Rect,
}

/// Extra slop around the flyout so crossing the gap between icon and panel does
/// not close it.
const PEEK_REACH: f32 = 14.0;

/// Discord's rail peek: hover a server icon and see its channels without
/// switching to it. The rows are clickable, so the peek is also a faster way to
/// reach a channel than switching servers first.
///
/// It has to be drawn outside the 72 px rail panel, so it is an `Area` rather than
/// a widget inside the rail, and it uses the foreground layer so its rows can
/// receive clicks (`DECISIONS.md` D37).
pub(crate) fn hover_peek(app: &mut RustcordApp, ctx: &egui::Context) {
    // Other modes already own the screen or the keyboard.
    if app.navigator.active
        || app.search.open
        || app.switcher.open
        || app.show_settings
        || app.rail_rename.is_some()
        || app.message_log_open
    {
        app.rail_peek = None;
        return;
    }

    let hover = app
        .rail_hover
        .as_ref()
        .map(|(id, rect)| (id.as_str(), *rect));
    let candidate = peek_target(hover, app.rail_peek.as_ref(), ctx.pointer_hover_pos());

    let Some((guild_id, icon)) = candidate else {
        app.rail_peek = None;
        return;
    };

    let Some(guild) = app
        .guilds
        .iter()
        .find(|guild| guild.id == guild_id)
        .cloned()
    else {
        app.rail_peek = None;
        return;
    };
    let theme = app.plugins.theme().clone();

    const MAX_TEXT: usize = 8;
    const MAX_VOICE: usize = 5;

    let text_channels: Vec<(String, String, crate::models::Unread)> = app
        .channels
        .iter()
        .filter(|channel| channel.guild_id == guild_id && channel.kind == ChannelKind::Text)
        .map(|channel| {
            (
                channel.id.clone(),
                channel.name.clone(),
                app.unread.get(&channel.id),
            )
        })
        .collect();
    let voice_channels: Vec<(String, String)> = app
        .channels
        .iter()
        .filter(|channel| channel.guild_id == guild_id && channel.kind == ChannelKind::Voice)
        .map(|channel| (channel.id.clone(), channel.name.clone()))
        .collect();

    // Keep the panel on screen when the icon is near the bottom of the window.
    let max_top = (ctx.content_rect().bottom() - 240.0).max(8.0);
    let position = egui::pos2(icon.right() + 10.0, icon.top().min(max_top));
    let mut clicked: Option<String> = None;

    let area = egui::Area::new(egui::Id::unique("rail_channel_peek"))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme.surface)
                .stroke(egui::Stroke::new(1.0, lift(theme.surface, 18)))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_min_width(188.0);
                    ui.set_max_width(230.0);

                    ui.label(
                        RichText::new(&guild.name)
                            .size(14.0)
                            .strong()
                            .color(theme.text),
                    );
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("TEXT CHANNELS")
                            .size(10.0)
                            .strong()
                            .color(theme.muted),
                    );
                    if text_channels.is_empty() {
                        ui.label(RichText::new("none").small().color(theme.muted));
                    }
                    for (id, name, unread) in text_channels.iter().take(MAX_TEXT) {
                        let color = if unread.is_unread() {
                            theme.text
                        } else {
                            theme.muted
                        };
                        let mut label = format!("#{name}");
                        if let Some(badge) = unread.badge() {
                            label.push_str(&format!("   {badge}"));
                        }
                        let row = ui
                            .add(
                                egui::Label::new(RichText::new(label).size(13.0).color(color))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Open this channel");
                        if row.clicked() {
                            clicked = Some(id.clone());
                        }
                    }
                    if text_channels.len() > MAX_TEXT {
                        ui.label(
                            RichText::new(format!("and {} more", text_channels.len() - MAX_TEXT))
                                .size(11.0)
                                .color(theme.muted),
                        );
                    }

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("VOICE CHANNELS")
                            .size(10.0)
                            .strong()
                            .color(theme.muted),
                    );
                    if voice_channels.is_empty() {
                        ui.label(RichText::new("none").small().color(theme.muted));
                    }
                    for (id, name) in voice_channels.iter().take(MAX_VOICE) {
                        let row = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(format!("\u{1F50A} {name}"))
                                        .size(13.0)
                                        .color(theme.muted),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Open this channel");
                        if row.clicked() {
                            clicked = Some(id.clone());
                        }
                    }
                    if voice_channels.len() > MAX_VOICE {
                        ui.label(
                            RichText::new(format!("and {} more", voice_channels.len() - MAX_VOICE))
                                .size(11.0)
                                .color(theme.muted),
                        );
                    }
                });
        })
        .response
        .rect;

    app.rail_peek = Some(Peek {
        guild_id: guild_id.clone(),
        icon,
        area,
    });

    if let Some(channel_id) = clicked {
        // Same path as clicking the channel in the sidebar: selects the server,
        // opens the channel and marks it read (D30).
        app.navigator.stop();
        app.selected_guild = Some(guild_id);
        app.select_channel(channel_id);
    }
}

/// Which server the peek should show, and where its icon is.
///
/// An icon under the pointer always wins; otherwise the flyout stays open only
/// while the pointer is inside it (plus a little slop for the gap), which is what
/// makes its rows clickable at all: reaching them means leaving the icon.
fn peek_target(
    hover: Option<(&str, egui::Rect)>,
    last: Option<&Peek>,
    pointer: Option<egui::Pos2>,
) -> Option<(String, egui::Rect)> {
    if let Some((id, rect)) = hover {
        return Some((id.to_string(), rect));
    }
    let (last, pointer) = (last?, pointer?);
    if last
        .area
        .expand2(egui::vec2(PEEK_REACH, 4.0))
        .contains(pointer)
    {
        Some((last.guild_id.clone(), last.icon))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(48.0, 48.0))
    }

    fn peek() -> Peek {
        Peek {
            guild_id: "7".to_string(),
            icon: rect(12.0, 100.0),
            area: rect(80.0, 100.0),
        }
    }

    #[test]
    fn a_hovered_icon_always_wins() {
        let last = peek();
        let target = peek_target(
            Some(("9", rect(12.0, 200.0))),
            Some(&last),
            Some(egui::pos2(90.0, 110.0)),
        );
        assert_eq!(target.expect("target").0, "9");
    }

    #[test]
    fn the_peek_stays_open_while_the_pointer_is_on_it() {
        let last = peek();
        let target = peek_target(None, Some(&last), Some(egui::pos2(120.0, 120.0)));
        let (id, icon) = target.expect("kept open");
        assert_eq!(id, "7");
        assert_eq!(icon, last.icon, "the flyout stays anchored to its icon");

        // The gap between icon and flyout is part of the reach.
        assert!(peek_target(None, Some(&last), Some(egui::pos2(78.0, 120.0))).is_some());
    }

    #[test]
    fn leaving_the_peek_closes_it() {
        let last = peek();
        assert!(peek_target(None, Some(&last), Some(egui::pos2(300.0, 300.0))).is_none());
        assert!(peek_target(None, Some(&last), None).is_none());
        assert!(peek_target(None, None, Some(egui::pos2(90.0, 110.0))).is_none());
    }
}
