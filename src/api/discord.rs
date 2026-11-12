use crate::models::{Message, Presence, User};
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
        let url = format!("{API_BASE}{path}");
        let response = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &self.token)
            .header(USER_AGENT, "Rustcord/0.1")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        let response = response
            .error_for_status()
            .with_context(|| format!("Discord returned an error for {path}"))?;

        response
            .json()
            .await
            .with_context(|| format!("could not decode Discord response for {path}"))
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
        raw.try_into()
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
    channel_id: String,
    content: String,
    timestamp: String,
    author: DiscordUser,
}

impl TryFrom<DiscordMessage> for Message {
    type Error = anyhow::Error;

    fn try_from(raw: DiscordMessage) -> Result<Self> {
        let timestamp = DateTime::parse_from_rfc3339(&raw.timestamp)
            .context("invalid Discord timestamp")?
            .with_timezone(&Utc);

        Ok(Self {
            id: raw.id,
            channel_id: Some(raw.channel_id),
            author: raw.author.into(),
            content: raw.content,
            timestamp,
            reactions: Vec::new(),
        })
    }
}
