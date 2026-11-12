use crate::models::{Message, Presence, Reaction, User};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const API_BASE: &str = "https://discord.com/api/v10";

/// Thin Discord REST wrapper. The token should include its auth scheme, e.g.
/// `Bot <token>` for bot accounts. Third-party user clients are not endorsed;
/// check Discord's terms before using a user token.
pub struct DiscordClient {
    http: reqwest::Client,
    token: String,
}

impl DiscordClient {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("Rustcord/0.1 (https://example.invalid)")
            .build()
            .context("failed to build Discord HTTP client")?;

        let token = token.into();
        let token = token.trim();
        let token = if token.starts_with("Bot ") || token.starts_with("Bearer ") {
            token.to_string()
        } else {
            format!("Bot {token}")
        };

        Ok(Self { http, token })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = self.get_text(path).await?;
        serde_json::from_str(&body)
            .with_context(|| format!("could not decode Discord response for {path}"))
    }

    async fn get_text(&self, path: &str) -> Result<String> {
        let url = format!("{API_BASE}{path}");
        let response = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &self.token)
            .header(USER_AGENT, "Rustcord/0.1")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        response
            .error_for_status()
            .with_context(|| format!("Discord returned an error for {path}"))?
            .text()
            .await
            .with_context(|| format!("could not read Discord response for {path}"))
    }

    pub async fn get_gateway_url(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Gateway {
            url: String,
        }
        let gateway: Gateway = self.get("/gateway").await?;
        Ok(gateway.url)
    }

    pub async fn get_current_user(&self) -> Result<User> {
        let raw: DiscordUser = self.get("/users/@me").await?;
        Ok(raw.into())
    }

    /// Fetch one page of channel history.
    ///
    /// Discord returns newest-first; the result is oldest-first for display.
    /// `before` is the pagination cursor — pass the id of the oldest message
    /// already held to load the preceding page.
    pub async fn fetch_messages(
        &self,
        channel_id: &str,
        before: Option<&str>,
        limit: u8,
    ) -> Result<Vec<Message>> {
        let limit = limit.clamp(1, 100);
        let mut path = format!("/channels/{channel_id}/messages?limit={limit}");
        if let Some(before) = before {
            path.push_str("&before=");
            path.push_str(before);
        }

        let body = self.get_text(&path).await?;
        parse_history(&body, channel_id)
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<Message> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages");
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &self.token)
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?
            .error_for_status()
            .context("Discord rejected the message")?;

        let raw: DiscordMessage = response
            .json()
            .await
            .context("could not decode Discord message")?;
        Message::try_from(raw)
    }
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    avatar: Option<String>,
}

impl From<DiscordUser> for User {
    fn from(raw: DiscordUser) -> Self {
        Self {
            id: raw.id,
            username: raw.username,
            avatar: raw.avatar,
            status: Presence::Offline,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    content: String,
    timestamp: String,
    author: DiscordUser,
    #[serde(default)]
    reactions: Vec<DiscordReaction>,
}

#[derive(Debug, Deserialize)]
struct DiscordReaction {
    #[serde(default)]
    count: u32,
    #[serde(default)]
    me: bool,
    #[serde(default)]
    emoji: DiscordEmoji,
}

#[derive(Debug, Default, Deserialize)]
struct DiscordEmoji {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

/// Parse a `GET /channels/{id}/messages` body.
///
/// Pure so pagination order, reaction mapping and malformed payloads are
/// testable without a network call. `channel_id` is applied explicitly so a
/// message can never leak into the wrong channel's view.
pub fn parse_history(body: &str, channel_id: &str) -> Result<Vec<Message>> {
    let raw: Vec<DiscordMessage> =
        serde_json::from_str(body).context("could not decode Discord history")?;

    let mut messages = Vec::with_capacity(raw.len());
    for item in raw {
        let mut message = Message::try_from(item)?;
        message.channel_id = Some(channel_id.to_string());
        messages.push(message);
    }

    // Discord sends newest-first; everything downstream expects chronological.
    messages.reverse();
    Ok(messages)
}

impl TryFrom<DiscordMessage> for Message {
    type Error = anyhow::Error;

    fn try_from(raw: DiscordMessage) -> Result<Self> {
        let timestamp = DateTime::parse_from_rfc3339(&raw.timestamp)
            .context("invalid Discord timestamp")?
            .with_timezone(&Utc);

        Ok(Self {
            id: raw.id,
            channel_id: raw.channel_id,
            author: raw.author.into(),
            content: raw.content,
            timestamp,
            reactions: raw
                .reactions
                .into_iter()
                .map(|reaction| Reaction {
                    emoji: reaction
                        .emoji
                        .name
                        .or(reaction.emoji.id)
                        .unwrap_or_else(|| "?".to_string()),
                    count: reaction.count,
                    me: reaction.me,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Newest-first, exactly as Discord returns it. The second message has no
    // reaction array at all, which is a common real-world shape.
    const PAGE: &str = r#"[
      {
        "id": "102",
        "channel_id": "10",
        "content": "newest",
        "timestamp": "2026-01-02T03:04:05.000000+00:00",
        "author": { "id": "2", "username": "alice", "avatar": null },
        "reactions": [ { "count": 3, "me": true, "emoji": { "name": "rocket", "id": null } } ]
      },
      {
        "id": "101",
        "channel_id": "10",
        "content": "older",
        "timestamp": "2026-01-02T03:00:00.000000+00:00",
        "author": { "id": "3", "username": "bob", "avatar": null }
      }
    ]"#;

    #[test]
    fn history_is_returned_oldest_first() {
        let messages = parse_history(PAGE, "10").expect("parse");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "older");
        assert_eq!(messages[1].content, "newest");
        assert!(messages[0].timestamp < messages[1].timestamp);
    }

    #[test]
    fn reactions_and_missing_arrays_are_handled() {
        let messages = parse_history(PAGE, "10").expect("parse");
        assert!(messages[0].reactions.is_empty());
        let reaction = &messages[1].reactions[0];
        assert_eq!(reaction.emoji, "rocket");
        assert_eq!(reaction.count, 3);
        assert!(reaction.me);
    }

    #[test]
    fn requested_channel_id_wins_over_the_payload() {
        let messages = parse_history(PAGE, "999").expect("parse");
        assert!(
            messages
                .iter()
                .all(|message| message.channel_id.as_deref() == Some("999")),
            "history must stay attached to the channel that was requested"
        );
    }

    #[test]
    fn author_is_mapped_from_the_payload() {
        let messages = parse_history(PAGE, "10").expect("parse");
        assert_eq!(messages[0].author.username, "bob");
        assert_eq!(messages[0].author.id, "3");
    }

    #[test]
    fn malformed_or_empty_bodies_behave_predictably() {
        assert!(parse_history("[]", "10").expect("empty page").is_empty());
        assert!(parse_history("{\"message\": \"Unauthorized\"}", "10").is_err());
        assert!(parse_history("not json", "10").is_err());
    }
}
