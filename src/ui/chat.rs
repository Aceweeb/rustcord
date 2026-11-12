use crate::app::RustcordApp;
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add_space(12.0);
                for message in app.messages.iter().filter(|message| {
                    message.channel_id.as_deref() == app.selected_channel.as_deref()
                }) {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            18.0,
                            app.plugins.theme().surface_alt,
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            initial(&message.author.username),
                            egui::FontId::proportional(14.0),
                            app.plugins.theme().text,
                        );

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&message.author.username)
                                        .strong()
                                        .color(app.plugins.theme().text),
                                );
                                ui.label(
                                    RichText::new(message.timestamp.format("%H:%M").to_string())
                                        .small()
                                        .color(app.plugins.theme().muted),
                                );
                            });
                            ui.label(
                                RichText::new(&message.content).color(app.plugins.theme().text),
                            );

                            if !message.reactions.is_empty() {
                                ui.horizontal(|ui| {
                                    for reaction in &message.reactions {
                                        let text = format!("{} {}", reaction.emoji, reaction.count);
                                        let mut rich = RichText::new(text).small();
                                        if reaction.me {
                                            rich = rich.color(app.plugins.theme().accent).strong();
                                        }
                                        ui.label(rich);
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(10.0);
                }
            });

        let has_visible_messages = app
            .messages
            .iter()
            .any(|message| message.channel_id.as_deref() == app.selected_channel.as_deref());

        if !has_visible_messages {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No messages in this channel yet.")
                        .color(app.plugins.theme().muted),
                );
            });
        }
    });
}
