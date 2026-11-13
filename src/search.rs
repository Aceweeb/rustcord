//! Local message search.
//!
//! Discord's guild-search endpoint (`GET /guilds/{id}/messages/search`) is part
//! of the **user** API: a bot token cannot use it, and Rustcord does not hold a
//! user token on purpose (`DECISIONS.md` D2, D15). The honest alternative is to
//! search what the client actually holds: every message cached this session,
//! across all channels that were opened, bounded by
//! `ResourcePriority::history_cap()`. Results can be narrowed by author
//! (`from:name`), channel (`in:#name`) and time (`before:`/`after:`).
//!
//! That limit is stated in the UI rather than papered over. It also keeps the
//! whole feature cheap: no index, no extra memory, no network. A search is a
//! reverse scan over the cache with a case-insensitive substring test per token.
//!
//! Everything here is pure and tested without a UI or a gateway.

use crate::models::Message;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Never return more than this, whatever the caller asks for.
pub const MAX_RESULTS: usize = 50;

/// Characters of context kept around the first match.
const SNIPPET_CHARS: usize = 96;
/// Characters of content kept before the match.
const SNIPPET_LEAD: usize = 28;

/// A parsed query: free-text terms (all must match) plus an optional author.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// Lowercased; every term must appear in a message's content.
    pub terms: Vec<String>,
    /// Lowercased username fragment from `from:someone`.
    pub author: Option<String>,
    /// Lowercased channel name fragment from `in:#general` (the `#` is optional).
    pub channel: Option<String>,
    /// Exclusive upper bound from `before:YYYY-MM-DD` (UTC midnight).
    pub before: Option<DateTime<Utc>>,
    /// Exclusive lower bound from `after:YYYY-MM-DD` (UTC midnight).
    pub after: Option<DateTime<Utc>>,
}

impl SearchQuery {
    /// Parse `text`, `from:name`, `@name`, channel filters (`in:#name`,
    /// `channel:#name`, `#name`) and date filters (`before:`, `after:`).
    ///
    /// Dates accept `YYYY-MM-DD` and `YYYY-MM-DDThh:mm[:ss]` as UTC, and both
    /// bounds are exclusive, matching Discord's own search. Unknown
    /// `word:value` tokens — including an unparseable date — are treated as
    /// text, not filters, so a colon in a message never silently drops the term.
    pub fn parse(input: &str) -> Self {
        let mut terms = Vec::new();
        let mut author = None;
        let mut channel = None;
        let mut before = None;
        let mut after = None;

        for token in input.split_whitespace() {
            if let Some(rest) = token.strip_prefix("before:") {
                if let Some(date) = parse_date(rest) {
                    before = Some(date);
                    continue;
                }
            }
            if let Some(rest) = token.strip_prefix("after:") {
                if let Some(date) = parse_date(rest) {
                    after = Some(date);
                    continue;
                }
            }
            let channel_part = token
                .strip_prefix("in:")
                .or_else(|| token.strip_prefix("channel:"));
            if let Some(rest) = channel_part {
                let name = rest.trim_start_matches('#').trim().to_lowercase();
                if !name.is_empty() {
                    channel = Some(name);
                }
                // A bare `in:` is an unfinished filter, not a literal term.
                continue;
            }
            if let Some(rest) = token.strip_prefix('#') {
                // A bare `#general` is the same filter, which is what people type.
                let name = rest.trim().to_lowercase();
                if !name.is_empty() {
                    channel = Some(name);
                    continue;
                }
            }
            if let Some(rest) = token.strip_prefix("from:") {
                let name = rest.trim_start_matches('@').trim().to_lowercase();
                if !name.is_empty() {
                    author = Some(name);
                }
                // A bare `from:` is an unfinished filter, not a literal term.
                continue;
            }
            if let Some(rest) = token.strip_prefix('@') {
                // A bare `@name` is the same filter, which is what people type.
                let name = rest.trim().to_lowercase();
                if !name.is_empty() {
                    author = Some(name);
                    continue;
                }
            }
            terms.push(token.to_lowercase());
        }

        Self {
            terms,
            author,
            channel,
            before,
            after,
        }
    }

    /// True when there is nothing to search for.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.author.is_none()
            && self.channel.is_none()
            && self.before.is_none()
            && self.after.is_none()
    }

    /// A short description of the filter, for the results header.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.terms.is_empty() {
            parts.push(format!("\u{201c}{}\u{201d}", self.terms.join(" ")));
        }
        if let Some(author) = &self.author {
            parts.push(format!("from {author}"));
        }
        if let Some(channel) = &self.channel {
            parts.push(format!("in #{channel}"));
        }
        if let Some(after) = self.after {
            parts.push(format!("after {}", after.format("%-d %b %Y")));
        }
        if let Some(before) = self.before {
            parts.push(format!("before {}", before.format("%-d %b %Y")));
        }
        parts.join(" ")
    }
}

/// One result. Deliberately a snapshot rather than a reference: the message list
/// keeps changing under live events, and a row that vanished mid-frame must not
/// index into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub message_id: String,
    pub channel_id: Option<String>,
    /// Resolved channel name, so a result says where it came from.
    pub channel: Option<String>,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub snippet: String,
}

/// Search newest-first across the cached messages, stopping at `limit` (clamped
/// to `1..=MAX_RESULTS`). `channel_names` maps channel id to display name so the
/// `in:#name` filter and the result label both work.
pub fn search(
    messages: &[Message],
    channel_names: &HashMap<String, String>,
    query: &SearchQuery,
    limit: usize,
) -> Vec<Hit> {
    if query.is_empty() {
        return Vec::new();
    }

    let limit = limit.clamp(1, MAX_RESULTS);
    let mut hits = Vec::new();

    for message in messages.iter().rev() {
        if hits.len() >= limit {
            break;
        }
        if let Some(hit) = hit_for(message, channel_names, query) {
            hits.push(hit);
        }
    }

    hits
}

fn hit_for(
    message: &Message,
    channel_names: &HashMap<String, String>,
    query: &SearchQuery,
) -> Option<Hit> {
    let channel = message
        .channel_id
        .as_ref()
        .and_then(|id| channel_names.get(id))
        .cloned();

    if let Some(author) = &query.author {
        if !message.author.username.to_lowercase().contains(author) {
            return None;
        }
    }

    if let Some(wanted) = &query.channel {
        let matches = channel
            .as_deref()
            .map(|name| name.to_lowercase().contains(wanted))
            .unwrap_or(false);
        if !matches {
            return None;
        }
    }

    // Both bounds are exclusive, so `before:2026-11-12` means "before that day
    // began" rather than including anything stamped on it.
    if let Some(before) = query.before {
        if message.timestamp >= before {
            return None;
        }
    }
    if let Some(after) = query.after {
        if message.timestamp <= after {
            return None;
        }
    }

    let content = message.content.to_lowercase();
    if !query.terms.iter().all(|term| content.contains(term)) {
        return None;
    }

    Some(Hit {
        message_id: message.id.clone(),
        channel_id: message.channel_id.clone(),
        channel,
        author: message.author.username.clone(),
        timestamp: message.timestamp,
        snippet: snippet(&message.content, query.terms.first().map(String::as_str)),
    })
}

/// Byte ranges in `snippet` that match any of `terms`, merged and sorted.
///
/// Case-insensitive, like the search itself. Lowercasing can change a string's
/// length (`İ` becomes two chars), which would make byte offsets from the lowered
/// copy point into the wrong place; in that rare case the range is skipped rather
/// than mis-highlighting, and the row still shows the snippet.
pub fn highlight_ranges(snippet: &str, terms: &[String]) -> Vec<(usize, usize)> {
    if snippet.is_empty() || terms.is_empty() {
        return Vec::new();
    }
    let lowered = snippet.to_lowercase();
    if lowered.chars().count() != snippet.chars().count() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    for term in terms {
        if term.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(offset) = lowered[from..].find(term.as_str()) {
            let start = from + offset;
            let end = start + term.len();
            ranges.push((start, end));
            from = end.max(start + 1);
            if from >= lowered.len() {
                break;
            }
        }
    }

    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Parse an absolute date filter value. UTC is deliberate: message timestamps are
/// UTC, so a filter cannot silently mean different instants on different machines.
fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return date.and_hms_opt(0, 0, 0).map(|naive| naive.and_utc());
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Some(naive.and_utc());
        }
    }
    None
}

/// A single-line window around the first matching term.
///
/// Byte offsets from the lowercased copy are converted to character offsets over
/// the original before slicing, so multi-byte content (emoji, CJK, combining
/// marks) cannot panic or split a character.
fn snippet(content: &str, needle: Option<&str>) -> String {
    let flat: String = content
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .collect();
    let chars: Vec<char> = flat.chars().collect();

    let lead = needle
        .and_then(|needle| {
            let lowered = flat.to_lowercase();
            let byte_offset = lowered.find(needle)?;
            Some(lowered[..byte_offset].chars().count())
        })
        .unwrap_or(0);

    let start = lead.saturating_sub(SNIPPET_LEAD).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(chars[start..end].iter());
    if end < chars.len() {
        out.push('…');
    }

    out.trim().to_string()
}

/// UI state for the search overlay. Lives here so the app and the view share one
/// definition without a circular dependency.
#[derive(Debug, Default, Clone)]
pub struct SearchState {
    pub open: bool,
    /// Focus the field on the next frame (set when the overlay opens).
    pub just_opened: bool,
    pub query: String,
    pub results: Vec<Hit>,
    /// Live events or a new page changed the messages under an open overlay.
    pub needs_refresh: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;

    fn message(author: &str, content: &str, seconds: i64) -> Message {
        let mut message = Message::new(
            User {
                id: "1".to_string(),
                username: author.to_string(),
                avatar: None,
                status: crate::models::Presence::default(),
                ..User::default()
            },
            content,
        );
        message.id = format!("{seconds}");
        message.channel_id = Some("300".to_string());
        message.timestamp = DateTime::from_timestamp(seconds, 0).expect("timestamp");
        message
    }

    fn names() -> HashMap<String, String> {
        HashMap::from([("300".to_string(), "general".to_string())])
    }

    fn hits(messages: &[Message], input: &str) -> Vec<Hit> {
        search(messages, &names(), &SearchQuery::parse(input), MAX_RESULTS)
    }

    #[test]
    fn a_plain_query_is_lowercased_and_split() {
        let query = SearchQuery::parse("Hello World");
        assert_eq!(query.terms, vec!["hello", "world"]);
        assert_eq!(query.author, None);
        assert!(!query.is_empty());
    }

    #[test]
    fn from_and_at_prefixes_become_an_author_filter() {
        assert_eq!(
            SearchQuery::parse("from:@Alice hi").author.as_deref(),
            Some("alice")
        );
        assert_eq!(SearchQuery::parse("@bob").author.as_deref(), Some("bob"));
        // A bare `from:` with nothing after it is not a filter.
        assert_eq!(SearchQuery::parse("from:").author, None);
        assert_eq!(SearchQuery::parse("from:").terms, Vec::<String>::new());
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let messages = vec![message("alice", "hello", 1)];
        assert!(hits(&messages, "   ").is_empty());
        assert!(SearchQuery::parse("").is_empty());
    }

    #[test]
    fn matching_is_case_insensitive_and_and_requires_every_term() {
        let messages = vec![
            message("alice", "The Quick Brown Fox", 1),
            message("alice", "quick", 2),
        ];

        let found = hits(&messages, "quick fox");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message_id, "1");

        assert_eq!(hits(&messages, "QUICK").len(), 2);
        assert_eq!(hits(&messages, "quick zebra").len(), 0);
    }

    #[test]
    fn results_are_newest_first_and_capped() {
        let messages: Vec<_> = (0..10)
            .map(|index| message("alice", &format!("hello {index}"), index))
            .collect();

        let found = search(&messages, &names(), &SearchQuery::parse("hello"), 3);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].message_id, "9");
        assert_eq!(found[2].message_id, "7");

        // A zero limit still returns one result rather than nothing.
        assert_eq!(
            search(&messages, &names(), &SearchQuery::parse("hello"), 0).len(),
            1
        );
        // And the hard ceiling holds.
        assert_eq!(
            search(&messages, &names(), &SearchQuery::parse("hello"), 10_000).len(),
            10
        );
    }

    #[test]
    fn the_author_filter_narrows_results() {
        let messages = vec![
            message("alice", "ship it", 1),
            message("bob", "ship it", 2),
            message("bobby", "ship it", 3),
        ];

        let found = hits(&messages, "ship from:bob");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|hit| hit.author.starts_with("bob")));
    }

    #[test]
    fn an_author_only_query_lists_that_author() {
        let messages = vec![message("alice", "one", 1), message("bob", "two", 2)];

        let found = hits(&messages, "from:alice");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].author, "alice");
    }

    #[test]
    fn snippets_keep_the_match_and_flatten_newlines() {
        let long = format!("{} needle tail", "x ".repeat(80));
        let messages = vec![message("alice", &long, 1)];

        let found = hits(&messages, "needle");
        assert_eq!(found.len(), 1);
        let snippet = &found[0].snippet;
        assert!(snippet.contains("needle"), "snippet: {snippet}");
        assert!(snippet.starts_with('…'), "snippet: {snippet}");
        assert!(snippet.len() < long.len());

        let multiline = vec![message("alice", "first\nsecond\nthird", 1)];
        let found = hits(&multiline, "second");
        assert!(!found[0].snippet.contains('\n'));
    }

    #[test]
    fn snippets_survive_multibyte_content() {
        let content = format!("{}🔥 needle 🎉 {}", "é".repeat(40), "ü".repeat(60));
        let messages = vec![message("alice", &content, 1)];

        let found = hits(&messages, "needle");
        assert_eq!(found.len(), 1);
        assert!(found[0].snippet.contains("needle"));
        // No panic and no replacement characters: the slice landed on boundaries.
        assert!(!found[0].snippet.contains('\u{fffd}'));
    }

    #[test]
    fn a_hit_carries_what_the_ui_needs_to_jump() {
        let mut message = message("alice", "hello", 1);
        message.channel_id = Some("777".to_string());
        message.id = "42".to_string();

        let found = hits(&[message], "hello");
        assert_eq!(found[0].message_id, "42");
        assert_eq!(found[0].channel_id.as_deref(), Some("777"));
        assert_eq!(found[0].timestamp.timestamp(), 1);
    }

    #[test]
    fn channel_filters_parse_and_narrow() {
        assert_eq!(
            SearchQuery::parse("in:#general hi").channel.as_deref(),
            Some("general")
        );
        assert_eq!(
            SearchQuery::parse("channel:General").channel.as_deref(),
            Some("general")
        );
        assert_eq!(
            SearchQuery::parse("#off-topic").channel.as_deref(),
            Some("off-topic")
        );
        // A bare marker is not a filter.
        assert_eq!(SearchQuery::parse("in:").channel, None);
        assert!(SearchQuery::parse("in:").is_empty());

        let mut other = message("alice", "ship it", 2);
        other.channel_id = Some("301".to_string());
        let messages = vec![message("alice", "ship it", 1), other];
        let names = HashMap::from([
            ("300".to_string(), "general".to_string()),
            ("301".to_string(), "off-topic".to_string()),
        ]);

        let found = search(
            &messages,
            &names,
            &SearchQuery::parse("in:#off"),
            MAX_RESULTS,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].channel.as_deref(), Some("off-topic"));
        assert_eq!(found[0].channel_id.as_deref(), Some("301"));

        // An unknown channel matches nothing rather than ignoring the filter.
        assert!(search(
            &messages,
            &names,
            &SearchQuery::parse("in:#missing"),
            MAX_RESULTS
        )
        .is_empty());
    }

    #[test]
    fn a_hit_names_its_channel_for_the_row_label() {
        let found = hits(&[message("alice", "hello", 1)], "hello");
        assert_eq!(found[0].channel.as_deref(), Some("general"));

        // A message whose channel is not in the map still matches text queries;
        // it just has no name to show.
        let mut orphan = message("alice", "hello", 1);
        orphan.channel_id = Some("999".to_string());
        let found = hits(&[orphan], "hello");
        assert_eq!(found[0].channel, None);
    }

    #[test]
    fn date_filters_parse_and_bound_exclusively() {
        let query = SearchQuery::parse("hi before:2026-11-12 after:2026-11-01");
        assert_eq!(
            query.before,
            Some(
                DateTime::parse_from_rfc3339("2026-11-12T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        assert_eq!(
            query.after,
            Some(
                DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        assert!(!query.is_empty());

        // Times are accepted too, which makes a same-day window possible.
        let timed = SearchQuery::parse("after:2026-11-01T12:30");
        assert_eq!(
            timed.after,
            Some(
                DateTime::parse_from_rfc3339("2026-11-01T12:30:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        assert_eq!(timed.terms, Vec::<String>::new());

        // An unparseable date stays a literal term instead of acting as a filter.
        let bad = SearchQuery::parse("before:soon hi");
        assert_eq!(bad.before, None);
        assert_eq!(bad.terms, vec!["before:soon", "hi"]);

        let mut early = message("alice", "ship it", 0);
        early.timestamp = DateTime::parse_from_rfc3339("2026-10-31T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut start = message("alice", "ship it", 0);
        start.timestamp = DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut middle = message("alice", "ship it", 0);
        middle.timestamp = DateTime::parse_from_rfc3339("2026-11-05T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut late = message("alice", "ship it", 0);
        late.timestamp = DateTime::parse_from_rfc3339("2026-11-12T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let middle_ts = middle.timestamp;
        let messages = vec![early, start, middle, late];
        let found = hits(&messages, "ship before:2026-11-12 after:2026-11-01");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].timestamp, middle_ts);

        // Either bound alone works, and both are exclusive.
        assert_eq!(hits(&messages, "ship after:2026-11-01").len(), 2);
        assert_eq!(hits(&messages, "ship before:2026-11-12").len(), 3);
    }

    #[test]
    fn matched_terms_become_merged_highlight_ranges() {
        assert_eq!(
            highlight_ranges("hello world", &["world".into()]),
            vec![(6, 11)]
        );
        // Case-insensitive, like the search.
        assert_eq!(
            highlight_ranges("Hello World", &["hello".into()]),
            vec![(0, 5)]
        );
        // Every occurrence, with overlaps merged rather than double-painted.
        assert_eq!(
            highlight_ranges("ab ab", &["ab".into(), "b".into()]),
            vec![(0, 2), (3, 5)]
        );
        assert!(highlight_ranges("hello", &["zebra".into()]).is_empty());
        assert!(highlight_ranges("", &["x".into()]).is_empty());
        assert!(highlight_ranges("hello", &[]).is_empty());
        // A term that lowercases to a different character count cannot be mapped
        // safely, so it is skipped instead of highlighting the wrong bytes.
        assert!(highlight_ranges("İstanbul", &["stan".into()]).is_empty());
    }

    #[test]
    fn the_description_names_the_filter() {
        assert_eq!(
            SearchQuery::parse("hi from:bob").describe(),
            "“hi” from bob"
        );
        assert_eq!(SearchQuery::parse("from:bob").describe(), "from bob");
        assert_eq!(
            SearchQuery::parse("hi in:#general").describe(),
            "“hi” in #general"
        );
        assert_eq!(
            SearchQuery::parse("hi after:2026-11-01 before:2026-11-12").describe(),
            "“hi” after 1 Nov 2026 before 12 Nov 2026"
        );
    }
}
