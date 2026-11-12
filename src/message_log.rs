//! Local log of edited and deleted messages — the native answer to what Vencord
//! calls `MessageLogger`.
//!
//! Vencord's plugins are TypeScript patches against Discord's own web bundle, so
//! they cannot be loaded into a from-scratch Rust client (`DECISIONS.md` D36);
//! what they *do* is possible here because the gateway already delivers
//! `MESSAGE_UPDATE` and `MESSAGE_DELETE`, and the client already holds the
//! previous content of every message it has seen.
//!
//! Two deliberate limits:
//!
//! * **Session only.** Entries live in memory, capped at [`MAX_ENTRIES`], and are
//!   never written to disk. Logging other people's deleted messages to a file is
//!   a privacy decision this client does not make on the user's behalf.
//! * **Only what the client saw.** A message deleted before this process started,
//!   or in a channel that was never opened, leaves nothing to record. There is no
//!   endpoint that would tell us what it said.
//!
//! Pure and tested without a UI or a gateway.

use crate::models::Message;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;

/// Oldest entries are dropped past this; the log must never grow unbounded.
pub const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Edited,
    Deleted,
}

impl LogKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Edited => "edited",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub kind: LogKind,
    pub message_id: String,
    pub channel_id: String,
    pub author: String,
    /// Content before the change; for a deletion, the content that vanished.
    pub before: String,
    /// Replacement content. `None` for a deletion.
    pub after: Option<String>,
    pub at: DateTime<Utc>,
}

impl LogEntry {
    /// One line for filtering and for the list header.
    fn haystack(&self) -> String {
        format!(
            "{} {} {} {}",
            self.author,
            self.channel_id,
            self.before,
            self.after.as_deref().unwrap_or("")
        )
        .to_lowercase()
    }
}

#[derive(Debug, Default, Clone)]
pub struct MessageLog {
    /// Oldest at the front, newest at the back.
    entries: VecDeque<LogEntry>,
}

impl MessageLog {
    /// Record an edit and report whether anything was logged. An update that does
    /// not change the text (an embed unfurl, for instance) is not an edit.
    pub fn record_edit(&mut self, message: &Message, after: &str, at: DateTime<Utc>) -> bool {
        if message.content == after {
            return false;
        }
        self.push(LogEntry {
            kind: LogKind::Edited,
            message_id: message.id.clone(),
            channel_id: message.channel_id.clone().unwrap_or_default(),
            author: message.author.username.clone(),
            before: message.content.clone(),
            after: Some(after.to_string()),
            at,
        });
        true
    }

    /// Record a deletion, keeping the content that is about to disappear.
    pub fn record_delete(&mut self, message: &Message, at: DateTime<Utc>) {
        self.push(LogEntry {
            kind: LogKind::Deleted,
            message_id: message.id.clone(),
            channel_id: message.channel_id.clone().unwrap_or_default(),
            author: message.author.username.clone(),
            before: message.content.clone(),
            after: None,
            at,
        });
    }

    fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Newest first, which is the order the panel lists them in.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Case-insensitive match over author, channel id and both contents. An empty
    /// query matches everything.
    pub fn filter<'a>(&'a self, query: &str) -> Vec<&'a LogEntry> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.entries().collect();
        }
        self.entries()
            .filter(|entry| entry.haystack().contains(&needle))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;

    fn sample(id: &str, author: &str, content: &str) -> Message {
        let mut message = Message::new(
            User {
                id: "1".to_string(),
                username: author.to_string(),
                avatar: None,
                ..User::default()
            },
            content,
        );
        message.id = id.to_string();
        message.channel_id = Some("300".to_string());
        message
    }

    #[test]
    fn an_edit_keeps_both_versions() {
        let mut log = MessageLog::default();
        let message = sample("10", "alice", "first");

        assert!(log.record_edit(&message, "second", Utc::now()));
        let entry = log.entries().next().expect("entry");
        assert_eq!(entry.kind, LogKind::Edited);
        assert_eq!(entry.author, "alice");
        assert_eq!(entry.before, "first");
        assert_eq!(entry.after.as_deref(), Some("second"));
        assert_eq!(entry.channel_id, "300");
    }

    #[test]
    fn an_unchanged_update_is_not_logged() {
        let mut log = MessageLog::default();
        let message = sample("10", "alice", "same");
        assert!(!log.record_edit(&message, "same", Utc::now()));
        assert!(log.is_empty());
    }

    #[test]
    fn a_deletion_keeps_the_content_that_vanished() {
        let mut log = MessageLog::default();
        log.record_delete(&sample("10", "bob", "oops"), Utc::now());

        let entry = log.entries().next().expect("entry");
        assert_eq!(entry.kind, LogKind::Deleted);
        assert_eq!(entry.before, "oops");
        assert_eq!(entry.after, None);
        assert_eq!(entry.kind.label(), "deleted");
    }

    #[test]
    fn entries_are_newest_first() {
        let mut log = MessageLog::default();
        log.record_delete(&sample("1", "a", "one"), Utc::now());
        log.record_delete(&sample("2", "a", "two"), Utc::now());

        let ids: Vec<&str> = log
            .entries()
            .map(|entry| entry.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["2", "1"]);
    }

    #[test]
    fn the_log_is_capped_and_drops_the_oldest() {
        let mut log = MessageLog::default();
        for index in 0..(MAX_ENTRIES + 5) {
            log.record_delete(&sample(&index.to_string(), "a", "x"), Utc::now());
        }
        assert_eq!(log.len(), MAX_ENTRIES);
        assert_eq!(
            log.entries().next().unwrap().message_id,
            (MAX_ENTRIES + 4).to_string(),
            "newest kept"
        );
        assert!(
            log.entries()
                .all(|entry| entry.message_id != "0" && entry.message_id != "4"),
            "the five oldest were evicted"
        );
    }

    #[test]
    fn filtering_matches_author_and_both_contents() {
        let mut log = MessageLog::default();
        let message = sample("10", "Alice", "hello secret");
        log.record_edit(&message, "hello there", Utc::now());
        log.record_delete(&sample("11", "bob", "bye"), Utc::now());

        assert_eq!(log.filter("alice").len(), 1);
        assert_eq!(log.filter("SECRET").len(), 1, "the old text is searchable");
        assert_eq!(log.filter("there").len(), 1, "so is the new text");
        assert_eq!(log.filter("300").len(), 2, "channel id");
        assert_eq!(log.filter("   ").len(), 2, "blank matches everything");
        assert!(log.filter("nothing").is_empty());

        log.clear();
        assert!(log.is_empty());
    }
}
