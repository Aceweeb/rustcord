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
    #[serde(default)]
    pub status: Presence,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: "0".to_string(),
            username: "Unknown".to_string(),
            avatar: None,
            status: Presence::Offline,
        }
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

    /// A sentence for the composer, or `None` when nobody is typing.
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
            1 => Some(format!("{} is typing…", names[0])),
            2 => Some(format!("{} and {} are typing…", names[0], names[1])),
            _ => Some("several people are typing…".to_string()),
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
    fn typing_says_who_is_typing() {
        let mut typing = TypingTracker::default();
        typing.record("c1", "u1", "alice", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("alice is typing…")
        );

        typing.record("c1", "u2", "bob", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("alice and bob are typing…")
        );

        typing.record("c1", "u3", "carol", 0.0);
        assert_eq!(
            typing.summary("c1", 0.0).as_deref(),
            Some("several people are typing…")
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
