//! Message-search overlay.
//!
//! A small floating panel rather than a page: search is something you do while
//! looking at a channel, not somewhere you navigate to. It states its own limits
//! (loaded history only) because Discord's guild search is a user-API endpoint
//! this client deliberately cannot call.

use crate::app::RustcordApp;
use egui::RichText;

pub fn show(app: &mut RustcordApp, ctx: &egui::Context) {
    if !app.search.open {
        return;
    }

    // The global visuals already carry the palette (D12), so the window uses the
    // default frame and only the text colours are set explicitly.
    let theme = app.plugins.theme().clone();
    let mut close = false;
    let mut jump: Option<String> = None;
    // Snapshots, so the message list can change under us without a borrow fight.
    let results = app.search.results.clone();
    let held = app.messages.len();
    let query_empty = app.search.query.trim().is_empty();

    egui::Window::new("Search")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .fixed_size(egui::vec2(560.0, 400.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Search").small().color(theme.muted));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.search.query)
                        .hint_text("loaded messages — from:name narrows to an author")
                        .desired_width(ui.available_width()),
                );

                if app.search.just_opened {
                    response.request_focus();
                    app.search.just_opened = false;
                }
                if response.changed() {
                    app.refresh_search();
                }
            });

            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} result(s) · {held} message(s) held in this channel",
                    results.len()
                ))
                .small()
                .color(theme.muted),
            );
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(288.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if results.is_empty() {
                        ui.label(
                            RichText::new(if query_empty {
                                "Type to search the messages loaded here."
                            } else {
                                "Nothing in the loaded history matched."
                            })
                            .small()
                            .color(theme.muted),
                        );
                    }

                    for hit in &results {
                        let label = format!(
                            "{}  ·  {}\n{}",
                            hit.author,
                            hit.timestamp.format("%d %b %H:%M"),
                            hit.snippet
                        );
                        let row = ui.add(
                            egui::Label::new(RichText::new(label).small().color(theme.text))
                                .sense(egui::Sense::click()),
                        );
                        if row.on_hover_text("Open that channel").clicked() {
                            jump = hit.channel_id.clone();
                        }
                        ui.separator();
                    }
                });

            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Discord's server-side search needs a user token, so this searches only \
                     messages already loaded in Rustcord.",
                )
                .small()
                .color(theme.muted),
            );
        });

    // Outside the window too, so Escape works after clicking a result.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if let Some(channel_id) = jump {
        app.jump_to_channel(&channel_id);
    }
    if close {
        app.close_search();
    }
}
