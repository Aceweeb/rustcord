use crate::app::RustcordApp;
use crate::models::{Embed, Message, Presence, User};
use crate::utils::initial;
use chrono::{DateTime, Datelike, FixedOffset, Local, Utc};
use egui::RichText;

/// Discord's per-message character limit, mirrored from the REST guard so the
/// composer counter and the send check cannot disagree.
const MESSAGE_LIMIT: usize = crate::api::discord::DiscordClient::MAX_CONTENT_CHARS;

/// A compact Unicode set covering Discord's most-used reactions. The picker is
/// deliberately dependency-free; custom guild emoji need the guild emoji API,
/// which this client does not request yet.
const EMOJI_CHOICES: &[&str] = &[
    "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "😊", "😇", "🙂", "🙃", "😉", "😌", "😍", "🥰",
    "😘", "😗", "😙", "😚", "😋", "😛", "😝", "😜", "🤪", "🤨", "🧐", "🤓", "😎", "🥳", "🤩", "😏",
    "😒", "😞", "😔", "😟", "😕", "🙁", "☹️", "😣", "😖", "😫", "😩", "🥺", "😢", "😭", "😤", "😠",
    "😡", "🤬", "🤯", "😳", "🥵", "🥶", "😱", "😨", "😰", "😥", "😓", "🤗", "🤔", "🤭", "🤫", "🤥",
    "😶", "😐", "😑", "😬", "🙄", "😯", "😦", "😧", "😮", "😲", "🥱", "😴", "🤤", "😪", "😵", "🤐",
    "🥴", "🤢", "🤮", "🤧", "😷", "🤒", "🤕", "🤑", "🤠", "😈", "👿", "👹", "👺", "🤡", "💩", "👻",
    "💀", "☠️", "👽", "👾", "🤖", "🎃", "😺", "😸", "😹", "😻", "😼", "😽", "🙀", "😿", "😾", "👍",
    "👎", "👌", "✌️", "🤞", "🤟", "🤘", "🤙", "👈", "👉", "👆", "👇", "☝️", "✋", "🤚", "🖐️", "🖖",
    "👋", "🤝", "🙏", "💪", "🦾", "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "🤎", "💔", "❣️",
    "💕", "💞", "💓", "💗", "💖", "💘", "💝", "🔥", "✨", "🎉", "🎊", "💯", "✅", "❌", "⚠️", "👀",
    "💬", "📎", "📷", "🎮", "🎵", "🍕", "🍺", "☕", "🌙", "⭐",
];

/// Insert `text` at the selected character range, or at the end when there is
/// no editor selection. Returns the caret position after the inserted text.
fn insert_at_char_range(
    draft: &mut String,
    text: &str,
    selected: Option<std::ops::Range<usize>>,
) -> usize {
    let len = draft.chars().count();
    let mut range = selected.unwrap_or(len..len);
    if range.start > range.end {
        std::mem::swap(&mut range.start, &mut range.end);
    }
    let start = range.start.min(len);
    let end = range.end.min(len);
    let byte_start = draft
        .char_indices()
        .nth(start)
        .map_or(draft.len(), |(byte, _)| byte);
    let byte_end = draft
        .char_indices()
        .nth(end)
        .map_or(draft.len(), |(byte, _)| byte);
    draft.replace_range(byte_start..byte_end, text);
    start + text.chars().count()
}

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

/// The centered label on a date divider: Today, Yesterday, then the local date.
/// Kept separate from the per-message stamp so the divider is readable even when
/// the user has turned timestamps off.
pub(crate) fn date_divider_label(timestamp: DateTime<Utc>, now: DateTime<FixedOffset>) -> String {
    let local = timestamp.with_timezone(now.offset());
    let date = local.date_naive();
    let today = now.date_naive();
    if date == today {
        "Today".to_string()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".to_string()
    } else if date.year() == today.year() {
        local.format("%-d %B").to_string()
    } else {
        local.format("%-d %B %Y").to_string()
    }
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

    // Header buttons only read `app` inside the panel; apply their actions after
    // the closure so the borrow checker stays out of the layout code.
    let mut open_search = false;
    let mut toggle_members = false;

    // Discord's 48 px channel header: a muted #, the name, topic, then icons.
    egui::Panel::top("chat_top").show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let selected = app
                .selected_channel
                .as_deref()
                .and_then(|id| app.channels.iter().find(|known| known.id == id));
            let channel = selected
                .map(|known| known.name.clone())
                .unwrap_or_else(|| "No channel selected".to_string());
            let is_dm = selected.is_some_and(|known| known.guild_id == crate::models::DM_GUILD_ID);
            let topic = selected
                .and_then(|known| known.topic.as_deref())
                .map(str::trim)
                .filter(|topic| !topic.is_empty())
                .map(str::to_string);
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
            if let Some(topic) = topic.filter(|_| !is_dm) {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("│")
                        .size(14.0)
                        .color(crate::utils::lift(app.plugins.theme().background, -20)),
                );
                ui.scope(|ui| {
                    ui.set_max_width(260.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&topic)
                                .size(12.0)
                                .color(app.plugins.theme().muted),
                        )
                        .truncate(),
                    )
                    .on_hover_text(topic);
                });
            }

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
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("🔍")
                                .size(16.0)
                                .color(app.plugins.theme().muted),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Search messages (Ctrl+F)")
                    .clicked()
                {
                    open_search = true;
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("👥")
                                .size(16.0)
                                .color(app.plugins.theme().muted),
                        )
                        .frame(false),
                    )
                    .on_hover_text(if app.show_member_list {
                        "Hide member list"
                    } else {
                        "Show member list"
                    })
                    .clicked()
                {
                    toggle_members = true;
                }
            });
        });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.max_rect().bottom(),
            egui::Stroke::new(1.0, crate::utils::lift(app.plugins.theme().background, -10)),
        );
    });

    if open_search {
        app.open_search();
    }
    if toggle_members {
        app.show_member_list = !app.show_member_list;
    }

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
    // Only avatars in the visible slice are requested. The panel borrows the
    // roster, so the mutable image cache is filled after the closure returns.
    let mut visible_avatars: Vec<String> = Vec::new();

    if app.show_member_list {
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

                // Discord's panel is scoped to the open channel, but the API has no
                // endpoint that lists everyone who can read a text channel. The loaded
                // guild roster is the nearest evidence for members who have not spoken
                // yet; thread participants who are still guild members are in it too.
                // Authors and voice users cover gaps from a capped/unavailable roster.
                let channel_id = app.selected_channel.clone();
                let mut authors: Vec<crate::models::User> = Vec::new();
                let mut seen_authors = std::collections::BTreeSet::new();
                for message in app.messages.iter().rev() {
                    if message.channel_id.as_deref() != channel_id.as_deref() {
                        continue;
                    }
                    if seen_authors.insert(message.author.id.clone()) {
                        authors.push(message.author.clone());
                    }
                }

                let mut voice_members: Vec<crate::models::VoiceMember> = channel_id
                    .as_deref()
                    .map(|id| app.voice.members(id).to_vec())
                    .unwrap_or_default();
                voice_members.sort_by_key(|member| member.username.to_lowercase());

                let mut extras = authors;
                for member in voice_members {
                    if !extras.iter().any(|known| known.id == member.user_id) {
                        extras.push(crate::models::User {
                            id: member.user_id,
                            username: member.username,
                            status: Presence::Online,
                            ..crate::models::User::default()
                        });
                    }
                }
                let members = channel_member_rows(app.roster(), &extras);

                if members.is_empty() {
                    let hint = if channel_id.is_none() {
                        "Open a channel to see who is in it."
                    } else if !app.live_data {
                        "Members appear once a bot token is connected."
                    } else if app.member_intent_missing {
                        "Enable the Server Members Intent to load the roster."
                    } else {
                        "No channel members to show."
                    };
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(RichText::new(hint).size(11.0).color(theme.muted)).wrap(),
                    );
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

                ui.label(
                    RichText::new(format!("IN CHANNEL \u{2014} {}", members.len()))
                        .size(10.0)
                        .color(theme.muted),
                )
                .on_hover_text(
                    "Loaded guild roster plus participants in this channel. Discord does \
                 not expose per-channel access, so this is the closest available list.",
                );

                ui.add_space(6.0);
                // Discord groups members by their highest hoisted role, then
                // keeps ONLINE/OFFLINE catch-alls for members without one. This
                // client gets no live presence for REST members (they all arrive
                // offline), so role sections must include them or the roles would
                // never be visible. A full roster can hold thousands of users, so
                // the section rows are virtualised: only the visible slice builds
                // avatars and buttons.
                let roles = app
                    .selected_guild
                    .as_ref()
                    .and_then(|id| app.guilds.iter().find(|guild| &guild.id == id))
                    .map(|guild| guild.roles.as_slice())
                    .unwrap_or(&[]);
                let sections = member_sections(&members, roles);
                let mut rows: Vec<MemberListRow> =
                    Vec::with_capacity(members.len() + sections.len());
                for section in sections {
                    rows.push(MemberListRow::Header {
                        title: section.title,
                        count: section.members.len(),
                        color: section.color,
                    });
                    rows.extend(section.members.into_iter().map(MemberListRow::Member));
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, MEMBER_ROW_HEIGHT, rows.len(), |ui, range| {
                        for row in &rows[range] {
                            match row {
                                MemberListRow::Header {
                                    title,
                                    count,
                                    color,
                                } => {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(ui.available_width(), MEMBER_ROW_HEIGHT),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add_space(4.0);
                                            if let Some(color) = color {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(7.0, 7.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().circle_filled(
                                                    rect.center(),
                                                    3.0,
                                                    *color,
                                                );
                                                ui.add_space(4.0);
                                            }
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(format!(
                                                        "{title} \u{2014} {count}"
                                                    ))
                                                    .size(10.0)
                                                    .strong()
                                                    .color(theme.muted),
                                                )
                                                .truncate(),
                                            );
                                        },
                                    );
                                }
                                MemberListRow::Member(member) => {
                                    let hover = ui.painter().add(egui::Shape::Noop);
                                    let response = ui
                                        .allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), MEMBER_ROW_HEIGHT),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                if let Some(url) = member.avatar_url() {
                                                    visible_avatars.push(url);
                                                }
                                                let _ = avatar_with_status(app, ui, member, 32.0);
                                                ui.label(
                                                    RichText::new(&member.username)
                                                        .size(14.0)
                                                        .color(theme.text),
                                                );
                                                if app.live_data
                                                    && member.id != app.current_user.id
                                                    && ui
                                                        .small_button("DM")
                                                        .on_hover_text("Open a direct message")
                                                        .clicked()
                                                {
                                                    dm_target = Some((*member).clone());
                                                }
                                            },
                                        )
                                        .response;
                                    if response.hovered() {
                                        ui.painter().set(
                                            hover,
                                            egui::Shape::rect_filled(
                                                response.rect.expand2(egui::vec2(4.0, 1.0)),
                                                egui::CornerRadius::same(4),
                                                crate::utils::lift(theme.surface, 10),
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                    });
            });
    }

    // Visible avatars share the attachment byte budget; the loader deduplicates
    // URLs, so requesting the same row every frame is a cache lookup.
    app.images.prepare(&visible_avatars);
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
                if let Some(attachment) = app.composer_attachment.as_ref() {
                    let filename = attachment.filename.clone();
                    let size = attachment.size;
                    let mut remove = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("📎").size(15.0).color(theme.text));
                        ui.label(RichText::new(&filename).size(12.0).color(theme.text));
                        ui.label(
                            RichText::new(crate::sysinfo::human_bytes(size))
                                .size(10.0)
                                .color(theme.muted),
                        );
                        if ui
                            .small_button("\u{2715}")
                            .on_hover_text("Remove attachment")
                            .clicked()
                        {
                            remove = true;
                        }
                    });
                    if remove {
                        app.clear_attachment();
                    }
                }

                let counter = app.settings.character_counter;
                let mut editor_id = None;
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
                    editor_id = Some(response.id);

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

                ui.horizontal(|ui| {
                    if app.editing.is_none() {
                        let attach = ui
                            .add(egui::Button::new(RichText::new("📎").size(15.0)).frame(false))
                            .on_hover_text("Attach a file");
                        if attach.clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                app.attach_file(path);
                            }
                            // The native dialog takes keyboard focus; put it back
                            // so typing the caption needs no extra click.
                            if let Some(editor_id) = editor_id {
                                ui.ctx()
                                    .memory_mut(|memory| memory.request_focus(editor_id));
                            }
                        }
                    }

                    let emoji = ui
                        .add(egui::Button::new(RichText::new("🙂").size(15.0)).frame(false))
                        .on_hover_text("Emoji");
                    let mut chosen = None;
                    egui::Popup::menu(&emoji).show(|ui| {
                        ui.set_min_width(244.0);
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                egui::Grid::new("composer-emoji-grid")
                                    .num_columns(8)
                                    .spacing([2.0, 2.0])
                                    .show(ui, |ui| {
                                        for (index, emoji) in EMOJI_CHOICES.iter().enumerate() {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(*emoji).size(18.0),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text(*emoji)
                                                .clicked()
                                            {
                                                chosen = Some((*emoji).to_string());
                                            }
                                            if index % 8 == 7 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                    });
                    if let Some(emoji) = chosen {
                        let selection = editor_id
                            .and_then(|id| egui::text_edit::TextEditState::load(ui.ctx(), id))
                            .and_then(|state| state.cursor.char_range())
                            .map(|range| {
                                let chars = range.as_sorted_char_range();
                                chars.start.0..chars.end.0
                            });
                        let caret = insert_at_char_range(&mut app.draft, &emoji, selection);
                        if let Some(editor_id) = editor_id {
                            let mut state =
                                egui::text_edit::TextEditState::load(ui.ctx(), editor_id)
                                    .unwrap_or_default();
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(caret),
                                )));
                            state.store(ui.ctx(), editor_id);
                            // Clicking the popup moved focus off the field; put it
                            // back so the next keystroke lands at the new caret.
                            ui.ctx()
                                .memory_mut(|memory| memory.request_focus(editor_id));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let hint = if app.editing.is_some() {
                            "Enter to save · Esc to cancel"
                        } else {
                            "Enter to send"
                        };
                        ui.label(RichText::new(hint).size(10.0).color(theme.muted));
                    });
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
        let now = Local::now().fixed_offset();
        let grouped: Vec<bool> = visible
            .iter()
            .enumerate()
            .map(|(row, index)| {
                if row == 0 {
                    return false;
                }
                let current = &app.messages[*index];
                let previous = &app.messages[visible[row - 1]];
                let same_day = previous.timestamp.with_timezone(now.offset()).date_naive()
                    == current.timestamp.with_timezone(now.offset()).date_naive();
                same_day
                    && same_message_group(
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
        let list_rows = build_message_rows(
            &visible,
            &app.messages,
            &grouped,
            app.new_divider.as_deref(),
            now,
        );

        // Take the metrics out so the closure can borrow `app` immutably while
        // recording measurements into a local.
        let mut metrics = std::mem::take(&mut app.metrics);
        metrics.ensure_len(list_rows.len());

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
            let target_row = visible
                .iter()
                .position(|index| app.messages[*index].id == target)
                .and_then(|visible_row| {
                    list_rows.iter().position(|row| {
                        matches!(
                            row,
                            MessageListRow::Message { index, .. }
                                if *index == visible[visible_row]
                        )
                    })
                });
            if let Some(row) = target_row {
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
                match &list_rows[row] {
                    MessageListRow::Date { label } => {
                        let theme = app.plugins.theme();
                        let response = ui.scope(|ui| {
                            divider_row(ui, label, theme.muted, 26.0);
                        });
                        metrics.set_height(row, response.response.rect.height() + 4.0);
                    }
                    MessageListRow::New => {
                        let theme = app.plugins.theme();
                        let response = ui.scope(|ui| {
                            divider_row(ui, "NEW", theme.accent, 26.0);
                        });
                        metrics.set_height(row, response.response.rect.height() + 4.0);
                    }
                    MessageListRow::Message { index, grouped } => {
                        let message = &app.messages[*index];
                        if jump_target.as_deref() == Some(message.id.as_str()) {
                            saw_target = true;
                        }
                        wanted.extend(message.image_urls().map(str::to_string));
                        if let Some(url) = message.author.avatar_url() {
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
                                        message_row(app, ui, message, &mut row_action, *grouped);
                                    });
                            } else {
                                message_row(app, ui, message, &mut row_action, *grouped);
                            }
                        });
                        // A stacked row hugs the one below it the way Discord's
                        // blocks do. Divider rows are never part of a block.
                        let next_grouped = matches!(
                            list_rows.get(row + 1),
                            Some(MessageListRow::Message { grouped: true, .. })
                        );
                        let row_gap = if next_grouped { 0.0 } else { gap };
                        metrics.set_height(row, response.response.rect.height() + row_gap);
                    }
                }
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

/// One row in the virtualised message list. Date and NEW dividers participate
/// in the same measured layout as messages, so scrolling and search jumps keep
/// their offsets.
#[derive(Debug, PartialEq, Eq)]
enum MessageListRow {
    Date { label: String },
    New,
    Message { index: usize, grouped: bool },
}

/// Expand a channel's message indices into display rows, inserting a date
/// divider whenever the local calendar day changes and a NEW divider immediately
/// before the first unread message. Pure so divider placement is testable.
fn build_message_rows(
    visible: &[usize],
    messages: &[Message],
    grouped: &[bool],
    new_divider: Option<&str>,
    now: DateTime<FixedOffset>,
) -> Vec<MessageListRow> {
    let mut rows = Vec::with_capacity(visible.len() + 2);
    let mut last_day = None;
    let mut new_placed = false;

    for (position, index) in visible.iter().copied().enumerate() {
        let Some(message) = messages.get(index) else {
            continue;
        };
        let day = message.timestamp.with_timezone(now.offset()).date_naive();
        let mut starts_block = false;
        if last_day != Some(day) {
            rows.push(MessageListRow::Date {
                label: date_divider_label(message.timestamp, now),
            });
            last_day = Some(day);
            starts_block = true;
        }
        if !new_placed && new_divider == Some(message.id.as_str()) {
            rows.push(MessageListRow::New);
            new_placed = true;
            starts_block = true;
        }
        rows.push(MessageListRow::Message {
            index,
            grouped: !starts_block && grouped.get(position).copied().unwrap_or(false),
        });
    }

    rows
}

/// Draw one centered divider. The line stops short of the label on both sides;
/// callers choose the muted date colour or the accent NEW colour.
fn divider_row(ui: &mut egui::Ui, label: &str, color: egui::Color32, height: f32) {
    let width = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), egui::FontId::proportional(11.0), color);
    let center = rect.center();
    let text_half = galley.size().x / 2.0;
    let gap = 10.0;
    let stroke = egui::Stroke::new(1.0, color.gamma_multiply(0.35));
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 8.0, center.y),
            egui::pos2(center.x - text_half - gap, center.y),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(center.x + text_half + gap, center.y),
            egui::pos2(rect.right() - 8.0, center.y),
        ],
        stroke,
    );
    ui.painter().galley(
        egui::pos2(center.x - text_half, center.y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// A single message row, separated so the virtualised list can measure it.
fn message_row(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    message: &Message,
    action: &mut Option<RowAction>,
    grouped: bool,
) {
    // Reserve a shape before the row content so the hover fill paints behind it
    // instead of covering the text (Discord highlights the whole message row).
    let hover = ui.painter().add(egui::Shape::Noop);
    let response = ui
        // A clickable container so a right-click anywhere on the row opens the
        // context menu; the widgets inside still get their own clicks first.
        .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
            message_row_body(app, ui, message, action, grouped);
        })
        .response;
    if response.hovered() {
        ui.painter().set(
            hover,
            egui::Shape::rect_filled(
                response.rect.expand2(egui::vec2(6.0, 2.0)),
                egui::CornerRadius::same(4),
                crate::utils::lift(app.plugins.theme().background, -8),
            ),
        );
    }

    response.context_menu(|ui| {
        message_menu(ui, app, message, action);
    });

    hover_action_bar(app, ui, message, response.rect, action);
}

/// Discord's hover toolbar: react, reply and more, floating inside the row's
/// top-right corner. It is drawn after the row body so it receives the pointer,
/// and it stays alive while either of its popups is open even though the pointer
/// has left the row.
fn hover_action_bar(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    message: &Message,
    row: egui::Rect,
    action: &mut Option<RowAction>,
) {
    // `response.rect` only spans the message's content. Discord anchors the bar
    // at the channel's right edge, and hovering the empty part of the row still
    // counts, so extend the geometry to the row's full available width.
    let row = egui::Rect::from_min_max(row.min, egui::pos2(ui.max_rect().right(), row.max.y));
    let Some((bar, buttons)) = hover_action_rects(row) else {
        return;
    };
    let theme = app.plugins.theme();

    let react_id = egui::Id::unique(("message-react", message.id.as_str()));
    let reply_id = egui::Id::unique(("message-reply", message.id.as_str()));
    let more_id = egui::Id::unique(("message-more", message.id.as_str()));
    let react_open = egui::Popup::is_id_open(ui.ctx(), react_id.with("popup"));
    let more_open = egui::Popup::is_id_open(ui.ctx(), more_id.with("popup"));
    // The bar is only shown while the pointer is somewhere on the row. Once it
    // is visible its own rect is inside the row, so moving onto a button does
    // not dismiss it; an open popup keeps it alive beyond that.
    let pointer_over_bar = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pointer| bar.contains(pointer));
    let pointer_over_row = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pointer| row.contains(pointer));
    if !(pointer_over_row || pointer_over_bar || react_open || more_open) {
        return;
    }

    ui.painter().rect_filled(bar, 6.0, theme.surface);
    ui.painter().rect_stroke(
        bar,
        6.0,
        egui::Stroke::new(1.0, crate::utils::lift(theme.surface, 18)),
        egui::StrokeKind::Inside,
    );

    let icon_size = (buttons[0].height() * 0.75).clamp(10.0, 15.0);
    let icons = ["\u{1f642}", "\u{21a9}", "\u{22ef}"];
    let names = ["Add reaction", "Reply", "More"];
    let ids = [react_id, reply_id, more_id];
    let mut responses = Vec::with_capacity(3);
    for ((rect, icon), (name, id)) in buttons.iter().zip(icons).zip(names.iter().zip(ids)) {
        let response = ui
            .interact(*rect, id, egui::Sense::click())
            .on_hover_text(*name);
        if response.hovered() {
            ui.painter()
                .rect_filled(rect.shrink(1.0), 4.0, crate::utils::lift(theme.surface, 22));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(icon_size),
            if response.hovered() {
                theme.text
            } else {
                theme.muted
            },
        );
        responses.push(response);
    }

    // Quick reactions, in the same order as the right-click menu.
    egui::Popup::menu(&responses[0]).show(|ui| {
        quick_reactions(ui, message, action);
    });
    if responses[1].clicked() {
        *action = Some(RowAction::Reply(message.id.clone()));
    }
    egui::Popup::menu(&responses[2]).show(|ui| {
        message_menu(ui, app, message, action);
    });
}

/// The floating bar and its three hit rects, kept inside the row and clear of a
/// very narrow window. Returning `None` means the row simply has no toolbar.
fn hover_action_rects(row: egui::Rect) -> Option<(egui::Rect, [egui::Rect; 3])> {
    const BAR_WIDTH: f32 = 90.0;
    const BAR_HEIGHT: f32 = 28.0;
    const BUTTON_WIDTH: f32 = 26.0;
    const PAD: f32 = 6.0;
    if row.width() < BAR_WIDTH + 24.0 || row.height() < 16.0 {
        return None;
    }
    // Grouped continuation rows can be only one text line tall. Shrink the bar
    // to stay inside so adjacent rows cannot both show an overlapping toolbar.
    let bar_height = BAR_HEIGHT.min(row.height() - 2.0).max(16.0);
    let bar = egui::Rect::from_min_size(
        egui::pos2(
            row.right() - BAR_WIDTH - 8.0,
            row.top() + (row.height() - bar_height) / 2.0,
        ),
        egui::vec2(BAR_WIDTH, bar_height),
    );
    let button_height = (bar_height - 4.0).max(10.0);
    let button_top = bar.top() + (bar_height - button_height) / 2.0;
    let mut buttons = [egui::Rect::NOTHING; 3];
    for (index, rect) in buttons.iter_mut().enumerate() {
        *rect = egui::Rect::from_min_size(
            egui::pos2(bar.left() + PAD + index as f32 * BUTTON_WIDTH, button_top),
            egui::vec2(BUTTON_WIDTH, button_height),
        );
    }
    Some((bar, buttons))
}

/// The four emoji Discord puts first in its reaction picker. Shared by the
/// right-click menu and the hover bar's reaction button.
fn quick_reactions(ui: &mut egui::Ui, message: &Message, action: &mut Option<RowAction>) {
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
}

/// Everything the right-click context menu and the hover bar's "more" button
/// offer. Keeping one function means the two menus cannot drift apart.
fn message_menu(
    ui: &mut egui::Ui,
    app: &RustcordApp,
    message: &Message,
    action: &mut Option<RowAction>,
) {
    ui.set_min_width(190.0);
    quick_reactions(ui, message, action);
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
                if attachment.is_image() && !attachment.url.is_empty() {
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

const PROFILE_IMAGE_REQUESTS: &str = "rustcord-profile-image-requests";

fn queue_profile_image(ctx: &egui::Context, url: &str) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<String>>(egui::Id::unique(PROFILE_IMAGE_REQUESTS))
            .push(url.to_string());
    });
}

/// Take the profile-card images requested while drawing this frame. The app
/// feeds these to the image loader once all UI closures have finished borrowing
/// it, so the cache is not mutated from inside a hover card.
pub(crate) fn take_profile_image_requests(ctx: &egui::Context) -> Vec<String> {
    ctx.data_mut(|data| {
        std::mem::take(
            data.get_temp_mut_or_default::<Vec<String>>(egui::Id::unique(PROFILE_IMAGE_REQUESTS)),
        )
    })
}

/// The texture for `url` at the current frame. Animated CDN assets keep the
/// client awake only while they are actually on screen.
pub(crate) fn animated_texture<'a>(
    app: &'a RustcordApp,
    ui: &egui::Ui,
    url: &str,
) -> Option<&'a egui::TextureHandle> {
    let now = ui.input(|input| input.time);
    let texture = app.images.frame(url, now);
    if app.images.is_animated(url) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
    }
    texture
}

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

/// Discord's presence dot, drawn on the avatar's bottom-right corner with a ring
/// in the panel colour so it reads against any avatar. Idle and DND use the same
/// amber/red as the official client; offline is the muted grey.
fn presence_dot_color(theme: &crate::ui::theme::AppTheme, status: Presence) -> egui::Color32 {
    match status {
        Presence::Online => theme.success,
        Presence::Idle => egui::Color32::from_rgb(0xf0, 0xb2, 0x32),
        Presence::Dnd => theme.danger,
        Presence::Offline => theme.muted,
    }
}

/// A member-list avatar with its presence dot. Message rows keep the plain avatar:
/// Discord only marks presence in the roster and profile, not beside every message.
fn avatar_with_status(
    app: &RustcordApp,
    ui: &mut egui::Ui,
    user: &User,
    size: f32,
) -> egui::Response {
    let response = avatar(app, ui, user, size);
    if !size.is_finite() || size < 12.0 {
        return response;
    }

    let theme = app.plugins.theme();
    let rect = response.rect;
    let radius = (size * 0.17).clamp(4.0, 7.0);
    let center = rect.right_bottom() - egui::vec2(size * 0.10, size * 0.10);
    let painter = ui.painter();
    painter.circle_filled(center, radius + 2.0, theme.surface);
    painter.circle_filled(
        center,
        radius,
        presence_dot_color(theme, user.status.clone()),
    );
    response
}

/// The avatar without the hover card, so the card can draw one without nesting
/// another card inside itself.
fn avatar_visual(app: &RustcordApp, ui: &mut egui::Ui, user: &User, size: f32) -> egui::Response {
    let theme = app.plugins.theme();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());

    let avatar_url = user.avatar_url();
    match avatar_url
        .as_deref()
        .and_then(|url| animated_texture(app, ui, url))
    {
        Some(texture) => {
            let image = egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                .corner_radius(egui::CornerRadius::same((size / 2.0) as u8));
            ui.put(rect, image);
        }
        None => {
            // The numbered default is preloaded, so it appears on the first frame
            // while a custom avatar downloads — no initial flash. Only an id that
            // cannot map to a Discord slot (demo placeholders) keeps the initial.
            let index = user.default_avatar_index();
            if let Some(texture) = index.and_then(|index| app.images.default_avatar(index)) {
                ui.put(
                    rect,
                    egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                        .corner_radius(egui::CornerRadius::same((size / 2.0) as u8)),
                );
            } else {
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
    let banner_url = user.banner_url();
    if let Some(url) = banner_url.as_deref() {
        // Banners can be several megabytes of GIF frames. Queue one only while
        // its profile card is open instead of preloading every visible author.
        if app.images.peek(url).is_none() {
            queue_profile_image(ui.ctx(), url);
        }
    }
    match banner_url
        .as_deref()
        .and_then(|url| animated_texture(app, ui, url))
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

/// One fixed-height row in the virtualised member panel. Headers and members
/// share a height so `ScrollArea::show_rows` can calculate offsets without
/// measuring every user.
enum MemberListRow<'a> {
    Header {
        title: String,
        count: usize,
        color: Option<egui::Color32>,
    },
    Member(&'a crate::models::User),
}

/// One role/status section in the member panel. Headers own their labels
/// because role names come from the guild payload and the virtual row list
/// outlives the borrow of that payload.
struct MemberSection<'a> {
    title: String,
    color: Option<egui::Color32>,
    members: Vec<&'a crate::models::User>,
}

const MEMBER_ROW_HEIGHT: f32 = 34.0;

/// Build Discord's member-list sections: members grouped by their highest
/// hoisted role, with ONLINE/OFFLINE catch-alls for members without one.
/// Non-hoisted roles are intentionally not sections, matching Discord. Role
/// sections include offline members because this client has no live presence
/// for a REST roster; their status dot still distinguishes them. With no roles
/// (a failed or unavailable guild payload) this degrades to the old
/// ONLINE/OFFLINE view.
fn member_sections<'a>(
    members: &[&'a crate::models::User],
    roles: &[crate::models::Role],
) -> Vec<MemberSection<'a>> {
    use std::collections::{BTreeMap, HashMap};

    let by_id: HashMap<&str, &crate::models::Role> =
        roles.iter().map(|role| (role.id.as_str(), role)).collect();
    let mut grouped: BTreeMap<&str, Vec<&'a crate::models::User>> = BTreeMap::new();
    let mut ungrouped_online = Vec::new();
    let mut offline = Vec::new();

    for member in members.iter().copied() {
        let highest = member
            .role_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|role| role.hoist)
            .max_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));
        match highest {
            Some(role) => grouped.entry(role.id.as_str()).or_default().push(member),
            None if member.status == crate::models::Presence::Offline => offline.push(member),
            None => ungrouped_online.push(member),
        }
    }

    let mut role_groups: Vec<(&crate::models::Role, Vec<&crate::models::User>)> = grouped
        .into_iter()
        .filter_map(|(id, members)| by_id.get(id).map(|role| (*role, members)))
        .collect();
    role_groups.sort_by(|(a, _), (b, _)| {
        b.position
            .cmp(&a.position)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut sections = Vec::with_capacity(role_groups.len() + 2);
    for (role, members) in role_groups {
        let title = if role.name.is_empty() {
            "Unnamed role".to_string()
        } else {
            role.name.clone()
        };
        let color = (role.color & 0x00ff_ffff != 0).then(|| {
            egui::Color32::from_rgb(
                ((role.color >> 16) & 0xff) as u8,
                ((role.color >> 8) & 0xff) as u8,
                (role.color & 0xff) as u8,
            )
        });
        sections.push(MemberSection {
            title,
            color,
            members,
        });
    }
    if !ungrouped_online.is_empty() {
        sections.push(MemberSection {
            title: "ONLINE".to_string(),
            color: None,
            members: ungrouped_online,
        });
    }
    if !offline.is_empty() {
        sections.push(MemberSection {
            title: "OFFLINE".to_string(),
            color: None,
            members: offline,
        });
    }
    sections
}

/// Merge the loaded guild roster with channel evidence (message authors, thread
/// participants, voice members) into one name-sorted, deduplicated list. Roster
/// records win because they carry avatars, names and cosmetics; extras supply
/// people missing from a capped or unavailable roster.
fn channel_member_rows<'a>(
    roster: &'a [crate::models::User],
    extras: &'a [crate::models::User],
) -> Vec<&'a crate::models::User> {
    let mut seen = std::collections::BTreeSet::new();
    let mut rows: Vec<&crate::models::User> = roster
        .iter()
        .chain(extras.iter())
        .filter(|user| !user.id.is_empty() && seen.insert(user.id.as_str()))
        .collect();
    rows.sort_by(|a, b| {
        a.username
            .to_lowercase()
            .cmp(&b.username.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_picker_choices_are_unique_and_nonempty() {
        let unique: std::collections::HashSet<&str> = EMOJI_CHOICES.iter().copied().collect();
        assert_eq!(unique.len(), EMOJI_CHOICES.len());
        assert!(EMOJI_CHOICES.iter().all(|emoji| !emoji.is_empty()));
        assert!(EMOJI_CHOICES.len() >= 100);
    }

    #[test]
    fn emoji_insertion_uses_the_caret_and_replaces_a_selection() {
        let mut draft = "héllo".to_string();
        let caret = insert_at_char_range(&mut draft, "X", Some(2..2));
        assert_eq!(draft, "héXllo");
        assert_eq!(caret, 3);

        let mut draft = "héllo wörld".to_string();
        let caret = insert_at_char_range(&mut draft, "😀", Some(1..3));
        assert_eq!(draft, "h😀lo wörld");
        assert_eq!(caret, 2);

        let caret = insert_at_char_range(&mut draft, "!", None);
        assert_eq!(draft, "h😀lo wörld!");
        assert_eq!(caret, draft.chars().count());
    }

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

    fn message_at(id: &str, timestamp: DateTime<Utc>) -> Message {
        let mut message = Message::new(crate::models::User::default(), "");
        message.id = id.to_string();
        message.timestamp = timestamp;
        message
    }

    #[test]
    fn date_divider_labels_follow_the_local_calendar() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T21:04:00Z")
            .expect("now")
            .fixed_offset();
        assert_eq!(
            date_divider_label(utc("2026-11-12T09:05:00Z"), now),
            "Today"
        );
        assert_eq!(
            date_divider_label(utc("2026-11-11T23:59:00Z"), now),
            "Yesterday"
        );
        assert_eq!(
            date_divider_label(utc("2026-01-03T07:00:00Z"), now),
            "3 January"
        );
        assert_eq!(
            date_divider_label(utc("2025-12-31T23:00:00Z"), now),
            "31 December 2025"
        );
    }

    #[test]
    fn message_rows_add_date_dividers_and_a_new_separator() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T21:04:00Z")
            .expect("now")
            .fixed_offset();
        let older = message_at("older", utc("2026-11-11T23:00:00Z"));
        let first = message_at("first", utc("2026-11-12T09:00:00Z"));
        let second = message_at("second", utc("2026-11-12T09:01:00Z"));
        let messages = [older, first, second];

        let rows = build_message_rows(
            &[0, 1, 2],
            &messages,
            &[false, false, true],
            Some("second"),
            now,
        );

        assert_eq!(rows.len(), 6);
        assert!(matches!(&rows[0], MessageListRow::Date { label } if label == "Yesterday"));
        assert!(matches!(rows[1], MessageListRow::Message { index: 0, .. }));
        assert!(matches!(&rows[2], MessageListRow::Date { label } if label == "Today"));
        assert!(matches!(rows[3], MessageListRow::Message { index: 1, .. }));
        assert_eq!(rows[4], MessageListRow::New);
        // A divider starts a new block, so the first row after it keeps its
        // author header even when the caller thought it was a continuation.
        assert!(matches!(
            rows[5],
            MessageListRow::Message {
                index: 2,
                grouped: false
            }
        ));
        // The NEW marker only appears once, even if the id is repeated in the
        // caller's row list.
        assert!(
            rows[1..]
                .iter()
                .filter(|row| **row == MessageListRow::New)
                .count()
                == 1
        );
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
    fn presence_dots_use_distinct_status_colours() {
        let theme = crate::ui::theme::AppTheme::default();
        let online = presence_dot_color(&theme, Presence::Online);
        let idle = presence_dot_color(&theme, Presence::Idle);
        let dnd = presence_dot_color(&theme, Presence::Dnd);
        let offline = presence_dot_color(&theme, Presence::Offline);
        assert_eq!(online, theme.success);
        assert_eq!(dnd, theme.danger);
        assert_ne!(online, idle);
        assert_ne!(idle, dnd);
        assert_ne!(dnd, offline);
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
    fn hover_actions_sit_inside_the_row() {
        let row = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(420.0, 60.0));
        let (bar, buttons) = hover_action_rects(row).expect("room for a toolbar");
        assert!(row.contains(bar.min) && row.contains(bar.max));
        assert_eq!(buttons.len(), 3);
        for button in buttons {
            assert!(bar.contains(button.min) && bar.contains(button.max));
            assert!(button.width() > 0.0 && button.height() > 0.0);
        }
        assert!(buttons[0].right() <= buttons[1].left());
        assert!(buttons[1].right() <= buttons[2].left());
    }

    #[test]
    fn a_short_grouped_row_still_has_hover_actions() {
        let row = egui::Rect::from_min_size(egui::pos2(20.0, 10.0), egui::vec2(320.0, 20.0));
        let (bar, _) = hover_action_rects(row).expect("compact toolbar");
        assert!(row.contains(bar.min) && row.contains(bar.max));
    }

    #[test]
    fn a_narrow_row_has_no_hover_actions() {
        let row = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 40.0));
        assert!(hover_action_rects(row).is_none());
    }

    fn member(id: &str, name: &str) -> crate::models::User {
        crate::models::User {
            id: id.to_string(),
            username: name.to_string(),
            ..crate::models::User::default()
        }
    }

    #[test]
    fn member_panel_groups_members_by_highest_hoisted_role() {
        let roles = vec![
            crate::models::Role {
                id: "admin".to_string(),
                name: "Admin".to_string(),
                color: 0xe74c3c,
                hoist: true,
                position: 30,
            },
            crate::models::Role {
                id: "mod".to_string(),
                name: "Moderator".to_string(),
                color: 0,
                hoist: true,
                position: 20,
            },
            crate::models::Role {
                id: "colour".to_string(),
                name: "Colour".to_string(),
                color: 0x3498db,
                hoist: false,
                position: 40,
            },
        ];
        let mut alice = member("1", "alice");
        alice.status = Presence::Online;
        alice.role_ids = vec!["mod".to_string(), "admin".to_string()];
        let mut bob = member("2", "bob");
        bob.status = Presence::Idle;
        bob.role_ids = vec!["colour".to_string()];
        let mut carol = member("3", "carol");
        carol.role_ids = vec!["admin".to_string()];
        let mut dave = member("4", "dave");
        dave.status = Presence::Dnd;
        let eve = member("5", "eve");

        let members = [&alice, &bob, &carol, &dave, &eve];
        let sections = member_sections(&members, &roles);
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        // Admin outranks Moderator; Colour is not hoisted, so Bob falls into
        // ONLINE alongside Dave. Carol is offline but still belongs to Admin;
        // Eve, who has no hoisted role, is the only OFFLINE row.
        assert_eq!(titles, ["Admin", "ONLINE", "OFFLINE"]);
        assert_eq!(
            sections[0]
                .members
                .iter()
                .map(|user| user.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "3"]
        );
        assert_eq!(
            sections[0].color,
            Some(egui::Color32::from_rgb(0xe7, 0x4c, 0x3c))
        );
        assert_eq!(
            sections[1]
                .members
                .iter()
                .map(|user| user.id.as_str())
                .collect::<Vec<_>>(),
            ["2", "4"]
        );
        assert_eq!(
            sections[2]
                .members
                .iter()
                .map(|user| user.id.as_str())
                .collect::<Vec<_>>(),
            ["5"]
        );
    }

    #[test]
    fn member_panel_falls_back_to_presence_without_roles() {
        let mut online = member("1", "alice");
        online.status = Presence::Online;
        let offline = member("2", "bob");
        let members = [&online, &offline];

        let sections = member_sections(&members, &[]);
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(titles, ["ONLINE", "OFFLINE"]);
        assert_eq!(sections[0].members.len(), 1);
        assert_eq!(sections[1].members.len(), 1);
    }

    #[test]
    fn scoped_rows_include_silent_roster_members_and_extra_participants() {
        let roster = vec![member("1", "zoe"), member("2", "alice")];
        let extras = vec![
            // Already in the roster: the roster's record wins, but no duplicate row.
            member("2", "alice"),
            // A thread participant missing from the capped roster.
            member("3", "bob"),
        ];
        let rows = channel_member_rows(&roster, &extras);
        let ids: Vec<&str> = rows.iter().map(|user| user.id.as_str()).collect();
        assert_eq!(ids, ["2", "3", "1"]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn profile_image_requests_are_queued_and_drained_once() {
        let ctx = egui::Context::default();
        let url = "https://cdn.discordapp.com/banners/1/a_banner.gif?size=240";
        queue_profile_image(&ctx, url);
        queue_profile_image(&ctx, url);
        assert_eq!(take_profile_image_requests(&ctx), vec![url, url]);
        assert!(take_profile_image_requests(&ctx).is_empty());
    }

    #[test]
    fn missing_dimensions_still_produce_a_placeholder() {
        let size = fitted_size(300.0, egui::vec2(0.0, 0.0));
        assert!(size.x > 0.0 && size.y > 0.0);
    }
}
