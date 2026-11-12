//! Message-search overlay.
//!
//! A small floating panel rather than a page: search is something you do while
//! looking at a channel, not somewhere you navigate to. It searches everything
//! cached this session across channels, with `from:` and `in:#channel` filters,
//! and states its own limits because Discord's guild search is a user-API
//! endpoint this client deliberately cannot call.

use crate::app::RustcordApp;
use crate::ui::theme::AppTheme;
use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, RichText};

/// A result row: a muted header line, then the snippet with every matched term
/// tinted. Built as a `LayoutJob` so the highlight costs no extra widgets.
fn result_job(header: &str, snippet: &str, terms: &[String], theme: &AppTheme) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        header,
        0.0,
        TextFormat {
            font_id: FontId::proportional(11.0),
            color: theme.muted,
            ..Default::default()
        },
    );
    job.append(
        "\n",
        0.0,
        TextFormat {
            font_id: FontId::proportional(12.0),
            color: theme.muted,
            ..Default::default()
        },
    );

    let base = TextFormat {
        font_id: FontId::proportional(12.5),
        color: theme.text,
        ..Default::default()
    };
    let highlight = TextFormat {
        font_id: FontId::proportional(12.5),
        color: theme.text,
        background: theme.accent.gamma_multiply(0.35),
        ..Default::default()
    };

    let mut cursor = 0;
    for (start, end) in crate::search::highlight_ranges(snippet, terms) {
        if start > cursor {
            job.append(&snippet[cursor..start], 0.0, base.clone());
        }
        job.append(&snippet[start..end], 0.0, highlight.clone());
        cursor = end;
    }
    if cursor < snippet.len() {
        job.append(&snippet[cursor..], 0.0, base);
    }
    job
}

pub fn show(app: &mut RustcordApp, ctx: &egui::Context) {
    if !app.search.open {
        return;
    }

    // The global visuals already carry the palette (D12), so the window uses the
    // default frame and only the text colours are set explicitly.
    let theme = app.plugins.theme().clone();
    let mut close = false;
    let mut jump: Option<(String, String)> = None;
    // Snapshots, so the message list can change under us without a borrow fight.
    let results = app.search.results.clone();
    let held = app.messages.len();
    let channels = app
        .messages
        .iter()
        .filter_map(|message| message.channel_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let query_empty = app.search.query.trim().is_empty();
    // Snapshot of the parsed filters, so the overlay shows what the syntax did.
    let parsed = crate::search::SearchQuery::parse(&app.search.query);
    let described = parsed.describe();
    // Snapshot of the matching terms for the row highlights.
    let terms = parsed.terms.clone();

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
                        .hint_text("cached messages — from:name · in:#channel · before:/after:")
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
                    "{} result(s) · {held} message(s) cached this session across {channels} channel(s)",
                    results.len()
                ))
                .small()
                .color(theme.muted),
            );
            if !described.is_empty() {
                ui.label(
                    RichText::new(format!("matching {described}"))
                        .small()
                        .color(theme.accent),
                );
            }
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
                        let source = hit
                            .channel
                            .as_deref()
                            .map(|name| format!("#{name}  ·  "))
                            .unwrap_or_default();
                        let header = format!(
                            "{source}{}  ·  {}",
                            hit.author,
                            hit.timestamp.format("%d %b %H:%M")
                        );
                        let job = result_job(&header, &hit.snippet, &terms, &theme);
                        let row = ui.add(egui::Label::new(job).sense(egui::Sense::click()));
                        if row
                            .on_hover_text("Open that message in its channel")
                            .clicked()
                        {
                            jump = hit
                                .channel_id
                                .clone()
                                .map(|channel| (channel, hit.message_id.clone()));
                        }
                        ui.separator();
                    }
                });

            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Discord's server-side search needs a user token, so this searches only \
                     messages cached in Rustcord this session. Filters: from:name, in:#channel, \
                     before:YYYY-MM-DD, after:YYYY-MM-DD (exclusive, UTC). Click a result to \
                     open that message.",
                )
                .small()
                .color(theme.muted),
            );
        });

    // Outside the window too, so Escape works after clicking a result.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if let Some((channel_id, message_id)) = jump {
        app.jump_to_message(&channel_id, &message_id);
    }
    if close {
        app.close_search();
    }
}
