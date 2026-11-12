//! Ctrl+K overlay: type, arrow, Enter.
//!
//! Entries are rebuilt from the app's guilds and channels each frame, which is
//! cheap (a few thousand at most) and always current — no index to invalidate
//! when a guild arrives from READY.

use crate::app::RustcordApp;
use crate::switcher::{self, EntryKind};
use egui::RichText;

pub fn show(app: &mut RustcordApp, ctx: &egui::Context) {
    if !app.switcher.open {
        return;
    }

    let theme = app.plugins.theme().clone();
    let entries = switcher::build_entries(&app.guilds, &app.channels);
    let results: Vec<switcher::Entry> = switcher::filter(&entries, &app.switcher.query)
        .into_iter()
        .cloned()
        .collect();
    app.switcher.clamp(results.len());

    let mut activate: Option<usize> = None;
    let mut close = false;

    egui::Window::new("Quick switcher")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .fixed_size(egui::vec2(560.0, 320.0))
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.switcher.query)
                    .hint_text("Jump to a server or channel  (Ctrl+K)")
                    .desired_width(ui.available_width()),
            );

            if app.switcher.just_opened {
                response.request_focus();
                app.switcher.just_opened = false;
            }
            if response.changed() {
                app.switcher.selected = 0;
            }

            // Keys are read from the raw input so they work while the field has
            // focus and would otherwise swallow arrows.
            if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                app.switcher.move_by(1, results.len());
            }
            if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                app.switcher.move_by(-1, results.len());
            }
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                activate = Some(app.switcher.selected);
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                close = true;
            }

            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} match(es) · ↑↓ to move · Enter to jump · Esc to close",
                    results.len()
                ))
                .small()
                .color(theme.muted),
            );
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(216.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if results.is_empty() {
                        ui.label(
                            RichText::new("No server or channel matches that.")
                                .small()
                                .color(theme.muted),
                        );
                    }

                    for (index, entry) in results.iter().enumerate() {
                        let glyph = match entry.kind {
                            EntryKind::Guild => "server",
                            EntryKind::Channel => "#",
                        };
                        let label = format!("{glyph}  {}   ·   {}", entry.primary, entry.secondary);
                        let selected = index == app.switcher.selected;

                        let row = ui.add(
                            egui::Label::new(RichText::new(label).size(14.0).color(if selected {
                                theme.text
                            } else {
                                theme.muted
                            }))
                            .sense(egui::Sense::click()),
                        );

                        if selected {
                            ui.painter().rect_stroke(
                                row.rect.expand(2.0),
                                4.0,
                                egui::Stroke::new(1.0, theme.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if row.clicked() {
                            activate = Some(index);
                        }
                    }
                });
        });

    if let Some(index) = activate {
        if let Some(entry) = results.get(index) {
            app.activate_switcher(entry.clone());
        }
    }
    if close {
        app.close_switcher();
    }
}
