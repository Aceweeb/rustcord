use crate::app::RustcordApp;
use crate::models::Message;
use crate::utils::initial;
use egui::RichText;

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    egui::Panel::top("chat_top").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let channel = app
                .selected_channel_name()
                .map(|name| format!("# {name}"))
                .unwrap_or_else(|| "No channel selected".to_string());
            ui.heading(RichText::new(channel).size(18.0));

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
                ui.label(RichText::new("🔍").color(app.plugins.theme().muted));
                ui.label(RichText::new("☰").color(app.plugins.theme().muted));
            });
        });
        ui.add_space(4.0);
        ui.separator();
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
        .default_size(200.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new("ONLINE — MOCK DATA")
                    .small()
                    .color(app.plugins.theme().muted),
            );
            ui.add_space(4.0);

            let mut seen = std::collections::BTreeSet::new();
            for message in app.messages.iter().rev() {
                if seen.insert(message.author.username.clone()) {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            12.0,
                            app.plugins.theme().surface_alt,
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            initial(&message.author.username),
                            egui::FontId::proportional(11.0),
                            app.plugins.theme().text,
                        );
                        ui.label(&message.author.username);
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
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.label(
            RichText::new(status)
                .small()
                .color(app.plugins.theme().muted),
        );
    });

    egui::Panel::bottom("composer").show(ui, |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let hint = app
                .selected_channel_name()
                .map(|name| format!("Message #{name}"))
                .unwrap_or_else(|| "Select a channel".to_string());

            let width = (ui.available_width() - 90.0).max(120.0);
            let response = ui.add_sized(
                egui::vec2(width, 34.0),
                egui::TextEdit::singleline(&mut app.draft).hint_text(hint),
            );

            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                app.send_current_draft();
                response.request_focus();
            }

            if ui.button("Send").clicked() {
                app.send_current_draft();
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
                    let response = ui.scope(|ui| {
                        message_row(app, ui, &app.messages[index]);
                    });
                    metrics.set_height(row, response.response.rect.height() + gap);
                }

                let consumed = metrics.offset_of(range.end);
                let remaining = (metrics.total() - consumed).max(0.0);
                ui.allocate_space(egui::vec2(ui.available_width(), remaining));
            });

        app.metrics = metrics;
    });
}

/// A single message row, separated so the virtualised list can measure it.
fn message_row(app: &RustcordApp, ui: &mut egui::Ui, message: &Message) {
    let theme = app.plugins.theme();

    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 18.0, theme.surface_alt);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial(&message.author.username),
            egui::FontId::proportional(14.0),
            theme.text,
        );

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&message.author.username)
                        .strong()
                        .color(theme.text),
                );
                ui.label(
                    RichText::new(message.timestamp.format("%H:%M").to_string())
                        .small()
                        .color(theme.muted),
                );
            });
            ui.label(RichText::new(&message.content).color(theme.text));

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
