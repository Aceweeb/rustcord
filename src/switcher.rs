//! Ctrl+K quick switcher.
//!
//! Discord's switcher jumps between servers and channels by name; this is the
//! same idea over the data the client already holds (guilds and channels from
//! `READY`/`GUILD_CREATE`). Everything here is pure and tested without a UI: the
//! overlay in `ui/switcher.rs` only renders entries and moves a cursor.

use crate::models::{Channel, ChannelKind, Guild};

/// The list never grows past this, however broad the query is.
pub const MAX_RESULTS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Guild,
    Channel,
}

/// One row in the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub id: String,
    /// Guild to select, also for a channel (its parent).
    pub guild_id: String,
    pub primary: String,
    pub secondary: String,
    haystack: String,
}

impl Entry {
    fn guild(guild: &Guild) -> Self {
        let haystack = format!("{} server", guild.name).to_lowercase();
        Self {
            kind: EntryKind::Guild,
            id: guild.id.clone(),
            guild_id: guild.id.clone(),
            primary: guild.name.clone(),
            secondary: "Server".to_string(),
            haystack,
        }
    }

    fn channel(channel: &Channel, guild_name: &str) -> Self {
        let primary = format!("#{}", channel.name);
        let haystack = format!("{primary} {guild_name}").to_lowercase();
        Self {
            kind: EntryKind::Channel,
            id: channel.id.clone(),
            guild_id: channel.guild_id.clone(),
            primary,
            secondary: guild_name.to_string(),
            haystack,
        }
    }
}

/// Servers first, then their text and voice channels, in the order the client
/// received them (Discord already sends guilds in the user's own order).
pub fn build_entries(guilds: &[Guild], channels: &[Channel]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = guilds.iter().map(Entry::guild).collect();

    for guild in guilds {
        for channel in channels.iter().filter(|channel| {
            channel.guild_id == guild.id
                && matches!(channel.kind, ChannelKind::Text | ChannelKind::Voice)
        }) {
            entries.push(Entry::channel(channel, &guild.name));
        }
    }

    entries
}

fn rank(entry: &Entry, first_token: &str) -> u8 {
    let primary = entry.primary.to_lowercase();
    if primary.starts_with(first_token) {
        0
    } else if primary.contains(first_token) {
        1
    } else {
        2
    }
}

/// Entries whose name contains every whitespace-separated token, best first.
/// An empty query lists the first servers, which is what a switcher shows before
/// you type.
pub fn filter<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|token| !token.is_empty())
        .collect();

    if tokens.is_empty() {
        return entries.iter().take(MAX_RESULTS).collect();
    }

    let mut matches: Vec<(u8, &Entry)> = entries
        .iter()
        .filter(|entry| tokens.iter().all(|token| entry.haystack.contains(token)))
        .map(|entry| (rank(entry, &tokens[0]), entry))
        .collect();

    matches.sort_by(|(left_rank, left), (right_rank, right)| {
        left_rank
            .cmp(right_rank)
            .then_with(|| {
                left.primary
                    .to_lowercase()
                    .cmp(&right.primary.to_lowercase())
            })
            .then_with(|| left.secondary.cmp(&right.secondary))
    });

    matches
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, entry)| entry)
        .collect()
}

/// Overlay state. The cursor is clamped to the result list every frame, so a
/// narrowing query can never leave it pointing past the end.
#[derive(Debug, Default, Clone)]
pub struct SwitcherState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub just_opened: bool,
}

impl SwitcherState {
    pub fn open_now(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.just_opened = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.just_opened = false;
    }

    pub fn clamp(&mut self, count: usize) {
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    /// Move the cursor, wrapping at both ends like Discord's switcher.
    pub fn move_by(&mut self, delta: i32, count: usize) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        let count = count as i32;
        let next = (self.selected as i32 + delta).rem_euclid(count);
        self.selected = next as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(id: &str, name: &str) -> Guild {
        Guild {
            id: id.to_string(),
            name: name.to_string(),
            icon: None,
        }
    }

    fn channel(id: &str, guild_id: &str, name: &str, kind: ChannelKind) -> Channel {
        Channel {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            guild_id: guild_id.to_string(),
        }
    }

    fn sample() -> Vec<Entry> {
        let guilds = vec![guild("1", "Gamers"), guild("2", "Rustcord Dev")];
        let channels = vec![
            channel("10", "1", "general", ChannelKind::Text),
            channel("11", "1", "voice-lounge", ChannelKind::Voice),
            channel("12", "1", "Category", ChannelKind::Category),
            channel("20", "2", "cargo", ChannelKind::Text),
        ];
        build_entries(&guilds, &channels)
    }

    #[test]
    fn entries_include_servers_and_real_channels_only() {
        let entries = sample();
        assert_eq!(entries.len(), 5, "category channels are not jump targets");
        assert_eq!(entries[0].kind, EntryKind::Guild);
        assert_eq!(entries[0].primary, "Gamers");
        assert!(entries.iter().any(|entry| entry.primary == "#voice-lounge"));
        assert!(!entries.iter().any(|entry| entry.primary == "#Category"));
    }

    #[test]
    fn filtering_matches_channel_and_guild_names() {
        let entries = sample();

        let hits = filter(&entries, "cargo");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].primary, "#cargo");
        assert_eq!(hits[0].guild_id, "2");

        let hits = filter(&entries, "gamers");
        assert_eq!(hits[0].primary, "Gamers");
        assert_eq!(hits[0].kind, EntryKind::Guild);
    }

    #[test]
    fn every_token_must_match() {
        let entries = sample();
        let hits = filter(&entries, "rustcord cargo");
        assert_eq!(hits.len(), 1, "the guild name also matches the channel row");
        assert_eq!(hits[0].primary, "#cargo");

        assert!(filter(&entries, "rustcord general").is_empty());
    }

    #[test]
    fn prefixes_rank_above_substrings() {
        let guilds = vec![guild("1", "General Chat"), guild("2", "Chat")];
        let entries = build_entries(&guilds, &[]);

        let hits = filter(&entries, "chat");
        assert_eq!(hits[0].primary, "Chat", "prefix wins over substring");
        assert_eq!(hits[1].primary, "General Chat");
    }

    #[test]
    fn an_empty_query_lists_servers_first_and_is_capped() {
        let guilds: Vec<Guild> = (0..20).map(|i| guild(&i.to_string(), "g")).collect();
        let entries = build_entries(&guilds, &[]);

        let hits = filter(&entries, "   ");
        assert_eq!(hits.len(), MAX_RESULTS);
        assert_eq!(hits[0].kind, EntryKind::Guild);
    }

    #[test]
    fn the_cursor_wraps_and_clamps() {
        let mut state = SwitcherState::default();
        state.open_now();
        assert!(state.open && state.just_opened);

        state.move_by(1, 3);
        assert_eq!(state.selected, 1);
        state.move_by(-1, 3);
        assert_eq!(state.selected, 0);
        state.move_by(-1, 3);
        assert_eq!(state.selected, 2, "wraps to the end");

        state.selected = 9;
        state.clamp(3);
        assert_eq!(state.selected, 2);

        state.move_by(1, 0);
        assert_eq!(state.selected, 0);
        state.close();
        assert!(!state.open);
    }
}
