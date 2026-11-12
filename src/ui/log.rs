//! Message log panel: what the gateway reported as edited or deleted while this
//! session has been running.
//!
//! It states its own limits in the empty state, because a log that looks empty
//! could otherwise be mistaken for "nothing was deleted": entries only exist for
//! messages this client actually saw, and they are gone when the app closes
//! (`DECISIONS.md` D36).

use crate::app::RustcordApp;
use crate::message_log::LogKind;
use egui::{Color32, RichText};

pub fn show(app: &mut RustcordApp, ctx: &egui::Context) {
    if !app.message_log_open {
        return;
    }

    let theme = app.plugins.theme().clone();
    let mut close = false;
    let mut clear = false;

    // Snapshot for the frame: the panel never holds a borrow of the app.
    let names: std::collections::BTreeMap<String, String> = app
        .channels
        .iter()
        .map(|channel| (channel.id.clone(), channel.name.clone()))
        .collect();
    let entries: Vec<crate::message_log::LogEntry> = app
        .message_log
        .filter(&app.log_filter)
        .into_iter()
        .cloned()
        .collect();
    let total = app.message_log.len();

    egui::Window::new("Message log")
        .title_bar(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .default_size(egui::vec2(640.0, 420.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Message log")
                        .size(15.0)
                        .strong()
                        .color(theme.text),
                );
                ui.label(
                    RichText::new(format!("{total} entr(y/ies) this session"))
                        .small()
                        .color(theme.muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if ui
                        .add_enabled(total > 0, egui::Button::new("Clear"))
                        .clicked()
                    {
                        clear = true;
                    }
                });
            });

            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.log_filter)
                    .hint_text("filter by author, channel or text")
                    .desired_width(f32::INFINITY),
            );

            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if entries.is_empty() {
                        ui.label(
                            RichText::new(if total == 0 {
                                "Nothing logged yet. Edits and deletions the gateway reports \
                                 appear here while Rustcord runs; nothing is written to disk, \
                                 and messages deleted before this session leave nothing to log."
                            } else {
                                "No entries match that filter."
                            })
                            .small()
                            .color(theme.muted),
                        );
                    }

                    for entry in &entries {
                        let channel = names
                            .get(&entry.channel_id)
                            .map(|name| format!("#{name}"))
                            .unwrap_or_else(|| format!("#{}", entry.channel_id));
                        let colour = match entry.kind {
                            LogKind::Deleted => Color32::from_rgb(242, 63, 66),
                            LogKind::Edited => theme.accent,
                        };

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(entry.kind.label())
                                    .small()
                                    .strong()
                                    .color(colour),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} · {}",
                                    entry.author,
                                    channel,
                                    entry.at.format("%d %b %H:%M:%S")
                                ))
                                .small()
                                .color(theme.muted),
                            );
                        });

                        ui.label(RichText::new(&entry.before).size(13.0).color(theme.muted));
                        if let Some(after) = &entry.after {
                            ui.label(
                                RichText::new(format!("\u{2192} {after}"))
                                    .size(13.0)
                                    .color(theme.text),
                            );
                        }
                        ui.add_space(6.0);
                    }
                });

            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });

    if clear {
        app.message_log.clear();
        app.log_filter.clear();
    }
    if close {
        app.message_log_open = false;
        app.log_filter.clear();
    }
}
