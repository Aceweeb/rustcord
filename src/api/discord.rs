use crate::api::ratelimit::{
    global_from_body, retry_after_from_body, RateLimitHeaders, RateLimiter, Route, MAX_WAIT_SECS,
};
use crate::models::{
    Attachment, Channel, Embed, Message, Presence, Reaction, ReplyRef, Role, User,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const API_BASE: &str = "https://discord.com/api/v10";

/// Percent-encode one path segment. Discord accepts both raw Unicode and
/// `%XX`, but a request line with raw bytes is fragile, and a custom emoji
/// (`name:id`) contains a colon that would otherwise end the segment early.
/// RFC 3986 unreserved characters stay literal.
/// Turn a failed status into something the status line can explain. A 401 is
/// almost always the wrong kind of token: the REST API authenticates bots with
/// `Authorization: Bot …`, and a user-account token sent that way is rejected
/// even though the gateway may have accepted the same session. Rustcord does
/// not send user tokens (D2), so this is a dead end to report, not to work
/// around.
fn error_for_status(status: reqwest::StatusCode, path: &str) -> String {
    match status {
        reqwest::StatusCode::UNAUTHORIZED => format!(
            "Discord rejected the token (401 Unauthorized) for {path}: a user-account \
             token cannot be used here — install a bot token instead (see DECISIONS.md D2)"
        ),
        reqwest::StatusCode::FORBIDDEN => format!(
            "Discord refused {path} (403 Forbidden): the bot lacks permission in that channel"
        ),
        other => format!("Discord returned an error for {path}: HTTP {other}"),
    }
}

/// Both reaction calls address the bot's own reaction, so they share this path
/// and differ only by verb.
fn reaction_path(channel_id: &str, message_id: &str, emoji: &str) -> Result<String> {
    let emoji = emoji.trim();
    if emoji.is_empty() {
        anyhow::bail!("a reaction needs an emoji");
    }

    Ok(format!(
        "/channels/{channel_id}/messages/{message_id}/reactions/{}/@me",
        encode_path_segment(emoji)
    ))
}

pub fn encode_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The content to send, or `None` when Discord would reject it anyway: empty
/// after trimming, or over the 2000-character limit. Pure so the rule is tested
/// without a request.
pub fn valid_content(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 2000 {
        None
    } else {
        Some(trimmed)
    }
}

/// Thin Discord REST wrapper. The token should include its auth scheme, e.g.
/// `Bot <token>` for bot accounts. Third-party user clients are not endorsed;
/// check Discord's terms before using a user token.
pub struct DiscordClient {
    http: reqwest::Client,
    token: String,
    /// Discord's buckets for this client, and when this client started (the
    /// clock the limiter measures against).
    limiter: Mutex<RateLimiter>,
    started: Instant,
}

impl DiscordClient {
    /// Create a REST client for a Discord token.
    ///
    /// `use_bot_scheme` selects the `Authorization` header shape: `true` sends
    /// `Authorization: Bot <token>` for bot accounts, while `false` sends the
    /// personal/user-account token raw, as Discord expects.
    pub fn new(token: impl Into<String>, use_bot_scheme: bool) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("Rustcord/0.1")
            .build()
            .context("failed to build Discord HTTP client")?;

        let token = token.into();
        let token = token.trim();
        let token = if use_bot_scheme {
            if token.starts_with("Bot ") || token.starts_with("Bearer ") {
                token.to_string()
            } else {
                format!("Bot {token}")
            }
        } else {
            // Personal-account tokens are sent raw; Discord rejects them with
            // any scheme prefix. Do not add "Bot " or "Bearer ".
            token.to_string()
        };

        Ok(Self {
            http,
            token,
            limiter: Mutex::new(RateLimiter::new()),
            started: Instant::now(),
        })
    }

    fn limits(&self) -> std::sync::MutexGuard<'_, RateLimiter> {
        // A panicking request must not turn every later request into a panic.
        self.limiter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = self.get_text(path).await?;
        serde_json::from_str(&body)
            .with_context(|| format!("could not decode Discord response for {path}"))
    }

    async fn get_text(&self, path: &str) -> Result<String> {
        self.request(reqwest::Method::GET, path, None).await
    }

    /// One HTTP round trip that honours Discord's rate limits.
    ///
    /// Waits for the route's bucket before sending, records what Discord reports,
    /// and retries once after a 429 using the body's `retry_after`. A wait longer
    /// than [`MAX_WAIT_SECS`] is reported to the caller instead of slept on, so a
    /// punished account shows a message rather than appearing to hang.
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<String> {
        self.request_with(method, path, |builder| match &body {
            Some(body) => builder.json(body),
            None => builder,
        })
        .await
    }

    /// Shared rate-limited round trip. The builder is a closure because a
    /// multipart body cannot be cloned for the one 429 retry.
    async fn request_with<F>(&self, method: reqwest::Method, path: &str, build: F) -> Result<String>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        let route = Route::new(method.as_str(), path);
        let url = format!("{API_BASE}{path}");

        // Every REST call is Discord's or nothing: this is the one place a URL
        // could drift off the allowlist, and the client is local-only by design.
        if !crate::privacy::host_allowed(&url) {
            anyhow::bail!("refusing a request to a host that is not Discord: {url}");
        }

        for attempt in 0..2 {
            let wait = self.limits().wait_seconds(&route, self.now());
            if wait > 0.0 {
                if wait > MAX_WAIT_SECS {
                    anyhow::bail!("Discord rate limit on {path}: retry in {wait:.0}s");
                }
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
            }

            let builder = self
                .http
                .request(method.clone(), &url)
                .header(AUTHORIZATION, &self.token)
                .header(USER_AGENT, "Rustcord/0.1");
            let response = build(builder)
                .send()
                .await
                .with_context(|| format!("{method} {url} failed"))?;
            let status = response.status();
            let headers = RateLimitHeaders::from_headers(response.headers());

            if status.as_u16() == 429 {
                let retry_body = response.text().await.unwrap_or_default();
                let body_retry = retry_after_from_body(&retry_body);
                let global = headers.global || global_from_body(&retry_body);
                let mut headers = headers;
                headers.global = global;

                let wait = self
                    .limits()
                    .note_429(&route, &headers, body_retry, self.now());

                if attempt == 0 && wait <= MAX_WAIT_SECS {
                    log::warn!("rate limited on {path}; retrying in {wait:.1}s");
                    tokio::time::sleep(Duration::from_secs_f64(wait.max(0.05))).await;
                    continue;
                }

                anyhow::bail!("Discord rate limited {path}; retry in {wait:.0}s");
            }

            self.limits().observe(&route, &headers, self.now());

            if !status.is_success() {
                anyhow::bail!(error_for_status(status, path));
            }

            return response
                .text()
                .await
                .with_context(|| format!("could not read Discord response for {path}"));
        }

        anyhow::bail!("Discord kept rate limiting {path}; try again later")
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

    /// Every DM and group DM the account is part of, newest activity first as
    /// Discord returns them.
    pub async fn fetch_dm_channels(&self, self_id: &str) -> Result<Vec<Channel>> {
        let body = self.get_text("/users/@me/channels").await?;
        let raw: Vec<DiscordDmListEntry> =
            serde_json::from_str(&body).context("could not decode the DM list")?;
        Ok(raw
            .into_iter()
            .filter_map(|entry| entry.into_channel(self_id))
            .collect())
    }

    /// Open (or reuse) the DM channel with a user. Discord returns the existing
    /// channel when one is already open, so callers can cache the id.
    pub async fn open_dm(&self, recipient_id: &str) -> Result<Channel> {
        let body = self
            .request(
                reqwest::Method::POST,
                "/users/@me/channels",
                Some(serde_json::json!({ "recipient_id": recipient_id })),
            )
            .await?;
        let raw: DiscordDmChannel =
            serde_json::from_str(&body).context("could not decode the DM channel")?;
        raw.into_channel()
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

    /// Discord's message limit. Longer content is refused by the API, so it is
    /// refused here first instead of after a round trip.
    pub const MAX_CONTENT_CHARS: usize = 2000;
    /// Discord's default upload ceiling for one file in a guild channel
    /// (10 MiB). Boosted guilds can raise it, but the client does not know the
    /// tier here.
    pub const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
    /// Discord accepts up to ten files on one message.
    pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;
    /// Local memory bound for one multipart send. Discord's per-file ceiling
    /// alone would allow 100 MiB across ten files, which is too much to read
    /// into memory; this client keeps the whole batch at 20 MiB.
    pub const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;

    pub async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<Message> {
        let content = valid_content(content).ok_or_else(|| {
            anyhow::anyhow!("a message needs 1–{} characters", Self::MAX_CONTENT_CHARS)
        })?;

        let body = self
            .request(
                reqwest::Method::PATCH,
                &format!("/channels/{channel_id}/messages/{message_id}"),
                Some(serde_json::json!({ "content": content })),
            )
            .await?;

        let raw: DiscordMessage =
            serde_json::from_str(&body).context("could not decode Discord message")?;
        Message::try_from(raw)
    }

    /// Delete a message. Discord answers 204 with no body.
    pub async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/channels/{channel_id}/messages/{message_id}"),
            None,
        )
        .await?;
        Ok(())
    }

    /// Approximate member and online counts from `GET /guilds/{id}?with_counts`.
    /// The presence half is Discord's estimate and needs no privileged intent.
    pub async fn fetch_guild_counts(&self, guild_id: &str) -> Result<GuildCounts> {
        let body = self
            .get_text(&format!("/guilds/{guild_id}?with_counts=true"))
            .await?;
        parse_guild_counts(&body)
    }

    /// The guild's roles. This endpoint has no privileged-intent requirement
    /// and supplies the names, positions and hoist flags for member sections.
    pub async fn fetch_guild_roles(&self, guild_id: &str) -> Result<Vec<Role>> {
        let body = self.get_text(&format!("/guilds/{guild_id}/roles")).await?;
        parse_roles(&body)
    }

    /// Every guild member, newest id first order, in pages of 1000. Needs the
    /// privileged Members intent enabled for the application; without it Discord
    /// answers 403. The cap bounds memory for very large guilds.
    pub async fn fetch_members(&self, guild_id: &str) -> Result<Vec<User>> {
        let mut members: Vec<User> = Vec::new();
        let mut after: Option<String> = None;

        loop {
            let mut path = format!("/guilds/{guild_id}/members?limit={MEMBER_PAGE_LIMIT}");
            if let Some(after) = &after {
                path.push_str("&after=");
                path.push_str(after);
            }

            let body = self.get_text(&path).await?;
            let page = parse_members(&body)?;
            let page_len = page.len();
            // `after` pages by user id, so the cursor is the last id of this
            // unsorted page, taken before any filtering.
            let cursor = page.last().map(|user| user.id.clone());

            for user in page {
                if members.len() >= MEMBER_CAP {
                    break;
                }
                if !members.iter().any(|known| known.id == user.id) {
                    members.push(user);
                }
            }

            if page_len < MEMBER_PAGE_LIMIT || members.len() >= MEMBER_CAP {
                break;
            }

            after = cursor;
            if after.is_none() {
                break;
            }
        }

        Ok(members)
    }

    /// Add a reaction as the bot. Discord answers 204; reaction emoji are part
    /// of the path, so a Unicode glyph has to be percent-encoded.
    pub async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()> {
        self.request(
            reqwest::Method::PUT,
            &reaction_path(channel_id, message_id, emoji)?,
            None,
        )
        .await?;
        Ok(())
    }

    /// Remove the bot's own reaction. Same `@me` path as adding, different verb;
    /// Discord answers 204.
    pub async fn remove_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()> {
        self.request(
            reqwest::Method::DELETE,
            &reaction_path(channel_id, message_id, emoji)?,
            None,
        )
        .await?;
        Ok(())
    }

    /// Tell a channel that the bot is typing. Discord allows this once per ten
    /// seconds per channel; `App::notify_typing` throttles below that, and the
    /// rate limiter still covers the case where another client shares the bot.
    pub async fn trigger_typing(&self, channel_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::POST,
            &format!("/channels/{channel_id}/typing"),
            Some(serde_json::json!({})),
        )
        .await?;
        Ok(())
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<Message> {
        self.send_message_with_reference(channel_id, content, None)
            .await
    }

    /// Send a message that references another one. `fail_if_not_exists: false`
    /// is what Discord's own clients send: a deleted target shows a plain
    /// message rather than failing the send.
    pub async fn send_reply(
        &self,
        channel_id: &str,
        content: &str,
        reply_to: &str,
    ) -> Result<Message> {
        self.send_message_with_reference(channel_id, content, Some(reply_to))
            .await
    }

    async fn send_message_with_reference(
        &self,
        channel_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<Message> {
        let payload = message_payload(channel_id, content, reply_to);
        let body = self
            .request(
                reqwest::Method::POST,
                &format!("/channels/{channel_id}/messages"),
                Some(payload),
            )
            .await?;

        let raw: DiscordMessage =
            serde_json::from_str(&body).context("could not decode Discord message")?;
        Message::try_from(raw)
    }

    /// Upload several files with an optional caption/reply. Discord expects
    /// `payload_json` plus `files[0]`, `files[1]`, … in a multipart form.
    pub async fn send_attachments(
        &self,
        channel_id: &str,
        content: &str,
        paths: &[std::path::PathBuf],
        reply_to: Option<&str>,
    ) -> Result<Message> {
        if paths.is_empty() {
            return self
                .send_message_with_reference(channel_id, content, reply_to)
                .await;
        }
        if paths.len() > Self::MAX_ATTACHMENTS_PER_MESSAGE {
            anyhow::bail!(
                "a message can carry at most {} attachments",
                Self::MAX_ATTACHMENTS_PER_MESSAGE
            );
        }
        if content.chars().count() > Self::MAX_CONTENT_CHARS {
            anyhow::bail!(
                "a message needs at most {} characters",
                Self::MAX_CONTENT_CHARS
            );
        }

        // Validate every path before reading any bytes, so an oversized batch is
        // refused without filling memory first.
        let mut selected = Vec::with_capacity(paths.len());
        let mut declared_total = 0u64;
        for path in paths {
            let metadata = tokio::fs::metadata(path)
                .await
                .with_context(|| format!("could not read attachment {}", path.display()))?;
            if !metadata.is_file() {
                anyhow::bail!("attachment is not a regular file: {}", path.display());
            }
            if metadata.len() > Self::MAX_ATTACHMENT_BYTES {
                anyhow::bail!(
                    "attachment is larger than Discord's {} MiB upload limit",
                    Self::MAX_ATTACHMENT_BYTES / (1024 * 1024)
                );
            }
            declared_total = declared_total
                .checked_add(metadata.len())
                .context("attachment sizes overflowed")?;
            if declared_total > Self::MAX_TOTAL_ATTACHMENT_BYTES {
                anyhow::bail!(
                    "attachments must total at most {} MiB",
                    Self::MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
                );
            }
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".to_string());
            let content_type = attachment_mime(&file_name);
            selected.push((path.clone(), file_name, content_type));
        }

        let mut files = Vec::with_capacity(selected.len());
        let mut actual_total = 0u64;
        for (path, file_name, content_type) in selected {
            let bytes = tokio::fs::read(&path)
                .await
                .with_context(|| format!("could not read attachment {}", path.display()))?;
            if bytes.len() as u64 > Self::MAX_ATTACHMENT_BYTES {
                anyhow::bail!("attachment grew past the upload limit while being read");
            }
            actual_total = actual_total
                .checked_add(bytes.len() as u64)
                .context("attachment sizes overflowed")?;
            if actual_total > Self::MAX_TOTAL_ATTACHMENT_BYTES {
                anyhow::bail!(
                    "attachments grew past the {} MiB total upload limit while being read",
                    Self::MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
                );
            }
            files.push((file_name, bytes, content_type));
        }

        let file_names: Vec<&str> = files
            .iter()
            .map(|(file_name, _, _)| file_name.as_str())
            .collect();
        let payload_json =
            message_payload_with_attachments(channel_id, content, reply_to, &file_names)
                .to_string();
        let body = self
            .request_with(
                reqwest::Method::POST,
                &format!("/channels/{channel_id}/messages"),
                move |builder| {
                    let mut form =
                        reqwest::multipart::Form::new().text("payload_json", payload_json.clone());
                    for (index, (file_name, bytes, content_type)) in files.iter().enumerate() {
                        form = form.part(
                            format!("files[{index}]"),
                            attachment_part(file_name, bytes.clone(), content_type),
                        );
                    }
                    builder.multipart(form)
                },
            )
            .await?;

        let raw: DiscordMessage =
            serde_json::from_str(&body).context("could not decode Discord message")?;
        Message::try_from(raw)
    }
}

/// The JSON half of a message send, shared by JSON and multipart requests.
fn message_payload(channel_id: &str, content: &str, reply_to: Option<&str>) -> serde_json::Value {
    let mut payload = serde_json::json!({ "content": content });
    if let Some(reply_to) = reply_to {
        payload["message_reference"] = serde_json::json!({
            "message_id": reply_to,
            "channel_id": channel_id,
            "fail_if_not_exists": false
        });
    }
    payload
}

/// The payload for a multipart send. Discord maps each `files[n]` part to the
/// attachment entry with the matching string id.
fn message_payload_with_attachments(
    channel_id: &str,
    content: &str,
    reply_to: Option<&str>,
    file_names: &[&str],
) -> serde_json::Value {
    let mut payload = message_payload(channel_id, content, reply_to);
    if !file_names.is_empty() {
        payload["attachments"] = serde_json::Value::Array(
            file_names
                .iter()
                .enumerate()
                .map(|(index, file_name)| {
                    serde_json::json!({ "id": index.to_string(), "filename": file_name })
                })
                .collect(),
        );
    }
    payload
}

fn attachment_mime(file_name: &str) -> &'static str {
    match file_name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("txt" | "log" | "md") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn attachment_part(
    file_name: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> reqwest::multipart::Part {
    // `attachment_mime` only returns valid constants, so this cannot fail.
    reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name.to_string())
        .mime_str(content_type)
        .expect("attachment_mime returns valid MIME types")
}

/// The DM channel Discord returns from `POST /users/@me/channels`.
#[derive(Debug, Deserialize)]
struct DiscordDmChannel {
    id: String,
    #[serde(default)]
    recipients: Vec<DiscordUser>,
}

/// One entry from `GET /users/@me/channels`: a DM (type 1) or group DM (type 3).
#[derive(Debug, Deserialize)]
struct DiscordDmListEntry {
    id: String,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    recipients: Vec<DiscordUser>,
}

impl DiscordDmListEntry {
    /// `self_id` is filtered out so a DM is named after the other person, not
    /// after the signed-in account.
    fn into_channel(self, self_id: &str) -> Option<Channel> {
        if self.id.is_empty() || !matches!(self.kind, 1 | 3) {
            return None;
        }
        let others: Vec<&DiscordUser> = self
            .recipients
            .iter()
            .filter(|user| user.id != self_id)
            .collect();
        let name = if self.kind == 3 {
            self.name
                .filter(|name| !name.trim().is_empty())
                .or_else(|| {
                    (!others.is_empty()).then(|| {
                        others
                            .iter()
                            .map(|user| {
                                user.global_name
                                    .clone()
                                    .unwrap_or_else(|| user.username.clone())
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                })
        } else {
            others.first().map(|user| {
                user.global_name
                    .clone()
                    .unwrap_or_else(|| user.username.clone())
            })
        };
        Some(Channel {
            id: self.id,
            name: name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Direct message".to_string()),
            kind: crate::models::ChannelKind::Text,
            guild_id: crate::models::DM_GUILD_ID.to_string(),
            parent_id: None,
            position: None,
            topic: None,
        })
    }
}

impl DiscordDmChannel {
    fn into_channel(self) -> Result<Channel> {
        if self.id.is_empty() {
            anyhow::bail!("Discord returned a DM channel without an id");
        }
        let name = self
            .recipients
            .first()
            .map(|user| {
                user.global_name
                    .clone()
                    .unwrap_or_else(|| user.username.clone())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Direct message".to_string());
        Ok(Channel {
            id: self.id,
            name,
            // DMs behave like text channels for history, sending and unread
            // tracking; the synthetic guild id is what distinguishes them.
            kind: crate::models::ChannelKind::Text,
            guild_id: crate::models::DM_GUILD_ID.to_string(),
            parent_id: None,
            position: None,
            topic: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    avatar: Option<String>,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    banner: Option<String>,
    #[serde(default)]
    accent_color: Option<u32>,
    #[serde(default)]
    avatar_decoration_data: Option<DiscordDecoration>,
}

#[derive(Debug, Deserialize)]
struct DiscordDecoration {
    #[serde(default)]
    asset: Option<String>,
}

impl From<DiscordUser> for User {
    fn from(raw: DiscordUser) -> Self {
        Self {
            id: raw.id,
            username: raw.username,
            avatar: raw.avatar,
            global_name: raw.global_name,
            banner: raw.banner,
            accent_color: raw.accent_color,
            avatar_decoration: raw
                .avatar_decoration_data
                .and_then(|decoration| decoration.asset),
            status: Presence::Offline,
            role_ids: Vec::new(),
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
    #[serde(default)]
    attachments: Vec<Attachment>,
    #[serde(default)]
    embeds: Vec<Embed>,
    #[serde(default)]
    referenced_message: Option<DiscordReference>,
}

/// The part of Discord's `referenced_message` this client renders.
#[derive(Debug, Deserialize)]
struct DiscordReference {
    id: String,
    #[serde(default)]
    content: String,
    author: DiscordUser,
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

/// Discord's `limit` maximum for a member page, and the total this client keeps.
pub const MEMBER_PAGE_LIMIT: usize = 1000;
pub const MEMBER_CAP: usize = 5000;

/// Discord's approximate counts for a guild.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuildCounts {
    pub total: Option<u64>,
    pub active: Option<u64>,
}

impl GuildCounts {
    /// Fold a fresh response into known counts. Discord omits a field when it
    /// has no estimate; that must not erase a value fetched a moment earlier.
    pub fn merge(&mut self, fresh: Self) {
        if fresh.total.is_some() {
            self.total = fresh.total;
        }
        if fresh.active.is_some() {
            self.active = fresh.active;
        }
    }
}

/// Parse the count fields of a guild payload. Absent counts stay `None` rather
/// than becoming a misleading zero.
pub fn parse_guild_counts(body: &str) -> Result<GuildCounts> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("could not decode guild counts")?;
    Ok(GuildCounts {
        total: value["approximate_member_count"].as_u64(),
        active: value["approximate_presence_count"].as_u64(),
    })
}

/// Parse a `GET /guilds/{id}/roles` body. Roles without an id cannot be used as
/// a section key, so they are skipped; a missing name gets a readable fallback.
pub fn parse_roles(body: &str) -> Result<Vec<Role>> {
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(body).context("could not decode Discord roles")?;
    Ok(raw
        .iter()
        .filter_map(|value| {
            Some(Role {
                id: value["id"].as_str()?.to_string(),
                name: value["name"].as_str().unwrap_or("Unnamed role").to_string(),
                color: value["color"].as_u64().unwrap_or(0) as u32,
                hoist: value["hoist"].as_bool().unwrap_or(false),
                position: value["position"].as_i64().unwrap_or(0),
            })
        })
        .collect())
}

/// Parse a `GET /guilds/{id}/members` body. Order is preserved because the
/// caller paginates with `after=<last user id>`; display sorting happens later.
/// Duplicate and malformed entries are dropped.
pub fn parse_members(body: &str) -> Result<Vec<User>> {
    #[derive(Deserialize)]
    struct Member {
        // Absent on bot/partial entries; those are skipped rather than failing
        // the whole page.
        #[serde(default)]
        user: Option<DiscordUser>,
        /// Role ids are used by the member panel to choose Discord's hoisted
        /// role section. Members are never grouped from this endpoint's order.
        #[serde(default)]
        roles: Vec<String>,
    }

    let raw: Vec<Member> =
        serde_json::from_str(body).context("could not decode Discord members")?;

    let mut users: Vec<User> = Vec::with_capacity(raw.len());
    let mut seen = std::collections::HashSet::new();
    for member in raw {
        let Some(raw_user) = member.user else {
            continue;
        };
        let mut user: User = raw_user.into();
        user.role_ids = member.roles;
        if user.id.is_empty() || user.username.is_empty() {
            continue;
        }
        if seen.insert(user.id.clone()) {
            users.push(user);
        }
    }

    Ok(users)
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

        let channel_id = raw.channel_id;
        let reply_to = raw.referenced_message.map(|reference| ReplyRef {
            message_id: reference.id,
            channel_id: channel_id.clone(),
            author: reference.author.username,
            content: reference.content,
        });

        Ok(Self {
            id: raw.id,
            channel_id,
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
                    // History does not list who reacted; gateway events do.
                    users: Vec::new(),
                })
                .collect(),
            attachments: raw.attachments,
            embeds: raw.embeds,
            reply_to,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_counts_keep_presence_separate_from_total() {
        let counts = parse_guild_counts(
            r#"{"id":"1","approximate_member_count":420,"approximate_presence_count":37}"#,
        )
        .expect("counts");
        assert_eq!(counts.total, Some(420));
        assert_eq!(counts.active, Some(37));
        let absent = parse_guild_counts(r#"{"id":"1"}"#).expect("no counts");
        assert_eq!(absent, GuildCounts::default());
    }

    #[test]
    fn guild_counts_merge_does_not_erase_known_values() {
        let mut counts = GuildCounts {
            total: Some(420),
            active: Some(37),
        };
        counts.merge(GuildCounts {
            total: Some(425),
            active: None,
        });
        assert_eq!(counts.total, Some(425));
        assert_eq!(counts.active, Some(37));
        counts.merge(GuildCounts::default());
        assert_eq!(counts.total, Some(425));
        assert_eq!(counts.active, Some(37));
    }

    #[test]
    fn members_parse_without_reordering() {
        let body = r#"[
            { "user": { "id": "3", "username": "carol", "avatar": null }, "roles": ["10", "11"] },
            { "user": { "id": "1", "username": "alice", "avatar": null } },
            { "user": { "id": "3", "username": "carol", "avatar": null } },
            { "user": { "id": "", "username": "ghost", "avatar": null } },
            { "nick": "no user object" }
        ]"#;

        let members = parse_members(body).expect("members");
        assert_eq!(members.len(), 2, "duplicates and malformed entries dropped");
        // Pagination depends on this: `after` uses the last id as returned.
        assert_eq!(members[0].id, "3");
        assert_eq!(members[0].role_ids, ["10", "11"]);
        assert!(members[1].role_ids.is_empty());
        assert_eq!(members[1].id, "1");
    }

    #[test]
    fn roles_parse_names_colours_and_hoist_order() {
        let body = r#"[
            { "id": "10", "name": "Admin", "color": 15158332, "hoist": true, "position": 30 },
            { "id": "11", "name": "Member", "color": 0, "hoist": false, "position": 0 },
            { "name": "no id" }
        ]"#;
        let roles = parse_roles(body).expect("roles");
        assert_eq!(roles.len(), 2);
        assert_eq!(roles[0].name, "Admin");
        assert_eq!(roles[0].color, 0xe7_4c_3c);
        assert!(roles[0].hoist);
        assert_eq!(roles[0].position, 30);
        assert!(!roles[1].hoist);
    }

    #[test]
    fn failures_explain_the_token_problem() {
        let unauthorized =
            error_for_status(reqwest::StatusCode::UNAUTHORIZED, "/channels/1/messages");
        assert!(unauthorized.contains("401"));
        assert!(unauthorized.contains("user-account token"));

        let forbidden = error_for_status(reqwest::StatusCode::FORBIDDEN, "/guilds/1/members");
        assert!(forbidden.contains("403"));
        assert!(forbidden.contains("permission"));

        let other = error_for_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "/x");
        assert_eq!(
            other,
            "Discord returned an error for /x: HTTP 500 Internal Server Error"
        );
    }

    #[test]
    fn a_reaction_emoji_is_encoded_for_the_path() {
        // Thumbs up is U+1F44D.
        assert_eq!(encode_path_segment("\u{1f44d}"), "%F0%9F%91%8D");
        // Custom emoji carry a colon, which must not end the segment.
        assert_eq!(encode_path_segment("blobcat:123"), "blobcat%3A123");
        assert_eq!(encode_path_segment("wave"), "wave");
    }

    #[test]
    fn adding_and_removing_share_one_reaction_path() {
        let path = reaction_path("10", "20", "\u{1f44d}").expect("path");
        assert_eq!(path, "/channels/10/messages/20/reactions/%F0%9F%91%8D/@me");
        // Trimming happens before encoding, and an empty emoji is refused so a
        // DELETE cannot turn into a request for the whole reaction list.
        assert_eq!(
            reaction_path("10", "20", " \u{1f389} ").unwrap(),
            "/channels/10/messages/20/reactions/%F0%9F%8E%89/@me"
        );
        assert!(reaction_path("10", "20", "   ").is_err());
    }

    #[test]
    fn message_payload_carries_the_reply_reference() {
        // The payload shape is what Discord's docs specify; a reply without it
        // would be sent as a normal message, which is the bug this guards.
        let payload = message_payload("10", "hi", Some("42"));
        assert_eq!(payload["message_reference"]["message_id"], "42");
        assert_eq!(payload["message_reference"]["channel_id"], "10");
        assert_eq!(payload["message_reference"]["fail_if_not_exists"], false);
        // An attachment-only upload sends an empty caption, not a missing one.
        assert_eq!(message_payload("10", "", None)["content"], "");
        assert!(message_payload("10", "", None)
            .get("message_reference")
            .is_none());
    }

    #[test]
    fn multipart_payload_ids_each_file() {
        let payload = message_payload_with_attachments("10", "hi", Some("42"), &["a.png", "b.jpg"]);
        let attachments = payload["attachments"].as_array().expect("array");
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0]["id"], "0");
        assert_eq!(attachments[0]["filename"], "a.png");
        assert_eq!(attachments[1]["id"], "1");
        assert_eq!(attachments[1]["filename"], "b.jpg");
        assert_eq!(payload["message_reference"]["message_id"], "42");
        assert!(message_payload_with_attachments("10", "hi", None, &[])
            .get("attachments")
            .is_none());
    }

    #[test]
    fn attachment_mime_comes_from_the_extension() {
        assert_eq!(attachment_mime("cat.PNG"), "image/png");
        assert_eq!(attachment_mime("photo.jpeg"), "image/jpeg");
        assert_eq!(attachment_mime("notes.md"), "text/plain");
        assert_eq!(attachment_mime("archive.zip"), "application/octet-stream");
        assert_eq!(attachment_mime("no-extension"), "application/octet-stream");
    }

    #[test]
    fn content_is_trimmed_for_sending() {
        assert_eq!(valid_content("  hello \n"), Some("hello"));
    }

    #[test]
    fn empty_content_is_refused() {
        assert_eq!(valid_content(""), None);
        assert_eq!(valid_content("   \n\t "), None);
    }

    #[test]
    fn the_two_thousand_character_limit_is_exact() {
        let at_limit = "a".repeat(2000);
        assert_eq!(valid_content(&at_limit).map(str::len), Some(2000));

        let over = "a".repeat(2001);
        assert_eq!(valid_content(&over), None);
    }

    #[test]
    fn the_limit_counts_characters_not_bytes() {
        // 2000 emoji are 8000 bytes; Discord counts characters.
        let emoji = "\u{1f525}".repeat(2000);
        assert!(valid_content(&emoji).is_some());
        assert_eq!(valid_content(&format!("{emoji}x")), None);
    }

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
    fn attachments_and_embeds_are_parsed() {
        let body = r#"[{
          "id": "1",
          "content": "",
          "timestamp": "2026-01-02T03:04:05+00:00",
          "author": { "id": "2", "username": "alice", "avatar": null },
          "attachments": [
            { "id": "9", "filename": "cat.png", "url": "https://cdn.discordapp.com/a/cat.png",
              "content_type": "image/png", "width": 800, "height": 600, "size": 2048 }
          ],
          "embeds": [
            { "title": "A link", "color": 5793266,
              "image": { "url": "https://cdn.discordapp.com/e.png", "width": 400, "height": 300 } }
          ]
        }]"#;

        let messages = parse_history(body, "10").expect("parse");
        let message = &messages[0];

        let attachment = &message.attachments[0];
        assert_eq!(attachment.filename, "cat.png");
        assert!(attachment.is_image());
        assert_eq!(attachment.dimensions(), Some((800, 600)));

        let embed = &message.embeds[0];
        assert_eq!(embed.title.as_deref(), Some("A link"));
        assert_eq!(embed.color, Some(5_793_266));
        assert_eq!(embed.image_url(), Some("https://cdn.discordapp.com/e.png"));
        assert!(!embed.is_empty());
    }

    #[test]
    fn image_detection_falls_back_to_the_extension() {
        let attachment = Attachment {
            filename: "photo.JPEG".to_string(),
            ..Attachment::default()
        };
        assert!(attachment.is_image());

        let document = Attachment {
            filename: "notes.pdf".to_string(),
            content_type: Some("application/pdf".to_string()),
            ..Attachment::default()
        };
        assert!(!document.is_image());
    }

    #[test]
    fn the_dm_list_filters_self_and_ignores_guild_channels() {
        let raw: Vec<DiscordDmListEntry> = serde_json::from_str(
            r#"[
                {"id":"500","type":1,"recipients":[
                    {"id":"42","username":"jane","global_name":"Jane"},
                    {"id":"900","username":"rustcord-bot"}
                ]},
                {"id":"501","type":3,"name":"","recipients":[
                    {"id":"900","username":"rustcord-bot"},
                    {"id":"42","username":"jane"},
                    {"id":"43","username":"bob","global_name":"Bobby"}
                ]},
                {"id":"502","type":0,"recipients":[]}
            ]"#,
        )
        .expect("dm list");
        let channels: Vec<Channel> = raw
            .into_iter()
            .filter_map(|entry| entry.into_channel("900"))
            .collect();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Jane");
        assert_eq!(channels[1].name, "jane, Bobby");
        assert!(channels
            .iter()
            .all(|channel| channel.guild_id == crate::models::DM_GUILD_ID));
    }

    #[test]
    fn a_dm_channel_uses_the_recipients_display_name() {
        let raw: DiscordDmChannel = serde_json::from_str(
            r#"{"id":"500","recipients":[
                {"id":"42","username":"jane","global_name":"Jane","avatar":null}
            ]}"#,
        )
        .expect("dm payload");
        let channel = raw.into_channel().expect("channel");
        assert_eq!(channel.id, "500");
        assert_eq!(channel.name, "Jane");
        assert_eq!(channel.kind, crate::models::ChannelKind::Text);
        assert_eq!(channel.guild_id, crate::models::DM_GUILD_ID);

        // A payload without a recipient still names the channel and stays usable.
        let bare: DiscordDmChannel = serde_json::from_str(r#"{"id":"501"}"#).expect("bare payload");
        assert_eq!(bare.into_channel().expect("channel").name, "Direct message");

        // Discord must return an id; an empty one is an error, not a clickable row.
        let empty: DiscordDmChannel =
            serde_json::from_str(r#"{"id":"","recipients":[]}"#).expect("empty payload");
        assert!(empty.into_channel().is_err());
    }

    #[test]
    fn malformed_or_empty_bodies_behave_predictably() {
        assert!(parse_history("[]", "10").expect("empty page").is_empty());
        assert!(parse_history("{\"message\": \"Unauthorized\"}", "10").is_err());
        assert!(parse_history("not json", "10").is_err());
    }
}
