use crate::app::RustcordApp;
use crate::models::{Embed, Message, User};
use crate::utils::initial;
use chrono::{DateTime, Datelike, FixedOffset, Local, Utc};
use egui::RichText;

/// Discord's per-message character limit, mirrored from the REST guard so the
/// composer counter and the send check cannot disagree.
const MESSAGE_LIMIT: usize = crate::api::discord::DiscordClient::MAX_CONTENT_CHARS;

/// The stamp drawn on a message row, plus the exact time for its tooltip.
/// `long` adds Today/Yesterday/`d Mon` so every row carries its date; the clock
/// alone is enough when the setting is off. Pure so day boundaries are testable.
pub(crate) fn message_timestamp(
    timestamp: DateTime<Utc>,
    now: DateTime<FixedOffset>,
    long: bool,
) -> (String, String) {
    let local = timestamp.with_timezone(now.offset());
    let clock = local.format("%H:%M").to_string();
    let detail = local.format("%A, %-d %B %Y at %H:%M:%S %Z").to_string();
    if !long {
        return (clock, detail);
    }

    let date = local.date_naive();
    let today = now.date_naive();
    let stamp = if date == today {
        format!("Today at {clock}")
    } else if Some(date) == today.pred_opt() {
        format!("Yesterday at {clock}")
    } else if date.year() == today.year() {
        format!("{} at {clock}", local.format("%-d %b"))
    } else {
        format!("{} at {clock}", local.format("%-d %b %Y"))
    };
    (stamp, detail)
}

/// Discord stacks consecutive messages from one author into a single block for
/// seven minutes: only the first row carries the avatar, name and full stamp.
/// Replies start a new block even when the author and minute match.
pub(crate) fn same_message_group(
    previous_author: &str,
    previous_at: DateTime<Utc>,
    author: &str,
    at: DateTime<Utc>,
    is_reply: bool,
) -> bool {
    !is_reply && previous_author == author && (at - previous_at).num_seconds().abs() < 7 * 60
}

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    // Typing is tracked per channel and expires by itself, so this only reads
    // state. The sentence is built once and shared by the header and composer;
    // the dots are a pure function of the frame clock. While somebody types, a
    // bounded repaint drives the animation and keeps expiry honest.
    let typing_now = ui.input(|input| input.time);
    let typing_text = app
        .selected_channel
        .as_deref()
        .and_then(|channel| app.typing.summary(channel, typing_now))
        .map(|sentence| format!("{sentence}{}", crate::models::typing_dots(typing_now)));
    if typing_text.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(120));
    }

    // Discord's 48 px channel header: a muted #, the name, then the icons.
    egui::Panel::top("chat_top").show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let channel = app
                .selected_channel_name()
                .map(str::to_string)
                .unwrap_or_else(|| "No channel selected".to_string());
            let is_dm = app
                .selected_channel
                .as_deref()
                .and_then(|id| app.channels.iter().find(|known| known.id == id))
                .is_some_and(|known| known.guild_id == crate::models::DM_GUILD_ID);
            ui.label(
                RichText::new(if is_dm { "@" } else { "#" })
                    .size(20.0)
                    .color(app.plugins.theme().muted),
            );
            ui.label(
                RichText::new(channel)
                    .size(16.0)
                    .strong()
                    .color(app.plugins.theme().text),
            );

            // The same live indicator as the composer, so a glance at the header
            // is enough when the input field is scrolled out of view.
            if let Some(text) = &typing_text {
                ui.label(
                    RichText::new("\u{270e}")
                        .small()
                        .color(app.plugins.theme().accent),
                );
                ui.label(
                    RichText::new(text)
                        .size(12.0)
                        .italics()
                        .color(app.plugins.theme().muted),
                )
                .on_hover_text("Live typing indicator for this channel");
            }

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

    let mut dm_target: Option<crate::models::User> = None;

    egui::Panel::right("member_list")
        .exact_size(240.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(app.plugins.theme().surface)
                .inner_margin(egui::Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            let theme = app.plugins.theme().clone();
            ui.add_space(12.0);

            // The real roster when the Members intent is available; otherwise
            // the authors of loaded messages, so the panel is never useless.
            let roster = app.roster();
            let mut members: Vec<crate::models::User> = if roster.is_empty() {
                let mut seen = std::collections::BTreeSet::new();
                app.messages
                    .iter()
                    .rev()
                    .filter(|message| seen.insert(message.author.id.clone()))
                    .map(|message| message.author.clone())
                    .collect()
            } else {
                roster.to_vec()
            };
            members.sort_by(|a, b| {
                a.username
                    .to_lowercase()
                    .cmp(&b.username.to_lowercase())
                    .then_with(|| a.id.cmp(&b.id))
            });

            if members.is_empty() {
                let hint = if app.member_intent_missing {
                    "Enable the Server Members Intent in the Developer Portal to load the roster."
                } else if app.live_data {
                    "Loading members\u{2026}"
                } else {
                    "Members appear once a bot token is connected."
                };
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(hint).size(11.0).color(theme.muted)).wrap());
            }

            let counts = app
                .selected_guild
                .as_ref()
                .and_then(|id| app.guild_counts.get(id));
            // Discord's approximate counts cover members beyond the roster cap.
            // Without them fall back to the guild payload and then the roster.
            let active = counts
                .and_then(|counts| counts.active)
                .map(|count| count as usize)
                .unwrap_or_else(|| {
                    members
                        .iter()
                        .filter(|member| member.status != crate::models::Presence::Offline)
                        .count()
                });
            let total = counts
                .and_then(|counts| counts.total)
                .map(|count| count as usize)
                .or_else(|| {
                    app.selected_guild
                        .as_ref()
                        .and_then(|id| app.guilds.iter().find(|guild| &guild.id == id))
                        .and_then(|guild| guild.member_count)
                        .map(|count| count as usize)
                })
                .unwrap_or(members.len())
                .max(members.len());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("MEMBERS \u{2014} {total}"))
                        .size(11.0)
                        .strong()
                        .color(theme.muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{active} active"))
                            .size(10.0)
                            .color(theme.muted),
                    );
                });
            });

            ui.add_space(6.0);
            // The roster is capped and the loader deduplicates, so each avatar
            // starts downloading once; the textures share the attachment budget.
            let avatar_urls: Vec<String> = members
                .iter()
                .filter_map(|member| member.avatar_url())
                .collect();
            app.images.prepare(&avatar_urls);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Discord splits the list into ONLINE and OFFLINE groups. The
                    // demo roster has no presence data, so it lands under online.
                    for (title, offline) in [("ONLINE", false), ("OFFLINE", true)] {
                        let group: Vec<&crate::models::User> = members
                            .iter()
                            .filter(|member| {
                                (member.status == crate::models::Presence::Offline) == offline
                            })
                            .collect();
                        if group.is_empty() {
                            continue;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("{title} \u{2014} {}", group.len()))
                                .size(10.0)
                                .strong()
                                .color(theme.muted),
                        );
                        ui.add_space(2.0);
                        for member in group {
                            ui.horizontal(|ui| {
                                let _ = avatar(app, ui, member, 32.0);
                                ui.label(
                                    RichText::new(&member.username).size(14.0).color(theme.text),
                                );
                                if app.live_data && member.id != app.current_user.id {
                                    if ui
                                        .small_button("DM")
                                        .on_hover_text("Open a direct message")
                                        .clicked()
                                    {
                                        dm_target = Some(member.clone());
                                    }
                                }
                            });
                        }
                    }
                });
        });

    if let Some(user) = dm_target {
        app.open_dm_with(&user);
    }

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

        // A reply is only valid in the channel the target lives in, so it is
        // hidden (and dropped on send) once another channel is opened.
        if let Some(reply) = app
            .replying_to
            .clone()
            .filter(|reply| reply.channel_id == app.selected_channel)
        {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Replying to {}", reply.author))
                        .size(12.0)
                        .strong()
                        .color(app.plugins.theme().accent),
                );
                ui.label(
                    RichText::new(reply.preview(60))
                        .size(11.0)
                        .color(app.plugins.theme().muted),
                );
                if ui
                    .small_button("\u{2715}")
                    .on_hover_text("Cancel reply")
                    .clicked()
                {
                    app.cancel_reply();
                }
            });
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                app.cancel_reply();
            }
        }

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

        if let Some(typing) = &typing_text {
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
                let counter = app.settings.character_counter;
                ui.horizontal(|ui| {
                    // Leave room for the counter so it never sits under the text.
                    let reserve = if counter { 62.0 } else { 0.0 };
                    let width = (ui.available_width() - reserve).max(80.0);
                    let response = ui.add_sized(
                        egui::vec2(width, 22.0),
                        egui::TextEdit::singleline(&mut app.draft)
                            .hint_text(hint)
                            .frame(egui::Frame::new())
                            .text_color(theme.text),
                    );

                    if counter {
                        let count = app.draft.chars().count();
                        let color = if count > MESSAGE_LIMIT {
                            theme.danger
                        } else if count + 200 >= MESSAGE_LIMIT {
                            theme.accent
                        } else {
                            theme.muted
                        };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{count}/{MESSAGE_LIMIT}"))
                                    .size(11.0)
                                    .color(color),
                            )
                            .on_hover_text(if count > MESSAGE_LIMIT {
                                "Over Discord's limit — shorten the message to send it"
                            } else {
                                "Discord allows 2000 characters per message"
                            });
                        });
                    }

                    if response.changed() {
                        // Each keystroke can ask for a notice; `notify_typing`
                        // throttles it to one REST call per eight seconds.
                        app.notify_typing();
                    }

                    // A draft that stays focused must keep telling the channel you
                    // are still there: Discord's notice lapses after ten seconds.
                    // `notify_typing` is a no-op until the eight-second window is
                    // up, and the wait schedules exactly one wake-up for the
                    // renewal instead of polling every frame.
                    if response.has_focus() && !app.draft.trim().is_empty() {
                        app.notify_typing();
                        if let Some(wait) = app.typing_refresh_wait() {
                            ui.ctx().request_repaint_after(wait);
                        }
                    }

                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        app.send_current_draft();
                        response.request_focus();
                    }
                });
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

        // Discord groups each author's consecutive messages; computed for the
        // whole channel so a viewport that starts mid-block still draws compact.
        let grouped: Vec<bool> = visible
            .iter()
            .enumerate()
            .map(|(row, index)| {
                if row == 0 {
                    return false;
                }
                let current = &app.messages[*index];
                let previous = &app.messages[visible[row - 1]];
                same_message_group(
                    &previous.author.id,
                    previous.timestamp,
                    &current.author.id,
                    current.timestamp,
                    current.reply_to.is_some(),
                )
            })
            .collect();

        // Heights measured for a different channel would be wrong for a frame.
        if app.metrics_channel != channel_id {
            app.metrics.clear();
            app.metrics_channel = channel_id.clone();
        }

        // History state first, even for an empty channel: a channel that failed
        // to load must say why, not look like a channel nobody has posted in.
        let load_error = history_controls(app, ui, channel_id.as_deref());

        if visible.is_empty() {
            ui.centered_and_justified(|ui| match &load_error {
                Some(error) => {
                    ui.label(RichText::new(error).color(app.plugins.theme().danger));
                }
                None => {
                    ui.label(
                        RichText::new("No messages in this channel yet.")
                            .color(app.plugins.theme().muted),
                    );
                }
            });
            return;
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

        // A search result asked for a specific row. Heights below it may still be
        // estimates, so this lands at the best known offset and the brief tint
        // makes the message itself obvious. The request is consumed either way so
        // it can never fight the user's scrolling on later frames.
        let mut jump_offset = None;
        let mut jump_target = None;
        if let Some(target) = app.scroll_to_message.clone() {
            if let Some(row) = visible
                .iter()
                .position(|index| app.messages[*index].id == target)
            {
                // Heights above the target are estimates on the first frame; the
                // estimate improves as rows around the landing get measured, so
                // keep asking until the row is actually drawn (bounded).
                jump_offset = Some((metrics.estimated_offset_of(row) - 36.0).max(0.0));
                app.jump_attempts = app.jump_attempts.saturating_add(1);
                if app.jump_attempts <= 6 {
                    jump_target = Some(target);
                } else {
                    app.scroll_to_message = None;
                    app.jump_attempts = 0;
                }
            } else {
                // The message is not in the loaded page; do not scroll at all.
                app.scroll_to_message = None;
                app.jump_attempts = 0;
            }
        }
        // Any manual scroll wins immediately over a jump in progress.
        let user_scrolled = ui.input(|input| input.smooth_scroll_delta != egui::Vec2::ZERO);
        if user_scrolled {
            app.scroll_to_message = None;
            jump_target = None;
            jump_offset = None;
            app.jump_attempts = 0;
        }
        let highlight_active = app
            .jump_highlight
            .as_ref()
            .is_some_and(|(_, until)| std::time::Instant::now() < *until);
        if highlight_active {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let mut scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(jump_offset.is_none());
        if let Some(offset) = jump_offset {
            scroll = scroll.vertical_scroll_offset(offset);
        }

        scroll.show_viewport(ui, |ui, viewport| {
            let range = metrics.range_for(viewport.min.y, viewport.max.y);
            let mut saw_target = false;
            ui.allocate_space(egui::vec2(
                ui.available_width(),
                metrics.offset_of(range.start),
            ));

            for row in range.clone() {
                let index = visible[row];
                let message = &app.messages[index];
                if jump_target.as_deref() == Some(message.id.as_str()) {
                    saw_target = true;
                }
                wanted.extend(message.image_urls().map(str::to_string));
                if let Some(url) = message.author.avatar_url() {
                    wanted.push(url);
                }
                if let Some(url) = message.author.banner_url() {
                    wanted.push(url);
                }
                if let Some(url) = message.author.decoration_url() {
                    wanted.push(url);
                }

                let focused = app.jump_highlight.as_ref().is_some_and(|(id, until)| {
                    id == &message.id && std::time::Instant::now() < *until
                });
                let response = ui.scope(|ui| {
                    if focused {
                        egui::Frame::new()
                            .fill(app.plugins.theme().accent.gamma_multiply(0.22))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                                message_row(app, ui, message, &mut row_action, grouped[row]);
                            });
                    } else {
                        message_row(app, ui, message, &mut row_action, grouped[row]);
                    }
                });
                // A stacked row hugs the one below it the way Discord's blocks do.
                let next_grouped = grouped.get(row + 1).copied().unwrap_or(false);
                let row_gap = if next_grouped { 0.0 } else { gap };
                metrics.set_height(row, response.response.rect.height() + row_gap);
            }

            let consumed = metrics.offset_of(range.end);
            let remaining = (metrics.total() - consumed).max(0.0);
            ui.allocate_space(egui::vec2(ui.available_width(), remaining));

            if jump_target.is_some() && !saw_target {
                // Another frame measures the rows just drawn and lands closer,
                // so keep going rather than waiting for an input event.
                ui.ctx().request_repaint();
            } else if saw_target {
                app.scroll_to_message = None;
                app.jump_attempts = 0;
            }
        });

        app.metrics = metrics;
        // Drop the tint once it has served its purpose, so no wake-ups linger.
        if app
            .jump_highlight
            .as_ref()
            .is_some_and(|(_, until)| std::time::Instant::now() >= *until)
        {
            app.jump_highlight = None;
        }

        match row_action {
            Some(RowAction::Copy(text)) => {
                ui.ctx().copy_text(text);
                app.status_line = Some("message copied".to_string());
            }
            Some(RowAction::Edit(id)) => {
                if let Some(message) = app.messages.iter().find(|m| m.id == id).cloned() {
                    app.begin_edit(&message);
                }
            }
            Some(RowAction::Delete(id)) => app.request_delete(id),
            Some(RowAction::Reply(id)) => {
                if let Some(message) = app.messages.iter().find(|m| m.id == id).cloned() {
                    app.begin_reply(&message);
                }
            }
            Some(RowAction::React { message_id, emoji }) => app.react_to(message_id, emoji),
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

/// Pagination and load state for the open channel. Returns the last error so an
/// empty channel can show it instead of a cheerful "no messages yet".
fn history_controls(
    app: &mut RustcordApp,
    ui: &mut egui::Ui,
    channel_id: Option<&str>,
) -> Option<String> {
    if !app.live_data {
        return None;
    }
    let state = channel_id.and_then(|id| app.channel_history(id)).cloned()?;
    let theme = app.plugins.theme().clone();

    ui.horizontal(|ui| {
        if state.loading {
            ui.label(
                RichText::new("loading messages…")
                    .small()
                    .color(theme.muted),
            );
        } else {
            if !state.exhausted && ui.button("Load older messages").clicked() {
                app.load_older();
            }
            if state.exhausted {
                ui.label(RichText::new("start of channel").small().color(theme.muted));
            }
        }
    });

    if let Some(error) = &state.error {
        // On its own line and wrapped: a 401 explanation is longer than a row.
        ui.add(egui::Label::new(RichText::new(error).small().color(theme.danger)).wrap());
    }

    ui.separator();
    state.error
}

/// What a row's menu asked for. Rows are drawn from `&RustcordApp`, so the
/// action is recorded and applied once the borrow ends.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RowAction {
    Copy(String),
    Edit(String),
    Delete(String),
    Reply(String),
    React { message_id: String, emoji: String },
}

/// The quick reactions in the context menu, in the order Discord shows them.
pub(crate) const QUICK_REACTIONS: [&str; 4] =
    ["\u{1f44d}", "\u{2764}\u{fe0f}", "\u{1f602}", "\u{1f389}"];

/// A single message row, separated so the virtualised list can measure it.
fn message_row(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    message: &Message,
    action: &mut Option<RowAction>,
    grouped: bool,
) {
    let response = ui
        // A clickable container so a right-click anywhere on the row opens the
        // context menu; the widgets inside still get their own clicks first.
        .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
            message_row_body(app, ui, message, action, grouped);
        })
        .response;

    response.context_menu(|ui| {
        ui.set_min_width(190.0);

        ui.horizontal(|ui| {
            for emoji in QUICK_REACTIONS {
                if ui
                    .button(RichText::new(emoji).size(16.0))
                    .on_hover_text("React")
                    .clicked()
                {
                    *action = Some(RowAction::React {
                        message_id: message.id.clone(),
                        emoji: emoji.to_string(),
                    });
                    ui.close();
                }
            }
        });

        ui.separator();

        if ui
            .add_enabled(!message.content.is_empty(), egui::Button::new("Copy text"))
            .clicked()
        {
            *action = Some(RowAction::Copy(message.content.clone()));
            ui.close();
        }

        if ui.button("Reply").clicked() {
            *action = Some(RowAction::Reply(message.id.clone()));
            ui.close();
        }

        if app.can_modify(message) {
            ui.separator();
            if ui.button("Edit message").clicked() {
                *action = Some(RowAction::Edit(message.id.clone()));
                ui.close();
            }
            if ui.button("Delete message").clicked() {
                *action = Some(RowAction::Delete(message.id.clone()));
                ui.close();
            }
        }
    });
}

fn message_row_body(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    message: &Message,
    action: &mut Option<RowAction>,
    grouped: bool,
) {
    let theme = app.plugins.theme();

    ui.horizontal_top(|ui| {
        if grouped {
            // A continuation row keeps the avatar gutter. Discord only reveals
            // the clock on hover; a faint always-on clock is kinder to a desktop
            // client and still reads as part of the block.
            ui.allocate_ui_with_layout(
                egui::vec2(40.0, 16.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let (stamp, detail) =
                        message_timestamp(message.timestamp, Local::now().fixed_offset(), false);
                    ui.label(RichText::new(stamp).size(10.0).color(theme.muted))
                        .on_hover_text(detail);
                },
            );
        } else {
            // Discord's message row: 40 px avatar, 16 px name line, 12 px timestamp.
            avatar(app, ui, &message.author, 40.0);
        }

        ui.vertical(|ui| {
            if !grouped {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&message.author.username)
                            .size(15.0)
                            .strong()
                            .color(theme.text),
                    );
                    let (stamp, detail) = message_timestamp(
                        message.timestamp,
                        Local::now().fixed_offset(),
                        app.settings.message_timestamps,
                    );
                    ui.label(RichText::new(stamp).size(11.0).color(theme.muted))
                        .on_hover_text(detail);

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
            }
            // Discord's reply line: who it answers, then a truncated copy.
            if let Some(reply) = &message.reply_to {
                ui.label(
                    RichText::new(format!("\u{21b3} {}: {}", reply.author, reply.preview(70)))
                        .size(12.0)
                        .color(theme.muted),
                );
            }

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

                        // A chip is a button: our own toggles off, anyone's adds
                        // or removes ours on top of theirs.
                        let response = ui.add(egui::Button::new(rich).frame(false));
                        if response.clicked() {
                            *action = Some(RowAction::React {
                                message_id: message.id.clone(),
                                emoji: reaction.emoji.clone(),
                            });
                        }

                        let hint = if reaction.me {
                            "Remove your reaction"
                        } else {
                            "Add reaction"
                        };
                        response.on_hover_text(hint);
                    }
                });
            }
        });
    });
}

/// Draw an attachment or embed image, or a placeholder of the same size while
/// it decodes. The placeholder uses Discord's reported dimensions so the row
/// does not jump when the texture arrives.

/// Draw a user's avatar at `size`, falling back to the initial when the CDN image
/// is not loaded yet (or the user has none). Hovering opens the profile card,
/// which is where Discord's Nitro cosmetics — banner, accent colour and avatar
/// decoration — are shown.
pub(crate) fn avatar(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    user: &User,
    size: f32,
) -> egui::Response {
    let response = avatar_visual(app, ui, user, size);
    response.on_hover_ui(|ui| profile_card(app, ui, user))
}

/// The avatar without the hover card, so the card can draw one without nesting
/// another card inside itself.
fn avatar_visual(app: &RustcordApp, ui: &mut egui::Ui, user: &User, size: f32) -> egui::Response {
    let theme = app.plugins.theme();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());

    match user
        .avatar_url()
        .as_deref()
        .and_then(|url| app.images.peek(url))
    {
        Some(texture) => {
            let image = egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                .corner_radius(egui::CornerRadius::same((size / 2.0) as u8));
            ui.put(rect, image);
        }
        None => {
            ui.painter().circle_filled(
                rect.center(),
                size / 2.0,
                crate::utils::lift(theme.background, 20),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial(&user.username),
                egui::FontId::proportional(size * 0.375),
                theme.text,
            );
        }
    }

    // The decoration is a transparent PNG drawn slightly outside the avatar.
    if let Some(texture) = user
        .decoration_url()
        .as_deref()
        .and_then(|url| app.images.peek(url))
    {
        ui.painter().image(
            texture.id(),
            rect.expand(size * 0.16),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    response
}

/// A compact version of Discord's profile popout: banner (or accent colour),
/// avatar with its decoration, display name and handle. Everything is drawn from
/// the user object the gateway/REST already sent; no extra request is made.
fn profile_card(app: &RustcordApp, ui: &mut egui::Ui, user: &User) {
    let theme = app.plugins.theme();
    ui.set_min_width(280.0);

    let (banner, _) = ui.allocate_exact_size(egui::vec2(280.0, 72.0), egui::Sense::hover());
    match user
        .banner_url()
        .as_deref()
        .and_then(|url| app.images.peek(url))
    {
        Some(texture) => {
            ui.painter().image(
                texture.id(),
                banner,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            let fill = user
                .accent_color
                .map(accent_color)
                .unwrap_or_else(|| crate::utils::lift(theme.surface_alt, 12));
            ui.painter()
                .rect_filled(banner, egui::CornerRadius::same(6), fill);
        }
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        avatar_visual(app, ui, user, 48.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(user.display_name())
                    .size(16.0)
                    .strong()
                    .color(theme.text),
            );
            ui.label(
                RichText::new(format!("@{}", user.username))
                    .size(12.0)
                    .color(theme.muted),
            );
            let mut tags = Vec::new();
            if user.banner.is_some() {
                tags.push("profile banner");
            }
            if user.accent_color.is_some() {
                tags.push("accent colour");
            }
            if user.avatar_decoration.is_some() {
                tags.push("avatar decoration");
            }
            if !tags.is_empty() {
                ui.label(
                    RichText::new(tags.join(" · "))
                        .size(11.0)
                        .color(theme.muted),
                );
            }
        });
    });
}

/// Packed `0xRRGGBB` to an egui colour.
fn accent_color(packed: u32) -> egui::Color32 {
    egui::Color32::from_rgb(
        ((packed >> 16) & 0xff) as u8,
        ((packed >> 8) & 0xff) as u8,
        (packed & 0xff) as u8,
    )
}

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

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("fixed timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn long_timestamps_name_the_day() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T21:04:00Z")
            .expect("now")
            .fixed_offset();

        let (stamp, detail) = message_timestamp(utc("2026-11-12T09:05:00Z"), now, true);
        assert_eq!(stamp, "Today at 09:05");
        assert!(
            detail.contains("Thursday, 12 November 2026 at 09:05:00"),
            "{detail}"
        );

        let (stamp, _) = message_timestamp(utc("2026-11-11T23:59:00Z"), now, true);
        assert_eq!(stamp, "Yesterday at 23:59");

        let (stamp, _) = message_timestamp(utc("2026-01-03T07:00:00Z"), now, true);
        assert_eq!(stamp, "3 Jan at 07:00");

        let (stamp, _) = message_timestamp(utc("2025-12-31T23:00:00Z"), now, true);
        assert_eq!(stamp, "31 Dec 2025 at 23:00");
    }

    #[test]
    fn short_timestamps_keep_only_the_clock() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T21:04:00Z")
            .expect("now")
            .fixed_offset();
        let (stamp, detail) = message_timestamp(utc("2026-11-12T09:05:00Z"), now, false);
        assert_eq!(stamp, "09:05");
        // The tooltip stays exact even when the row is terse.
        assert!(detail.contains("09:05:00"), "{detail}");
    }

    #[test]
    fn consecutive_messages_from_one_author_stack() {
        let at = utc("2026-11-12T09:00:00Z");
        assert!(same_message_group("42", at, "42", at, false));
        assert!(same_message_group(
            "42",
            at,
            "42",
            at + chrono::Duration::minutes(6),
            false
        ));
        assert!(!same_message_group(
            "42",
            at,
            "42",
            at + chrono::Duration::minutes(8),
            false
        ));
        assert!(!same_message_group("42", at, "43", at, false));
        // A reply introduces a new block even in the same minute.
        assert!(!same_message_group("42", at, "42", at, true));
    }

    #[test]
    fn missing_dimensions_still_produce_a_placeholder() {
        let size = fitted_size(300.0, egui::vec2(0.0, 0.0));
        assert!(size.x > 0.0 && size.y > 0.0);
    }
}
