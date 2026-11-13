use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub avatar: Option<String>,
    /// Discord's display name (Nitro users can set one independent of the handle).
    #[serde(default)]
    pub global_name: Option<String>,
    /// Banner hash. Nitro users get an image; everyone else can have an accent.
    #[serde(default)]
    pub banner: Option<String>,
    /// Profile accent colour as packed `0xRRGGBB`.
    #[serde(default)]
    pub accent_color: Option<u32>,
    /// Avatar-decoration preset asset hash (the Nitro ornament around the avatar).
    #[serde(default)]
    pub avatar_decoration: Option<String>,
    #[serde(default)]
    pub status: Presence,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: "0".to_string(),
            username: "Unknown".to_string(),
            avatar: None,
            global_name: None,
            banner: None,
            accent_color: None,
            avatar_decoration: None,
            status: Presence::Offline,
        }
    }
}

impl User {
    /// The name Discord shows: the display name when set, else the handle.
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    /// CDN URL for the avatar. Animated hashes (`a_…`) are fetched as PNG so the
    /// decoder never needs a frame clock; Discord serves a still frame for them.
    /// Users without a custom avatar fall back to Discord's numbered default
    /// avatar, exactly as the official client does.
    pub fn avatar_url(&self) -> Option<String> {
        if let Some(hash) = &self.avatar {
            return Some(format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=80",
                self.id
            ));
        }
        let id: u64 = self.id.parse().ok()?;
        let index = (id >> 22) % 6;
        Some(format!(
            "https://cdn.discordapp.com/embed/avatars/{index}.png"
        ))
    }

    /// CDN URL for the profile banner, when the user has one.
    pub fn banner_url(&self) -> Option<String> {
        self.banner.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/banners/{}/{hash}.png?size=480",
                self.id
            )
        })
    }

    /// CDN URL for the avatar-decoration preset (Nitro ornament).
    pub fn decoration_url(&self) -> Option<String> {
        self.avatar_decoration.as_ref().map(|asset| {
            format!("https://cdn.discordapp.com/avatar-decoration-presets/{asset}.png")
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Text,
    Voice,
    Category,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub guild_id: String,
}

/// How many reacting users are remembered per emoji. `count` stays
/// authoritative (Discord reports it); the list exists so a repeated event from
/// the same user cannot be counted twice, without unbounded growth.
pub const REACTION_USER_CAP: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    /// Whether the signed-in bot has reacted with this emoji.
    pub me: bool,
    /// Known user ids, capped at [`REACTION_USER_CAP`]. Empty for reactions that
    /// came from REST history, which does not list who reacted.
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub size: u64,
}

impl Attachment {
    /// Discord sets `content_type` for most uploads but not all of them, so the
    /// extension is the fallback.
    pub fn is_image(&self) -> bool {
        if let Some(kind) = &self.content_type {
            if kind.starts_with("image/") {
                return true;
            }
        }

        self.filename
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
    }

    /// Pixel dimensions, when Discord reported them.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match (self.width, self.height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbedMedia {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Embed {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Discord sends the accent colour as a 24-bit integer.
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub image: Option<EmbedMedia>,
    #[serde(default)]
    pub thumbnail: Option<EmbedMedia>,
}

impl Embed {
    pub fn image_url(&self) -> Option<&str> {
        self.image.as_ref().and_then(|media| media.url.as_deref())
    }

    pub fn thumbnail_url(&self) -> Option<&str> {
        self.thumbnail
            .as_ref()
            .and_then(|media| media.url.as_deref())
    }

    /// True when there is nothing to draw but an empty box.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.image_url().is_none()
            && self.thumbnail_url().is_none()
    }
}

/// What a message replies to: enough to draw Discord's `↳ @author snippet`
/// line and to send a `message_reference`. Discord sends this as
/// `referenced_message` (a full message) or `message_reference` (an id only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyRef {
    pub message_id: String,
    pub channel_id: Option<String>,
    pub author: String,
    pub content: String,
}

impl ReplyRef {
    pub fn new(message: &Message) -> Self {
        Self {
            message_id: message.id.clone(),
            channel_id: message.channel_id.clone(),
            author: message.author.username.clone(),
            content: message.content.clone(),
        }
    }

    /// One line, whichever characters the source used.
    pub fn preview(&self, max_chars: usize) -> String {
        snippet(&self.content, max_chars)
    }
}

/// Cut at a character boundary so multi-byte text never panics or splits a
/// glyph in half. Ellipsis included in the budget.
pub fn snippet(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out.replace('\n', " ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: Option<String>,
    pub author: User,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub embeds: Vec<Embed>,
    #[serde(default)]
    pub reply_to: Option<ReplyRef>,
}

impl Message {
    pub fn new(author: User, content: impl Into<String>) -> Self {
        Self {
            id: Utc::now().timestamp_millis().to_string(),
            channel_id: None,
            author,
            content: content.into(),
            timestamp: Utc::now(),
            reactions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            reply_to: None,
        }
    }

    /// Apply our own reaction locally, exactly as Discord will after the PUT
    /// lands. Returns `false` when we had already reacted with this emoji, in
    /// which case the API call would be a no-op too.
    pub fn add_own_reaction(&mut self, emoji: &str) -> bool {
        if let Some(reaction) = self.reactions.iter_mut().find(|it| it.emoji == emoji) {
            if reaction.me {
                return false;
            }
            reaction.me = true;
            reaction.count = reaction.count.saturating_add(1);
            return true;
        }

        self.reactions.push(Reaction {
            emoji: emoji.to_string(),
            count: 1,
            me: true,
            users: Vec::new(),
        });
        true
    }

    /// Undo `add_own_reaction`, used when the request failed.
    pub fn remove_own_reaction(&mut self, emoji: &str) {
        if let Some(index) = self
            .reactions
            .iter()
            .position(|reaction| reaction.emoji == emoji)
        {
            let reaction = &mut self.reactions[index];
            if !reaction.me {
                return;
            }
            reaction.me = false;
            reaction.count = reaction.count.saturating_sub(1);
            if reaction.count == 0 {
                self.reactions.remove(index);
            }
        }
    }

    /// Whether this message is the one an event names: always by id, and by
    /// channel too when the event carries one.
    pub fn matches_identity(&self, id: &str, channel_id: Option<&str>) -> bool {
        if self.id != id {
            return false;
        }

        match channel_id {
            Some(channel) => self.channel_id.as_deref() == Some(channel),
            None => true,
        }
    }

    /// Apply a `MESSAGE_UPDATE`.
    ///
    /// Discord's update payload is partial in practice: it always carries the
    /// message id, and only sometimes the content or embeds. Nothing is cleared
    /// because a field was absent, so an update the client cannot fully read
    /// leaves the message as it was.
    pub fn apply_edit(&mut self, content: Option<&str>, embeds: Option<Vec<Embed>>) -> bool {
        let mut changed = false;

        if let Some(content) = content {
            if content != self.content {
                self.content = content.to_string();
                changed = true;
            }
        }

        // An empty list from a partial payload is not "no embeds"; only a
        // non-empty payload replaces what is held.
        if let Some(embeds) = embeds.filter(|embeds| !embeds.is_empty()) {
            self.embeds = embeds;
            changed = true;
        }

        changed
    }

    /// Add one reaction, creating the chip when it is the first of its emoji.
    ///
    /// Returns whether anything changed: a repeated add from the same user (a
    /// resumed session replays events) is ignored rather than double-counted.
    pub fn add_reaction(&mut self, emoji: &str, user_id: Option<&str>, by_me: bool) -> bool {
        if emoji.is_empty() {
            return false;
        }

        let user = user_id.filter(|id| !id.is_empty());

        let Some(index) = self.reactions.iter().position(|r| r.emoji == emoji) else {
            let users = user.map(|id| vec![id.to_string()]).unwrap_or_default();
            self.reactions.push(Reaction {
                emoji: emoji.to_string(),
                count: 1,
                me: by_me,
                users,
            });
            return true;
        };

        let reaction = &mut self.reactions[index];
        if let Some(user) = user {
            if reaction.users.iter().any(|known| known == user) {
                return false;
            }
            if reaction.users.len() < REACTION_USER_CAP {
                reaction.users.push(user.to_string());
            }
        }

        reaction.count = reaction.count.saturating_add(1);
        if by_me {
            reaction.me = true;
        }
        true
    }

    /// Remove one reaction; the chip disappears at zero.
    ///
    /// A reaction whose user list is known only counts down for a user it lists,
    /// so a duplicated remove cannot decrement twice. Reactions from REST history
    /// carry no user list, so those count down from whatever Discord reported.
    pub fn remove_reaction(&mut self, emoji: &str, user_id: Option<&str>, by_me: bool) -> bool {
        let Some(index) = self.reactions.iter().position(|r| r.emoji == emoji) else {
            return false;
        };

        let reaction = &mut self.reactions[index];
        if let Some(user) = user_id.filter(|id| !id.is_empty()) {
            match reaction.users.iter().position(|known| known == user) {
                Some(position) => {
                    reaction.users.remove(position);
                }
                None if !reaction.users.is_empty() => return false,
                None => {}
            }
        }

        if by_me {
            reaction.me = false;
        }
        reaction.count = reaction.count.saturating_sub(1);

        if reaction.count == 0 {
            self.reactions.remove(index);
        }
        true
    }

    /// `MESSAGE_REACTION_REMOVE_ALL`. Returns whether anything was cleared.
    pub fn clear_reactions(&mut self) -> bool {
        let had_any = !self.reactions.is_empty();
        self.reactions.clear();
        had_any
    }

    /// `MESSAGE_REACTION_REMOVE_EMOJI`: only this emoji goes away.
    pub fn remove_reaction_emoji(&mut self, emoji: &str) -> bool {
        let before = self.reactions.len();
        self.reactions.retain(|reaction| reaction.emoji != emoji);
        self.reactions.len() != before
    }

    /// Every URL this message would like to display, in draw order. Used to
    /// prefetch only the rows that are on screen.
    pub fn image_urls(&self) -> impl Iterator<Item = &str> {
        self.attachments
            .iter()
            .filter(|attachment| attachment.is_image())
            .map(|attachment| attachment.url.as_str())
            .chain(self.embeds.iter().filter_map(|embed| embed.image_url()))
            .filter(|url| !url.is_empty())
    }
}

/// Apply a `MESSAGE_UPDATE` to a message list.
///
/// Returns whether a message changed. An edit for a message this client no
/// longer holds (trimmed history, or an event from before the page loaded) is
/// ignored rather than appended, so out-of-order delivery cannot resurrect a row.
pub fn apply_update(
    messages: &mut [Message],
    id: &str,
    channel_id: Option<&str>,
    content: Option<&str>,
    embeds: Option<Vec<Embed>>,
) -> bool {
    let Some(message) = messages
        .iter_mut()
        .find(|message| message.matches_identity(id, channel_id))
    else {
        return false;
    };

    message.apply_edit(content, embeds)
}

/// Remove one message from a list. Returns whether it was there.
pub fn remove_message(messages: &mut Vec<Message>, id: &str, channel_id: Option<&str>) -> bool {
    let before = messages.len();
    messages.retain(|message| !message.matches_identity(id, channel_id));
    messages.len() != before
}

/// Discord caps visible mention badges at 99+.
pub const UNREAD_BADGE_CAP: u32 = 99;

/// Messages seen since a channel was last opened, and how many of them named
/// this bot. Bots have no server-side read state, so this is local by nature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unread {
    pub messages: u32,
    pub mentions: u32,
}

impl Unread {
    pub fn is_unread(&self) -> bool {
        self.messages > 0
    }

    /// The red pill's text, or `None` when nothing mentioned us.
    pub fn badge(&self) -> Option<String> {
        if self.mentions == 0 {
            return None;
        }
        Some(if self.mentions > UNREAD_BADGE_CAP {
            format!("{UNREAD_BADGE_CAP}+")
        } else {
            self.mentions.to_string()
        })
    }
}

/// Per-channel unread counters.
#[derive(Debug, Default, Clone)]
pub struct UnreadTracker {
    channels: std::collections::BTreeMap<String, Unread>,
}

impl UnreadTracker {
    /// Count a message. The open channel is read by definition, and our own
    /// messages never count.
    pub fn observe(&mut self, channel_id: &str, is_mention: bool, is_open: bool, is_own: bool) {
        if channel_id.is_empty() || is_open || is_own {
            return;
        }

        let entry = self.channels.entry(channel_id.to_string()).or_default();
        entry.messages = entry.messages.saturating_add(1);
        if is_mention {
            entry.mentions = entry.mentions.saturating_add(1);
        }
    }

    pub fn mark_read(&mut self, channel_id: &str) {
        self.channels.remove(channel_id);
    }

    pub fn get(&self, channel_id: &str) -> Unread {
        self.channels.get(channel_id).copied().unwrap_or_default()
    }

    /// Mentions across every channel of one guild, for the rail badge.
    pub fn guild_mentions<'a>(&self, channel_ids: impl Iterator<Item = &'a str>) -> u32 {
        channel_ids
            .map(|id| self.get(id).mentions)
            .fold(0u32, u32::saturating_add)
    }

    /// Whether any channel of one guild has unread messages.
    pub fn guild_unread<'a>(&self, channel_ids: impl Iterator<Item = &'a str>) -> bool {
        channel_ids.into_iter().any(|id| self.get(id).is_unread())
    }

    pub fn clear(&mut self) {
        self.channels.clear();
    }
}

/// Whether a message body names this bot: a direct `<@id>`/`<@!id>` ping or an
/// `@everyone`/`@here` that Discord would notify for anyway.
pub fn mentions_user(content: &str, user_id: &str) -> bool {
    if content.contains("@everyone") || content.contains("@here") {
        return true;
    }
    if user_id.is_empty() {
        return false;
    }
    content.contains(&format!("<@{user_id}>")) || content.contains(&format!("<@!{user_id}>"))
}

/// Someone sitting in a voice channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceMember {
    pub user_id: String,
    pub username: String,
}

/// How many people are listed per voice channel. Discord's own limit is 99;
/// a bot in a busy stage channel should not grow this without bound.
pub const VOICE_CHANNEL_CAP: usize = 50;

/// Who is in which voice channel, kept from `VOICE_STATE_UPDATE`.
///
/// This is state only: it is what the sidebar shows. Audio is a different
/// protocol (UDP, Opus, DAVE/E2EE) and is not implemented — see `DECISIONS.md`
/// D24 rather than a mute button that does nothing.
#[derive(Debug, Default, Clone)]
pub struct VoiceTracker {
    channels: std::collections::BTreeMap<String, Vec<VoiceMember>>,
}

impl VoiceTracker {
    /// Apply one voice-state update: leave every channel, then join `channel_id`
    /// when one is given. Returns whether anything changed.
    pub fn update(&mut self, channel_id: Option<&str>, user_id: &str, username: &str) -> bool {
        if user_id.is_empty() {
            return false;
        }

        let before: Vec<(String, Vec<VoiceMember>)> = self
            .channels
            .iter()
            .map(|(id, members)| (id.clone(), members.clone()))
            .collect();

        for members in self.channels.values_mut() {
            members.retain(|member| member.user_id != user_id);
        }

        if let Some(channel_id) = channel_id.filter(|id| !id.is_empty()) {
            let members = self.channels.entry(channel_id.to_string()).or_default();
            if members.len() < VOICE_CHANNEL_CAP {
                members.push(VoiceMember {
                    user_id: user_id.to_string(),
                    username: username.to_string(),
                });
            }
        }

        self.channels.retain(|_, members| !members.is_empty());

        let after: Vec<(String, Vec<VoiceMember>)> = self
            .channels
            .iter()
            .map(|(id, members)| (id.clone(), members.clone()))
            .collect();

        before != after
    }

    pub fn members(&self, channel_id: &str) -> &[VoiceMember] {
        self.channels
            .get(channel_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Total people across every channel, for the status line.
    pub fn total(&self) -> usize {
        self.channels.values().map(Vec::len).sum()
    }

    pub fn clear(&mut self) {
        self.channels.clear();
    }
}

/// How long a typing indicator lives after the last `TYPING_START`, in seconds.
/// Discord resends the event every ~10 s while someone keeps typing, so a
/// shorter lifetime makes the line vanish promptly when they stop.
pub const TYPING_TTL: f64 = 8.0;

/// Upper bound on tracked typers. A busy guild must not be able to grow this.
pub const TYPING_CAP: usize = 32;

#[derive(Debug, Clone)]
struct TypingEntry {
    channel_id: String,
    user_id: String,
    username: String,
    expires: f64,
}

/// Who is typing where, with expiry.
///
/// Pure: callers pass the current time, so expiry is testable without sleeping
/// and the app needs no extra timer or thread.
/// Discord's typing endpoint is rate-limited to one call per ten seconds per
/// channel. Refreshing at eight keeps the indicator alive through a long message
/// without ever being the reason a call is refused.
pub const TYPING_REFRESH: std::time::Duration = std::time::Duration::from_secs(8);

/// How long one frame of the animated trailing dots is shown.
pub const TYPING_FRAME_SECONDS: f64 = 0.4;

/// The trailing dots for a given moment: `.`, `..`, `...`, then round again.
///
/// Pure and monotonic-clock friendly: callers pass egui's frame time, so the
/// animation is a function of time rather than mutable widgets. When a typing
/// label is on screen the UI asks for a repaint every 120 ms, which is enough
/// granularity for a 400 ms cycle without pinning a core.
pub fn typing_dots(now: f64) -> &'static str {
    // `floor`, not a cast: a cast truncates toward zero, so a clock at -1 s would
    // land in the wrong frame.
    let phase = (now / TYPING_FRAME_SECONDS).floor() as i64;
    match phase.rem_euclid(3) {
        0 => ".",
        1 => "..",
        _ => "...",
    }
}

/// Whether a typing notice is due: the first keystroke always sends one, a
/// different channel sends one, and the same channel waits out `TYPING_REFRESH`.
pub fn typing_due(
    last: Option<(&str, std::time::Instant)>,
    channel_id: &str,
    now: std::time::Instant,
) -> bool {
    match last {
        None => true,
        Some((id, sent)) => id != channel_id || now.duration_since(sent) >= TYPING_REFRESH,
    }
}

/// How long until the same channel's notice may be refreshed, or `None` when no
/// notice was sent for this channel. `Some(0)` means it is due now.
///
/// Separated from `typing_due` so the composer can schedule a wake-up exactly when
/// a long, still-focused draft needs its notice renewed, without polling.
pub fn typing_refresh_in(
    last: Option<(&str, std::time::Instant)>,
    channel_id: &str,
    now: std::time::Instant,
) -> Option<std::time::Duration> {
    match last {
        Some((id, sent)) if id == channel_id => {
            Some(TYPING_REFRESH.saturating_sub(now.saturating_duration_since(sent)))
        }
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct TypingTracker {
    entries: Vec<TypingEntry>,
}

impl TypingTracker {
    pub fn record(&mut self, channel_id: &str, user_id: &str, username: &str, now: f64) {
        self.prune(now);

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.channel_id == channel_id && entry.user_id == user_id)
        {
            entry.username = username.to_string();
            entry.expires = now + TYPING_TTL;
            return;
        }

        if self.entries.len() >= TYPING_CAP {
            // Drop whichever would expire first; that is the stalest typist.
            if let Some(position) = self
                .entries
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.expires
                        .partial_cmp(&b.expires)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(index, _)| index)
            {
                self.entries.remove(position);
            }
        }

        self.entries.push(TypingEntry {
            channel_id: channel_id.to_string(),
            user_id: user_id.to_string(),
            username: username.to_string(),
            expires: now + TYPING_TTL,
        });
    }

    pub fn prune(&mut self, now: f64) {
        self.entries.retain(|entry| entry.expires > now);
    }

    /// The names typing in a channel, or `None` when nobody is.
    ///
    /// No trailing ellipsis: the dot animation is appended by the UI from
    /// [`typing_dots`], so the same sentence serves the composer and the header.
    pub fn summary(&mut self, channel_id: &str, now: f64) -> Option<String> {
        self.prune(now);

        let names: Vec<&str> = self
            .entries
            .iter()
            .filter(|entry| entry.channel_id == channel_id)
            .map(|entry| entry.username.as_str())
            .collect();

        match names.len() {
            0 => None,
            1 => Some(format!("{} is typing", names[0])),
            2 => Some(format!("{} and {} are typing", names[0], names[1])),
            _ => Some("several people are typing".to_string()),
        }
    }

    pub fn forget_channel(&mut self, channel_id: &str) {
        self.entries.retain(|entry| entry.channel_id != channel_id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_with(content: &str) -> Message {
        Message::new(User::default(), content)
    }

    #[test]
    fn an_edit_replaces_content_and_reports_the_change() {
        let mut message = message_with("hello");
        assert!(message.apply_edit(Some("hello there"), None));
        assert_eq!(message.content, "hello there");
        // The same text again is not a change.
        assert!(!message.apply_edit(Some("hello there"), None));
    }

    #[test]
    fn a_partial_update_leaves_untouched_fields_alone() {
        let mut message = message_with("hello");
        message.embeds.push(Embed {
            title: Some("kept".to_string()),
            ..Embed::default()
        });

        assert!(!message.apply_edit(None, None));
        assert_eq!(message.content, "hello");
        assert_eq!(message.embeds.len(), 1);

        // An empty embed list from a partial payload is not "no embeds".
        assert!(!message.apply_edit(None, Some(Vec::new())));
        assert_eq!(message.embeds.len(), 1);
    }

    #[test]
    fn updated_embeds_replace_the_previous_ones() {
        let mut message = message_with("");
        message.embeds.push(Embed::default());

        assert!(message.apply_edit(
            None,
            Some(vec![Embed {
                title: Some("new".to_string()),
                ..Embed::default()
            }])
        ));
        assert_eq!(message.embeds.len(), 1);
        assert_eq!(message.embeds[0].title.as_deref(), Some("new"));
    }

    #[test]
    fn the_first_reaction_creates_a_chip() {
        let mut message = message_with("");
        assert!(message.add_reaction("🔥", Some("1"), false));
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert_eq!(message.reactions[0].users, vec!["1".to_string()]);
    }

    #[test]
    fn repeated_adds_from_one_user_do_not_double_count() {
        let mut message = message_with("");
        message.add_reaction("🔥", Some("1"), false);

        // A resumed session replays events; the second add is not a new reaction.
        assert!(!message.add_reaction("🔥", Some("1"), false));
        assert_eq!(message.reactions[0].count, 1);

        assert!(message.add_reaction("🔥", Some("2"), false));
        assert_eq!(message.reactions[0].count, 2);
    }

    #[test]
    fn a_bot_reaction_marks_the_chip_as_ours() {
        let mut message = message_with("");
        message.add_reaction("🔥", Some("1"), false);
        message.add_reaction("🔥", Some("bot"), true);
        assert!(message.reactions[0].me);

        message.remove_reaction("🔥", Some("bot"), true);
        assert!(!message.reactions[0].me);
        assert_eq!(message.reactions[0].count, 1);
    }

    #[test]
    fn removing_the_last_reaction_drops_the_chip() {
        let mut message = message_with("");
        message.add_reaction("🔥", Some("1"), false);
        assert!(message.remove_reaction("🔥", Some("1"), false));
        assert!(message.reactions.is_empty());
    }

    #[test]
    fn a_duplicate_remove_does_not_decrement_twice() {
        let mut message = message_with("");
        message.add_reaction("🔥", Some("1"), false);
        message.add_reaction("🔥", Some("2"), false);

        assert!(message.remove_reaction("🔥", Some("1"), false));
        assert!(!message.remove_reaction("🔥", Some("1"), false));
        assert_eq!(message.reactions[0].count, 1);
    }

    #[test]
    fn reactions_from_history_still_count_down() {
        let mut message = message_with("");
        message.reactions.push(Reaction {
            emoji: "🔥".to_string(),
            count: 3,
            me: false,
            // REST history does not list who reacted.
            users: Vec::new(),
        });

        assert!(message.remove_reaction("🔥", Some("9"), false));
        assert_eq!(message.reactions[0].count, 2);
    }

    #[test]
    fn unknown_emoji_removals_are_ignored() {
        let mut message = message_with("");
        assert!(!message.remove_reaction("🔥", Some("1"), false));
        assert!(!message.remove_reaction_emoji("🔥"));
        assert!(!message.clear_reactions());
    }

    #[test]
    fn remove_emoji_takes_one_and_remove_all_takes_the_rest() {
        let mut message = message_with("");
        message.add_reaction("🔥", Some("1"), false);
        message.add_reaction("🎨", Some("2"), false);

        assert!(message.remove_reaction_emoji("🔥"));
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].emoji, "🎨");

        assert!(message.clear_reactions());
        assert!(message.reactions.is_empty());
    }

    #[test]
    fn identity_needs_the_id_and_any_named_channel() {
        let mut message = message_with("");
        message.id = "100".to_string();
        message.channel_id = Some("300".to_string());

        assert!(message.matches_identity("100", None));
        assert!(message.matches_identity("100", Some("300")));
        assert!(!message.matches_identity("100", Some("301")));
        assert!(!message.matches_identity("101", None));
    }

    #[test]
    fn an_update_for_an_unknown_message_changes_nothing() {
        let mut messages = vec![message_with("hello")];
        messages[0].id = "100".to_string();

        assert!(!apply_update(
            &mut messages,
            "999",
            None,
            Some("stranger"),
            None
        ));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn an_update_for_the_wrong_channel_is_ignored() {
        let mut messages = vec![message_with("hello")];
        messages[0].id = "100".to_string();
        messages[0].channel_id = Some("300".to_string());

        assert!(!apply_update(
            &mut messages,
            "100",
            Some("301"),
            Some("nope"),
            None
        ));
        assert_eq!(messages[0].content, "hello");
        assert!(apply_update(
            &mut messages,
            "100",
            Some("300"),
            Some("yes"),
            None
        ));
        assert_eq!(messages[0].content, "yes");
    }

    #[test]
    fn deleting_an_unknown_message_reports_no_change() {
        let mut messages = vec![message_with("hello")];
        messages[0].id = "100".to_string();

        assert!(!remove_message(&mut messages, "999", None));
        assert_eq!(messages.len(), 1);

        assert!(remove_message(&mut messages, "100", None));
        assert!(messages.is_empty());

        // A replayed delete of the same id is not an error.
        assert!(!remove_message(&mut messages, "100", None));
    }

    #[test]
    fn deleting_from_the_wrong_channel_keeps_the_message() {
        let mut messages = vec![message_with("hello")];
        messages[0].id = "100".to_string();
        messages[0].channel_id = Some("300".to_string());

        assert!(!remove_message(&mut messages, "100", Some("301")));
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn unread_counts_and_ignores_the_open_channel() {
        let mut unread = UnreadTracker::default();

        unread.observe("10", false, false, false);
        unread.observe("10", true, false, false);
        unread.observe("20", false, false, false);

        assert_eq!(
            unread.get("10"),
            Unread {
                messages: 2,
                mentions: 1
            }
        );
        assert_eq!(unread.get("20").mentions, 0);

        // The open channel is being read; our own messages never count.
        unread.observe("10", true, true, false);
        unread.observe("10", true, false, true);
        assert_eq!(unread.get("10").messages, 2);
    }

    #[test]
    fn opening_a_channel_clears_its_counters() {
        let mut unread = UnreadTracker::default();
        unread.observe("10", true, false, false);
        unread.mark_read("10");
        assert!(!unread.get("10").is_unread());
        assert!(unread.get("10").badge().is_none());
    }

    #[test]
    fn badges_cap_at_ninety_nine() {
        let mut unread = UnreadTracker::default();
        for _ in 0..120 {
            unread.observe("10", true, false, false);
        }
        assert_eq!(unread.get("10").badge().as_deref(), Some("99+"));
    }

    #[test]
    fn guild_totals_add_up() {
        let mut unread = UnreadTracker::default();
        unread.observe("1", true, false, false);
        unread.observe("2", true, false, false);
        unread.observe("3", false, false, false);

        let ids = ["1", "2", "3"];
        assert_eq!(unread.guild_mentions(ids.iter().copied()), 2);
        assert!(unread.guild_unread(ids.iter().copied()));
        assert_eq!(unread.guild_mentions(["9"].iter().copied()), 0);
    }

    #[test]
    fn mentions_recognise_direct_pings_and_everyone() {
        assert!(mentions_user("hi <@42>", "42"));
        assert!(mentions_user("hi <@!42>", "42"));
        assert!(mentions_user("@everyone look", "42"));
        assert!(mentions_user("@here look", "42"));
        assert!(!mentions_user("hi <@420>", "42"));
        assert!(!mentions_user("plain", "42"));
        assert!(!mentions_user("<@42>", ""));
    }

    #[test]
    fn snippet_is_char_safe() {
        assert_eq!(snippet("hello", 10), "hello");
        assert_eq!(snippet("hello world", 5), "hello…");
        // Each emoji is one character, four bytes.
        assert_eq!(
            snippet("\u{1f525}\u{1f525}\u{1f525}", 2),
            "\u{1f525}\u{1f525}…"
        );
        assert_eq!(snippet("two\nlines", 20), "two lines");
    }

    #[test]
    fn a_reply_ref_captures_the_target() {
        let mut message = message_with("original");
        message.author.username = "tester".to_string();
        message.id = "42".to_string();
        message.channel_id = Some("10".to_string());

        let reply = ReplyRef::new(&message);
        assert_eq!(reply.message_id, "42");
        assert_eq!(reply.channel_id.as_deref(), Some("10"));
        assert_eq!(reply.author, "tester");
        assert_eq!(reply.preview(40), "original");
    }

    #[test]
    fn own_reaction_is_added_once_and_removed() {
        let mut message = message_with("hi");

        assert!(message.add_own_reaction("\u{1f44d}"));
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(message.reactions[0].me);

        // A second click never inflates the count; Discord ignores the PUT.
        assert!(!message.add_own_reaction("\u{1f44d}"));
        assert_eq!(message.reactions[0].count, 1);

        message.remove_own_reaction("\u{1f44d}");
        assert!(
            message.reactions.is_empty(),
            "a lone own reaction disappears"
        );
    }

    #[test]
    fn removing_our_reaction_keeps_other_peoples_counts() {
        let mut message = message_with("hi");
        message.reactions.push(Reaction {
            emoji: "\u{2764}".to_string(),
            count: 4,
            me: true,
            users: Vec::new(),
        });

        message.remove_own_reaction("\u{2764}");
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 3);
        assert!(!message.reactions[0].me);
    }

    #[test]
    fn a_voice_join_lists_the_member() {
        let mut voice = VoiceTracker::default();
        assert!(voice.update(Some("50"), "7", "alice"));
        assert_eq!(voice.members("50").len(), 1);
        assert_eq!(voice.members("50")[0].username, "alice");
        assert_eq!(voice.total(), 1);
        assert!(voice.members("51").is_empty());
    }

    #[test]
    fn moving_between_channels_removes_the_old_entry() {
        let mut voice = VoiceTracker::default();
        voice.update(Some("50"), "7", "alice");
        assert!(voice.update(Some("51"), "7", "alice"));

        assert!(voice.members("50").is_empty());
        assert_eq!(voice.members("51").len(), 1);
        assert_eq!(voice.total(), 1);
    }

    #[test]
    fn disconnecting_removes_the_member() {
        let mut voice = VoiceTracker::default();
        voice.update(Some("50"), "7", "alice");
        assert!(voice.update(None, "7", "alice"));
        assert_eq!(voice.total(), 0);
        // A second leave is not a change.
        assert!(!voice.update(None, "7", "alice"));
    }

    #[test]
    fn a_repeated_update_changes_nothing() {
        let mut voice = VoiceTracker::default();
        voice.update(Some("50"), "7", "alice");
        assert!(!voice.update(Some("50"), "7", "alice"));
        assert_eq!(voice.members("50").len(), 1);
    }

    #[test]
    fn an_empty_user_id_is_ignored() {
        let mut voice = VoiceTracker::default();
        assert!(!voice.update(Some("50"), "", "ghost"));
        assert_eq!(voice.total(), 0);
    }

    #[test]
    fn a_channel_is_capped() {
        let mut voice = VoiceTracker::default();
        for index in 0..(VOICE_CHANNEL_CAP + 5) {
            voice.update(Some("50"), &format!("u{index}"), "someone");
        }
        assert_eq!(voice.members("50").len(), VOICE_CHANNEL_CAP);
        assert_eq!(voice.total(), VOICE_CHANNEL_CAP);
    }

    #[test]
    fn typing_says_who_is_typing() {
        let start = std::time::Instant::now();
        assert!(typing_due(None, "c1", start));
        assert!(!typing_due(Some(("c1", start)), "c1", start));
        assert!(typing_due(
            Some(("c1", start)),
            "c1",
            start + TYPING_REFRESH
        ));
        // A switch of channel sends immediately, even inside the window.
        assert!(typing_due(Some(("c1", start)), "c2", start));

        // The countdown drives a single scheduled wake-up, not a poll loop.
        assert_eq!(
            typing_refresh_in(
                Some(("c1", start)),
                "c1",
                start + std::time::Duration::from_secs(3)
            ),
            Some(std::time::Duration::from_secs(5))
        );
        assert_eq!(
            typing_refresh_in(
                Some(("c1", start)),
                "c1",
                start + TYPING_REFRESH + std::time::Duration::from_secs(1)
            ),
            Some(std::time::Duration::ZERO)
        );
        assert_eq!(typing_refresh_in(Some(("c1", start)), "c2", start), None);

        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("alice is typing")
        );

        typing.record("c1", "u2", "bob", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("alice and bob are typing")
        );

        typing.record("c1", "u3", "carol", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("several people are typing")
        );
    }

    #[test]
    fn typing_is_scoped_to_its_channel() {
        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 0.0);

        assert!(typing.summary("c2", 0.0).is_none());
        assert!(typing.summary("c1", 0.0).is_some());
    }

    #[test]
    fn typing_expires_without_a_timer() {
        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 100.0);

        assert!(typing.summary("c1", 100.0 + TYPING_TTL - 0.1).is_some());
        assert!(typing.summary("c1", 100.0 + TYPING_TTL).is_none());
    }

    #[test]
    fn a_repeated_typing_event_refreshes_rather_than_duplicates() {
        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 0.0);
        typing.record("c1", "u1", "alice", 5.0);

        assert_eq!(typing.len(), 1);
        // Refreshed in time, so it outlives the first expiry.
        assert!(typing.summary("c1", TYPING_TTL + 1.0).is_some());
    }

    #[test]
    fn the_typing_list_stays_bounded() {
        let mut typing = TypingTracker::default();
        // All at the same instant, so nothing expires while the cap is reached.
        for index in 0..100 {
            typing.record("c1", &format!("u{index}"), &format!("user{index}"), 0.0);
        }
        assert_eq!(typing.len(), TYPING_CAP);

        // The oldest expire first, and the tracker drains back to empty.
        assert!(typing.summary("c1", TYPING_TTL + 0.1).is_none());
    }

    #[test]
    fn the_typing_dots_cycle_through_three_frames() {
        assert_eq!(typing_dots(0.0), ".");
        assert_eq!(typing_dots(TYPING_FRAME_SECONDS * 1.2), "..");
        assert_eq!(typing_dots(TYPING_FRAME_SECONDS * 2.2), "...");
        // The cycle repeats, and a clock that starts before zero still lands on
        // a frame rather than panicking or indexing out of range.
        assert_eq!(typing_dots(TYPING_FRAME_SECONDS * 3.5), ".");
        assert_eq!(typing_dots(-1.0), ".");
    }

    #[test]
    fn profile_cosmetics_build_discord_cdn_urls() {
        let user = User {
            id: "42".to_string(),
            username: "jane".to_string(),
            avatar: Some("abc".to_string()),
            global_name: Some("Jane".to_string()),
            banner: Some("bannerhash".to_string()),
            accent_color: Some(0x11_22_33),
            avatar_decoration: Some("deco".to_string()),
            status: Presence::Online,
        };
        assert_eq!(user.display_name(), "Jane");
        assert_eq!(
            user.avatar_url().as_deref(),
            Some("https://cdn.discordapp.com/avatars/42/abc.png?size=80")
        );
        assert_eq!(
            user.banner_url().as_deref(),
            Some("https://cdn.discordapp.com/banners/42/bannerhash.png?size=480")
        );
        assert_eq!(
            user.decoration_url().as_deref(),
            Some("https://cdn.discordapp.com/avatar-decoration-presets/deco.png")
        );

        // No cosmetics: the handle is the name, the default avatar still loads,
        // and there is no banner or decoration to fetch.
        let plain = User::default();
        assert_eq!(plain.display_name(), "Unknown");
        assert_eq!(
            plain.avatar_url().as_deref(),
            Some("https://cdn.discordapp.com/embed/avatars/0.png")
        );
        assert!(plain.banner_url().is_none());
        assert!(plain.decoration_url().is_none());

        // An unparseable id cannot be mapped to a default avatar.
        let anonymous = User {
            id: String::new(),
            ..User::default()
        };
        assert!(anonymous.avatar_url().is_none());
    }

    #[test]
    fn forgetting_a_channel_clears_only_that_channel() {
        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 0.0);
        typing.record("c2", "u2", "bob", 0.0);

        typing.forget_channel("c1");
        assert!(typing.summary("c1", 0.0).is_none());
        assert!(typing.summary("c2", 0.0).is_some());
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Activity {
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamingState {
    pub active: bool,
    pub title: String,
    pub viewer_count: u32,
}
