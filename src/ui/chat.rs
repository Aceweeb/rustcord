use crate::app::RustcordApp;
use crate::models::{Embed, Message};
use crate::utils::initial;
use egui::RichText;

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    // Discord's 48 px channel header: a muted #, the name, then the icons.
    egui::Panel::top("chat_top").show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let channel = app
                .selected_channel_name()
                .map(str::to_string)
                .unwrap_or_else(|| "No channel selected".to_string());
            ui.label(
                RichText::new("#")
                    .size(20.0)
                    .color(app.plugins.theme().muted),
            );
            ui.label(
                RichText::new(channel)
                    .size(16.0)
                    .strong()
                    .color(app.plugins.theme().text),
            );

            let streaming = app.plugins.streaming_state();
            if streaming.active {
                ui.separator();
                ui.label(
                    RichText::new("● LIVE")
                        .strong()
                        .color(app.plugins.theme().danger),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {} viewers",
                        streaming.title, streaming.viewer_count
                    ))
                    .small()
                    .color(app.plugins.theme().muted),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("🔍")
                        .size(16.0)
                        .color(app.plugins.theme().muted),
                );
                ui.label(
                    RichText::new("☰")
                        .size(16.0)
                        .color(app.plugins.theme().muted),
                );
            });
        });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.max_rect().bottom(),
            egui::Stroke::new(1.0, crate::utils::lift(app.plugins.theme().background, -10)),
        );
    });

    // In-app alert banner (memory budget, test alerts). Dismissal mutates app
    // state, so it is applied after the panel closure returns.
    if let Some(alert) = app.alert.clone() {
        let mut dismissed = false;
        egui::Panel::top("alert_banner").show(ui, |ui| {
            ui.horizontal(|ui| {
                let color = match alert.kind {
                    crate::app::AlertKind::Warn => app.plugins.theme().danger,
                    crate::app::AlertKind::Info => app.plugins.theme().accent,
                };
                ui.label(RichText::new(&alert.text).strong().color(color));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Dismiss").clicked() {
                        dismissed = true;
                    }
                });
            });
        });
        if dismissed {
            app.dismiss_alert();
        }
    }

    egui::Panel::right("member_list")
        .exact_size(240.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(app.plugins.theme().surface)
                .inner_margin(egui::Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            ui.add_space(12.0);
            let online = app
                .messages
                .iter()
                .map(|message| message.author.username.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            ui.label(
                RichText::new(format!("ONLINE — {online}"))
                    .size(11.0)
                    .strong()
                    .color(app.plugins.theme().muted),
            );
            ui.add_space(6.0);

            let mut seen = std::collections::BTreeSet::new();
            for message in app.messages.iter().rev() {
                if seen.insert(message.author.username.clone()) {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            16.0,
                            crate::utils::lift(app.plugins.theme().surface, 18),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            initial(&message.author.username),
                            egui::FontId::proportional(12.0),
                            app.plugins.theme().text,
                        );
                        ui.label(
                            RichText::new(&message.author.username)
                                .size(14.0)
                                .color(app.plugins.theme().text),
                        );
                    });
                }
            }
        });

    let mut status = app.gateway_status.clone();
    if let Some(line) = &app.status_line {
        status.push_str(" · ");
        status.push_str(line);
    }

    // Live measurement, not an estimate: RSS/CPU come from /proc.
    if let Some(sample) = app.monitor.sample {
        status.push_str(" · ");
        status.push_str(&crate::sysinfo::human_bytes(sample.rss_bytes));
        if let Some(cpu) = app.monitor.cpu_percent {
            status.push_str(&format!(" · {cpu:.1}% CPU"));
        }
        status.push_str(&format!(" · {} thr", sample.threads));
    }
    let (image_bytes, _, image_count, _) = app.images.stats();
    if image_count > 0 {
        status.push_str(&format!(
            " · {} in {image_count} img",
            crate::sysinfo::human_bytes(image_bytes as u64)
        ));
    }
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.label(
            RichText::new(status)
                .small()
                .color(app.plugins.theme().muted),
        );
    });

    // Typing is tracked per channel and expires by itself, so this only reads
    // state; while somebody types, a bounded repaint keeps the line honest.
    let typing_now = ui.input(|input| input.time);
    let typing = app
        .selected_channel
        .as_deref()
        .and_then(|channel| app.typing.summary(channel, typing_now));
    if typing.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }

    // A delete is confirmed rather than done on the click that asked for it.
    if let Some(id) = app.pending_delete.clone() {
        egui::Panel::top("delete_confirm").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Delete this message?")
                        .size(13.0)
                        .color(app.plugins.theme().text),
                );
                if ui.button("Delete").clicked() {
                    app.delete_message(id.clone());
                }
                if ui.button("Cancel").clicked() {
                    app.pending_delete = None;
                }
            });
        });
    }

    // Discord's composer: a lifted, rounded field spanning the channel area.
    egui::Panel::bottom("composer").show(ui, |ui| {
        ui.add_space(6.0);

        if app.editing.is_some() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Editing message")
                        .size(12.0)
                        .strong()
                        .color(app.plugins.theme().accent),
                );
                ui.label(
                    RichText::new("Enter saves · Esc cancels")
                        .size(11.0)
                        .color(app.plugins.theme().muted),
                );
            });
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                app.cancel_edit();
            }
        }

        if let Some(typing) = &typing {
            ui.label(
                RichText::new(typing)
                    .size(12.0)
                    .italics()
                    .color(app.plugins.theme().muted),
            );
            ui.add_space(2.0);
        }

        let theme = app.plugins.theme().clone();
        let hint = app
            .selected_channel_name()
            .map(|name| format!("Message #{name}"))
            .unwrap_or_else(|| "Select a channel".to_string());

        egui::Frame::new()
            .fill(crate::utils::lift(theme.background, 12))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                let response = ui.add_sized(
                    egui::vec2(ui.available_width(), 22.0),
                    egui::TextEdit::singleline(&mut app.draft)
                        .hint_text(hint)
                        .frame(egui::Frame::new())
                        .text_color(theme.text),
                );

                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    app.send_current_draft();
                    response.request_focus();
                }
            });
        ui.add_space(8.0);
    });

    egui::CentralPanel::default().show(ui, |ui| {
        let channel_id = app.selected_channel.clone();
        let visible: Vec<usize> = app
            .messages
            .iter()
            .enumerate()
            .filter(|(_, message)| message.channel_id == channel_id)
            .map(|(index, _)| index)
            .collect();

        // Heights measured for a different channel would be wrong for a frame.
        if app.metrics_channel != channel_id {
            app.metrics.clear();
            app.metrics_channel = channel_id.clone();
        }

        if visible.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No messages in this channel yet.")
                        .color(app.plugins.theme().muted),
                );
            });
            return;
        }

        // Pagination controls for live channels only; demo data has no cursor.
        if app.live_data {
            if let Some(state) = channel_id
                .as_deref()
                .and_then(|id| app.channel_history(id))
                .cloned()
            {
                ui.horizontal(|ui| {
                    if state.loading {
                        ui.label(
                            RichText::new("loading messages…")
                                .small()
                                .color(app.plugins.theme().muted),
                        );
                    } else {
                        if !state.exhausted && ui.button("Load older messages").clicked() {
                            app.load_older();
                        }
                        if state.exhausted {
                            ui.label(
                                RichText::new("start of channel")
                                    .small()
                                    .color(app.plugins.theme().muted),
                            );
                        }
                    }
                    if let Some(error) = &state.error {
                        ui.label(
                            RichText::new(error)
                                .small()
                                .color(app.plugins.theme().danger),
                        );
                    }
                });
                ui.separator();
            }
        }

        let gap = if app.settings.compact_mode { 4.0 } else { 10.0 };

        // Take the metrics out so the closure can borrow `app` immutably while
        // recording measurements into a local.
        let mut metrics = std::mem::take(&mut app.metrics);
        metrics.ensure_len(visible.len());

        // Image URLs seen while drawing. Filled inside the viewport closure and
        // acted on afterwards, because requesting a download needs `&mut app`.
        let mut wanted: Vec<String> = Vec::new();
        let mut row_action: Option<RowAction> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show_viewport(ui, |ui, viewport| {
                let range = metrics.range_for(viewport.min.y, viewport.max.y);
                ui.allocate_space(egui::vec2(
                    ui.available_width(),
                    metrics.offset_of(range.start),
                ));

                for row in range.clone() {
                    let index = visible[row];
                    let message = &app.messages[index];
                    wanted.extend(message.image_urls().map(str::to_string));

                    let response = ui.scope(|ui| {
                        message_row(app, ui, message, &mut row_action);
                    });
                    metrics.set_height(row, response.response.rect.height() + gap);
                }

                let consumed = metrics.offset_of(range.end);
                let remaining = (metrics.total() - consumed).max(0.0);
                ui.allocate_space(egui::vec2(ui.available_width(), remaining));
            });

        app.metrics = metrics;

        match row_action {
            Some(RowAction::Edit(id)) => {
                if let Some(message) = app.messages.iter().find(|m| m.id == id).cloned() {
                    app.begin_edit(&message);
                }
            }
            Some(RowAction::Delete(id)) => app.request_delete(id),
            None => {}
        }

        // Only rows that were actually drawn pull their images down; scrolling
        // past a 9 MB screenshot never touches the network.
        if app.images.prepare(&wanted) > 0 {
            // Wake up when decodes land instead of waiting for the telemetry tick.
            ui.ctx().request_repaint();
        }
    });
}

/// What a row's menu asked for. Rows are drawn from `&RustcordApp`, so the
/// action is recorded and applied once the borrow ends.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RowAction {
    Edit(String),
    Delete(String),
}

/// A single message row, separated so the virtualised list can measure it.
fn message_row(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    message: &Message,
    action: &mut Option<RowAction>,
) {
    let theme = app.plugins.theme();

    ui.horizontal_top(|ui| {
        // Discord's message row: 40 px avatar, 16 px name line, 12 px timestamp.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        ui.painter().circle_filled(
            rect.center(),
            20.0,
            crate::utils::lift(theme.background, 20),
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial(&message.author.username),
            egui::FontId::proportional(15.0),
            theme.text,
        );

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&message.author.username)
                        .size(15.0)
                        .strong()
                        .color(theme.text),
                );
                ui.label(
                    RichText::new(message.timestamp.format("%H:%M").to_string())
                        .size(11.0)
                        .color(theme.muted),
                );

                if app.can_modify(message) {
                    ui.menu_button(RichText::new("⋯").color(theme.muted), |ui| {
                        if ui.button("Edit message").clicked() {
                            *action = Some(RowAction::Edit(message.id.clone()));
                            ui.close();
                        }
                        if ui.button("Delete message").clicked() {
                            *action = Some(RowAction::Delete(message.id.clone()));
                            ui.close();
                        }
                    });
                }
            });
            ui.label(RichText::new(&message.content).size(15.0).color(theme.text));

            for attachment in &message.attachments {
                if attachment.is_image() {
                    image_block(app, ui, &attachment.url, attachment.dimensions());
                } else {
                    ui.label(
                        RichText::new(format!(
                            "📎 {} · {}",
                            attachment.filename,
                            crate::sysinfo::human_bytes(attachment.size)
                        ))
                        .small()
                        .color(theme.muted),
                    );
                }
            }

            for embed in &message.embeds {
                embed_block(app, ui, embed);
            }

            if !message.reactions.is_empty() {
                ui.horizontal(|ui| {
                    for reaction in &message.reactions {
                        let text = format!("{} {}", reaction.emoji, reaction.count);
                        let mut rich = RichText::new(text).small();
                        if reaction.me {
                            rich = rich.color(theme.accent).strong();
                        }
                        ui.label(rich);
                    }
                });
            }
        });
    });
}

/// Draw an attachment or embed image, or a placeholder of the same size while
/// it decodes. The placeholder uses Discord's reported dimensions so the row
/// does not jump when the texture arrives.
fn image_block(app: &RustcordApp, ui: &mut egui::Ui, url: &str, known: Option<(u32, u32)>) {
    let theme = app.plugins.theme();

    if app.images.failure(url).is_some() {
        ui.label(
            RichText::new("image unavailable")
                .small()
                .color(theme.muted),
        );
        return;
    }

    let available = ui.available_width().min(360.0);
    let target = match app.images.peek(url) {
        Some(texture) => fitted_size(available, texture.size_vec2()),
        None => {
            let (width, height) = known.unwrap_or((4, 3));
            fitted_size(
                available,
                egui::vec2(width.max(1) as f32, height.max(1) as f32),
            )
        }
    };

    let (rect, _) = ui.allocate_exact_size(target, egui::Sense::hover());

    match app.images.peek(url) {
        Some(texture) => {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), theme.surface_alt);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if app.images.is_pending(url) {
                    "loading image…"
                } else {
                    "image queued…"
                },
                egui::FontId::proportional(11.0),
                theme.muted,
            );
        }
    }
}

fn embed_block(app: &RustcordApp, ui: &mut egui::Ui, embed: &Embed) {
    if embed.is_empty() {
        return;
    }

    let theme = app.plugins.theme();
    let accent = embed
        .color
        .map(|color| {
            egui::Color32::from_rgb(
                ((color >> 16) & 0xff) as u8,
                ((color >> 8) & 0xff) as u8,
                (color & 0xff) as u8,
            )
        })
        .unwrap_or(theme.accent);

    egui::Frame::group(ui.style())
        .fill(theme.surface)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(420.0));

            if let Some(title) = &embed.title {
                ui.label(RichText::new(title).strong().color(accent));
            }
            if let Some(description) = &embed.description {
                ui.label(RichText::new(description).small().color(theme.text));
            }
            if let Some(url) = embed.image_url() {
                let known =
                    embed
                        .image
                        .as_ref()
                        .and_then(|media| match (media.width, media.height) {
                            (Some(width), Some(height)) if width > 0 && height > 0 => {
                                Some((width, height))
                            }
                            _ => None,
                        });
                image_block(app, ui, url, known);
            }
            if let Some(url) = &embed.url {
                ui.label(RichText::new(url).small().color(theme.muted));
            }
        });
}

/// Fit an image into the message column while preserving its aspect ratio.
fn fitted_size(available: f32, image: egui::Vec2) -> egui::Vec2 {
    const MAX_HEIGHT: f32 = 220.0;

    if image.x <= 0.0 || image.y <= 0.0 {
        return egui::vec2(available, 90.0);
    }

    let scale = (available / image.x).min(1.0);
    let mut width = image.x * scale;
    let mut height = image.y * scale;

    if height > MAX_HEIGHT {
        let shrink = MAX_HEIGHT / height;
        width *= shrink;
        height = MAX_HEIGHT;
    }

    egui::vec2(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_images_fill_the_column() {
        let size = fitted_size(360.0, egui::vec2(1200.0, 400.0));
        assert!((size.x - 360.0).abs() < 0.01);
        assert!((size.y - 120.0).abs() < 0.01);
    }

    #[test]
    fn tall_images_are_capped_by_height() {
        let size = fitted_size(360.0, egui::vec2(400.0, 2000.0));
        assert!((size.y - 220.0).abs() < 0.01);
        assert!(size.x <= 360.0);
        assert!(size.x > 0.0);
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let size = fitted_size(360.0, egui::vec2(64.0, 32.0));
        assert!((size.x - 64.0).abs() < 0.01);
        assert!((size.y - 32.0).abs() < 0.01);
    }

    #[test]
    fn missing_dimensions_still_produce_a_placeholder() {
        let size = fitted_size(300.0, egui::vec2(0.0, 0.0));
        assert!(size.x > 0.0 && size.y > 0.0);
    }
}
