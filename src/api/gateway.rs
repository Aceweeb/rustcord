use super::backoff::{
    jittered_secs, monotonic_secs, random_unit, should_reset_backoff, IdentifyAllow,
    IdentifyBudget, IDENTIFY_LIMIT,
};
use crate::models::{Channel, ChannelKind, Embed, Guild, Message, Presence, User};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

/// GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT.
/// MESSAGE_CONTENT is privileged; enable it in the bot settings or remove that
/// bit if you only need metadata.
/// GUILD_MEMBERS is privileged: it only works when "Server Members Intent" is
/// enabled for the application, and Discord closes the session with 4014 when it
/// is not. The gateway strips the bit and keeps running in that case, so a bot
/// without the toggle still gets guilds, channels and messages.
pub const MEMBER_INTENT: u64 = 1 << 1;

pub const DEFAULT_INTENTS: u64 = (1 << 0) | MEMBER_INTENT | (1 << 9) | (1 << 12) | (1 << 15);

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    Connected,
    Ready {
        username: String,
        /// The bot's own user id, so reaction events can tell whether the bot
        /// is the one reacting.
        user_id: String,
        /// The signed-in account's own cosmetics, from READY's user object.
        avatar: Option<String>,
        global_name: Option<String>,
        avatar_decoration: Option<String>,
        guild_count: usize,
    },
    GuildCreate {
        guild: Guild,
        channels: Vec<Channel>,
    },
    /// Boxed: a message carries its reply reference, and the enum is cloned
    /// through the event channel; the pointer keeps the variant small.
    Message(Box<Message>),
    /// `MESSAGE_UPDATE`. Absent fields were not sent and must be preserved.
    MessageUpdate {
        channel_id: Option<String>,
        id: String,
        content: Option<String>,
        embeds: Option<Vec<Embed>>,
    },
    MessageDelete {
        channel_id: Option<String>,
        id: String,
    },
    /// `MESSAGE_REACTION_ADD` / `MESSAGE_REACTION_REMOVE`.
    Reaction {
        channel_id: Option<String>,
        message_id: String,
        emoji: String,
        user_id: Option<String>,
        added: bool,
    },
    /// `MESSAGE_REACTION_REMOVE_ALL` (`emoji` is `None`) and
    /// `MESSAGE_REACTION_REMOVE_EMOJI` (one emoji).
    ReactionsRemoved {
        channel_id: Option<String>,
        message_id: String,
        emoji: Option<String>,
    },
    Typing {
        channel_id: String,
        user_id: String,
        username: String,
    },
    /// Discord refused the privileged Members intent; the roster is unavailable
    /// until it is enabled in the Developer Portal.
    MemberIntentMissing,
    /// `VOICE_STATE_UPDATE`. `channel_id` is `None` when the user left voice.
    VoiceState {
        channel_id: Option<String>,
        user_id: String,
        username: String,
    },
    Reconnecting {
        attempt: u32,
        delay_secs: u64,
        reason: String,
    },
    Resumed,
    Error(String),
}

pub struct GatewayHandle {
    pub events: mpsc::UnboundedReceiver<GatewayEvent>,
    shutdown: mpsc::UnboundedSender<()>,
}

impl GatewayHandle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(());
    }
}

pub struct GatewayClient {
    token: String,
    intents: u64,
}

impl GatewayClient {
    pub fn new(token: impl Into<String>, intents: u64) -> Self {
        let token = token.into();
        let token = token.trim();
        let token = if token.starts_with("Bot ") || token.starts_with("Bearer ") {
            token.to_string()
        } else {
            format!("Bot {token}")
        };

        Self { token, intents }
    }

    /// Spawn the gateway loop on an existing Tokio runtime. The UI drains
    /// `GatewayHandle::events` with `try_recv` from its own frame loop.
    pub fn start(self, runtime: &tokio::runtime::Runtime) -> GatewayHandle {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();

        runtime.spawn(async move {
            if let Err(err) =
                run_gateway(&self.token, self.intents, event_tx.clone(), shutdown_rx).await
            {
                let _ = event_tx.send(GatewayEvent::Error(format!("{err:#}")));
            }
        });

        GatewayHandle {
            events: event_rx,
            shutdown: shutdown_tx,
        }
    }
}

/// Connection state that survives across reconnects. Discord lets a client
/// resume an existing session instead of re-identifying, which avoids missing
/// events that were dispatched while the socket was down.
#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    resume_url: Option<String>,
    sequence: Option<u64>,
    /// When the last READY/RESUMED arrived, if it has not been consumed yet.
    healthy_since: Option<std::time::Instant>,
}

impl SessionState {
    fn invalidate(&mut self) {
        self.session_id = None;
        self.resume_url = None;
        self.sequence = None;
        self.healthy_since = None;
    }
}

enum CycleOutcome {
    Shutdown,
    Reconnect(String),
    InvalidSession,
    /// The identify allowance is spent; wait this long before reconnecting.
    /// Deliberately not counted as a failed attempt.
    BudgetWait(u64),
    /// Discord refused the requested intents (4014). The member intent is
    /// privileged, so the caller retries without it instead of giving up.
    IntentsRejected,
}

async fn run_gateway(
    token: &str,
    mut intents: u64,
    tx: mpsc::UnboundedSender<GatewayEvent>,
    mut shutdown: mpsc::UnboundedReceiver<()>,
) -> Result<()> {
    let mut session = SessionState::default();
    let mut attempt: u32 = 0;
    let mut identify_budget = IdentifyBudget::new();

    loop {
        let outcome = gateway_cycle(
            token,
            intents,
            &tx,
            &mut shutdown,
            &mut session,
            &mut identify_budget,
        )
        .await;

        let reason = match outcome {
            CycleOutcome::Shutdown => return Ok(()),
            CycleOutcome::BudgetWait(secs) => {
                let _ = tx.send(GatewayEvent::Reconnecting {
                    attempt,
                    delay_secs: secs,
                    reason: format!(
                        "identify allowance spent ({IDENTIFY_LIMIT} an hour); waiting {secs}s"
                    ),
                });
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                    _ = shutdown.recv() => return Ok(()),
                }
                // Not a failure: the ladder keeps its place.
                continue;
            }
            CycleOutcome::IntentsRejected if intents & MEMBER_INTENT != 0 => {
                intents &= !MEMBER_INTENT;
                // A fresh identify is required: the session never started.
                session.invalidate();
                let _ = tx.send(GatewayEvent::MemberIntentMissing);
                log::warn!("Discord refused the Members intent; retrying without it");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = shutdown.recv() => return Ok(()),
                }
                continue;
            }
            CycleOutcome::IntentsRejected => "disallowed intents".to_string(),
            CycleOutcome::InvalidSession => {
                session.invalidate();
                "invalid session (check the bot token)".to_string()
            }
            CycleOutcome::Reconnect(reason) => reason,
        };

        // Only a session that stayed up resets the ladder. A handshake that
        // succeeds and dies immediately must keep backing off, or a crash loop
        // would retry every two seconds forever.
        if session
            .healthy_since
            .take()
            .is_some_and(|since| should_reset_backoff(since.elapsed().as_secs_f64()))
        {
            attempt = 0;
        }

        attempt = attempt.saturating_add(1);
        // Jittered so a fleet on the same outage does not retry in lockstep.
        let delay_secs = jittered_secs(backoff_secs(attempt), random_unit());
        let _ = tx.send(GatewayEvent::Reconnecting {
            attempt,
            delay_secs,
            reason,
        });

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay_secs)) => {}
            _ = shutdown.recv() => return Ok(()),
        }
    }
}

/// Run one websocket lifetime. Returns when the socket needs to be replaced or
/// the caller asked us to shut down.
async fn gateway_cycle(
    token: &str,
    intents: u64,
    tx: &mpsc::UnboundedSender<GatewayEvent>,
    shutdown: &mut mpsc::UnboundedReceiver<()>,
    session: &mut SessionState,
    identify_budget: &mut IdentifyBudget,
) -> CycleOutcome {
    let url = session
        .resume_url
        .as_deref()
        .map(normalize_resume_url)
        .unwrap_or_else(|| GATEWAY_URL.to_string());

    // The resume URL arrives in a gateway payload, so it is checked like any
    // other untrusted input before a socket is opened to it.
    if !crate::privacy::host_allowed(&url) {
        return CycleOutcome::Reconnect(format!("refusing a non-Discord gateway: {url}"));
    }

    let (ws, _) = match connect_async(&url).await {
        Ok(pair) => pair,
        Err(err) => return CycleOutcome::Reconnect(format!("could not connect: {err}")),
    };
    let (mut write, mut read) = ws.split();

    let hello_frame = tokio::select! {
        _ = shutdown.recv() => {
            let _ = write.close().await;
            return CycleOutcome::Shutdown;
        }
        result = tokio::time::timeout(Duration::from_secs(30), read.next()) => {
            match result {
                Err(_) => return CycleOutcome::Reconnect("timed out waiting for Hello".to_string()),
                Ok(None) => {
                    return CycleOutcome::Reconnect("gateway closed before Hello".to_string());
                }
                Ok(Some(Err(err))) => {
                    return CycleOutcome::Reconnect(format!("gateway read failed: {err}"));
                }
                Ok(Some(Ok(frame))) => frame,
            }
        }
    };

    let hello_text = match hello_frame {
        WsMessage::Text(text) => text,
        other => {
            return CycleOutcome::Reconnect(format!(
                "expected gateway Hello text frame, got {other:?}"
            ));
        }
    };
    let hello: Value = match serde_json::from_str(hello_text.as_str()) {
        Ok(value) => value,
        Err(err) => return CycleOutcome::Reconnect(format!("invalid Hello JSON: {err}")),
    };
    let heartbeat_ms = hello["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);

    tx.send(GatewayEvent::Connected).ok();

    // Resume when we have both a session id and a sequence number; otherwise
    // start a fresh Identify handshake.
    let resume = session.session_id.clone().zip(session.sequence);
    let (op, data, action) = match resume {
        // A resume is not an identify and costs no allowance.
        Some((session_id, seq)) => (
            6,
            json!({ "token": token, "session_id": session_id, "seq": seq }),
            "Resume",
        ),
        None => match identify_budget.try_take(monotonic_secs()) {
            IdentifyAllow::Allowed => (2, identify_data(token, intents), "Identify"),
            IdentifyAllow::Wait { retry_after } => {
                let _ = write.close().await;
                return CycleOutcome::BudgetWait(retry_after.max(1));
            }
        },
    };
    let payload = json!({ "op": op, "d": data });
    if let Err(err) = write
        .send(WsMessage::Text(payload.to_string().into()))
        .await
    {
        return CycleOutcome::Reconnect(format!("could not send {action}: {err}"));
    }

    let mut heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_ms));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first interval tick fires immediately; consume it so the first
    // heartbeat happens one full interval after Identify/Resume.
    heartbeat.tick().await;

    // Discord closes the socket if it does not receive a heartbeat; if we do
    // not receive an ACK, the connection is a zombie and should be replaced.
    let mut heartbeat_ack = true;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if !heartbeat_ack {
                    return CycleOutcome::Reconnect("heartbeat ACK not received".to_string());
                }
                heartbeat_ack = false;
                if let Err(err) = send_heartbeat(&mut write, session.sequence).await {
                    return CycleOutcome::Reconnect(format!("heartbeat send failed: {err}"));
                }
            }
            _ = shutdown.recv() => {
                let _ = write.close().await;
                return CycleOutcome::Shutdown;
            }
            incoming = read.next() => {
                let Some(frame) = incoming else {
                    return CycleOutcome::Reconnect("gateway socket closed".to_string());
                };
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(err) => {
                        return CycleOutcome::Reconnect(format!("gateway read failed: {err}"));
                    }
                };

                match frame {
                    WsMessage::Text(text) => {
                        let payload: Value = match serde_json::from_str(text.as_str()) {
                            Ok(value) => value,
                            Err(err) => {
                                return CycleOutcome::Reconnect(format!(
                                    "invalid gateway JSON: {err}"
                                ));
                            }
                        };

                        if let Some(s) = payload["s"].as_u64() {
                            session.sequence = Some(s);
                        }

                        match payload["op"].as_u64().unwrap_or(0) {
                            // Heartbeat ACK.
                            11 => heartbeat_ack = true,
                            // Reconnect requested by the server.
                            7 => {
                                return CycleOutcome::Reconnect(
                                    "gateway requested reconnect".to_string(),
                                );
                            }
                            // Invalid session: discard it and Identify again.
                            9 => return CycleOutcome::InvalidSession,
                            // Server requests an immediate heartbeat.
                            1 => {
                                heartbeat_ack = false;
                                if let Err(err) =
                                    send_heartbeat(&mut write, session.sequence).await
                                {
                                    return CycleOutcome::Reconnect(format!(
                                        "server-requested heartbeat failed: {err}"
                                    ));
                                }
                            }
                            // Dispatch.
                            0 => {
                                match payload["t"].as_str().unwrap_or_default() {
                                    "READY" => {
                                        if let Some(id) = payload["d"]["session_id"].as_str() {
                                            session.session_id = Some(id.to_string());
                                        }
                                        if let Some(url) =
                                            payload["d"]["resume_gateway_url"].as_str()
                                        {
                                            session.resume_url = Some(url.to_string());
                                        }
                                        session.healthy_since = Some(std::time::Instant::now());
                                    }
                                    "RESUMED" => {
                                        session.healthy_since = Some(std::time::Instant::now());
                                        tx.send(GatewayEvent::Resumed).ok();
                                    }
                                    _ => {}
                                }

                                handle_payload(&payload, tx);
                            }
                            _ => {}
                        }
                    }
                    WsMessage::Ping(data) => {
                        if let Err(err) = write.send(WsMessage::Pong(data)).await {
                            return CycleOutcome::Reconnect(format!("pong failed: {err}"));
                        }
                    }
                    WsMessage::Close(frame) => {
                        let code = frame.as_ref().map(|frame| u16::from(frame.code));
                        if is_disallowed_intents(code) {
                            return CycleOutcome::IntentsRejected;
                        }
                        return CycleOutcome::Reconnect(format!("closed: {frame:?}"));
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Discord closes with 4014 ("Disallowed intent(s)") when a privileged intent
/// is requested without being enabled for the application. That is a
/// configuration fact, so the caller retries without the member intent rather
/// than backing off as if the network failed.
fn is_disallowed_intents(code: Option<u16>) -> bool {
    code == Some(4014)
}

/// Exponential backoff capped at one minute: 2, 4, 8, ... 60 seconds.
fn backoff_secs(attempt: u32) -> u64 {
    const MAX_BACKOFF_SECS: u64 = 60;
    (1u64 << attempt.min(6)).min(MAX_BACKOFF_SECS)
}

fn normalize_resume_url(raw: &str) -> String {
    if raw.contains('?') {
        raw.to_string()
    } else {
        format!("{}/?v=10&encoding=json", raw.trim_end_matches('/'))
    }
}

fn identify_data(token: &str, intents: u64) -> Value {
    json!({
        "token": token,
        "intents": intents,
        "properties": {
            "os": "linux",
            "browser": "rustcord",
            "device": "rustcord"
        }
    })
}

async fn send_heartbeat<S>(
    write: &mut S,
    sequence: Option<u64>,
) -> std::result::Result<(), tokio_tungstenite::tungstenite::Error>
where
    S: SinkExt<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let payload = json!({ "op": 1, "d": sequence });
    write
        .send(WsMessage::Text(payload.to_string().into()))
        .await
}

fn handle_payload(payload: &Value, tx: &mpsc::UnboundedSender<GatewayEvent>) {
    let event = payload["t"].as_str().unwrap_or_default();
    match event {
        "READY" => {
            let username = payload["d"]["user"]["username"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            let user_id = payload["d"]["user"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let avatar = payload["d"]["user"]["avatar"].as_str().map(str::to_string);
            let global_name = payload["d"]["user"]["global_name"]
                .as_str()
                .map(str::to_string);
            let avatar_decoration = payload["d"]["user"]["avatar_decoration_data"]["asset"]
                .as_str()
                .map(str::to_string);
            // Borrowed, not cloned: READY can be tens of megabytes of guild
            // objects and a copy of it is pure transient RSS.
            let guilds = payload["d"]["guilds"].as_array().map(Vec::as_slice);
            let guild_count = guilds.map(<[_]>::len).unwrap_or(0);
            tx.send(GatewayEvent::Ready {
                username,
                user_id,
                avatar,
                global_name,
                avatar_decoration,
                guild_count,
            })
            .ok();

            // Discord does not always follow READY with a GUILD_CREATE per
            // guild: an identify that is handed available guilds puts the full
            // objects in this array instead (verified against the live
            // gateway: 23 guilds here, zero GUILD_CREATE events). Using it only
            // as a count left the rail empty, so every complete entry is turned
            // into the same event GUILD_CREATE produces.
            for guild in guilds.unwrap_or_default() {
                if guild["unavailable"].as_bool().unwrap_or(false) {
                    continue;
                }
                if let Some((guild, channels)) = parse_guild(guild) {
                    tx.send(GatewayEvent::GuildCreate { guild, channels }).ok();
                }
            }
        }
        "GUILD_CREATE" => {
            if let Some((guild, channels)) = parse_guild(&payload["d"]) {
                tx.send(GatewayEvent::GuildCreate { guild, channels }).ok();
            }
        }
        "MESSAGE_CREATE" => {
            if let Ok(message) = parse_message(&payload["d"]) {
                tx.send(GatewayEvent::Message(Box::new(message))).ok();
            }
        }
        "MESSAGE_UPDATE" => {
            if let Some(event) = parse_message_update(&payload["d"]) {
                tx.send(event).ok();
            }
        }
        "MESSAGE_DELETE" => {
            if let Some(event) = parse_message_delete(&payload["d"]) {
                tx.send(event).ok();
            }
        }
        "MESSAGE_REACTION_ADD" => {
            if let Some(event) = parse_reaction(&payload["d"], true) {
                tx.send(event).ok();
            }
        }
        "MESSAGE_REACTION_REMOVE" => {
            if let Some(event) = parse_reaction(&payload["d"], false) {
                tx.send(event).ok();
            }
        }
        "MESSAGE_REACTION_REMOVE_ALL" => {
            if let Some(event) = parse_reactions_removed(&payload["d"], false) {
                tx.send(event).ok();
            }
        }
        "MESSAGE_REACTION_REMOVE_EMOJI" => {
            if let Some(event) = parse_reactions_removed(&payload["d"], true) {
                tx.send(event).ok();
            }
        }
        "VOICE_STATE_UPDATE" => {
            if let Some(event) = parse_voice_state(&payload["d"]) {
                tx.send(event).ok();
            }
        }
        "TYPING_START" => {
            if let Some(event) = parse_typing(&payload["d"]) {
                tx.send(event).ok();
            }
        }
        _ => {}
    }
}

fn parse_guild(value: &Value) -> Option<(Guild, Vec<Channel>)> {
    let id = value["id"].as_str()?.to_string();
    let name = value["name"]
        .as_str()
        .unwrap_or("Unnamed guild")
        .to_string();
    let icon = value["icon"].as_str().map(str::to_string);
    let member_count = value["member_count"].as_u64();

    let channels = value["channels"]
        .as_array()
        .map(|channels| {
            channels
                .iter()
                .filter_map(|channel| parse_channel(channel, &id))
                .collect()
        })
        .unwrap_or_default();

    Some((
        Guild {
            id,
            name,
            icon,
            member_count,
        },
        channels,
    ))
}

fn parse_channel(value: &Value, guild_id: &str) -> Option<Channel> {
    let kind = match value["type"].as_u64()? {
        0 => ChannelKind::Text,
        2 => ChannelKind::Voice,
        4 => ChannelKind::Category,
        _ => return None,
    };

    Some(Channel {
        id: value["id"].as_str()?.to_string(),
        name: value["name"].as_str().unwrap_or("unnamed").to_string(),
        kind,
        guild_id: guild_id.to_string(),
    })
}

fn parse_message(value: &Value) -> Result<Message> {
    let author = &value["author"];
    let user = User {
        id: author["id"].as_str().unwrap_or_default().to_string(),
        username: author["username"].as_str().unwrap_or("unknown").to_string(),
        avatar: author["avatar"].as_str().map(str::to_string),
        global_name: author["global_name"].as_str().map(str::to_string),
        banner: author["banner"].as_str().map(str::to_string),
        accent_color: author["accent_color"].as_u64().map(|value| value as u32),
        avatar_decoration: author["avatar_decoration_data"]["asset"]
            .as_str()
            .map(str::to_string),
        status: Presence::Offline,
    };

    let timestamp = value["timestamp"]
        .as_str()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    // A reply carries the target as `referenced_message`; a deleted target only
    // leaves `message_reference`, which this client shows as a plain message.
    let reply_to = value.get("referenced_message").and_then(|reference| {
        Some(crate::models::ReplyRef {
            message_id: reference["id"].as_str()?.to_string(),
            channel_id: value["channel_id"].as_str().map(str::to_string),
            author: reference["author"]["username"]
                .as_str()
                .unwrap_or("someone")
                .to_string(),
            content: reference["content"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
    });

    Ok(Message {
        id: value["id"].as_str().unwrap_or_default().to_string(),
        channel_id: value["channel_id"].as_str().map(str::to_string),
        author: user,
        content: value["content"].as_str().unwrap_or_default().to_string(),
        timestamp,
        reactions: Vec::new(),
        attachments: serde_json::from_value(value["attachments"].clone()).unwrap_or_default(),
        embeds: serde_json::from_value(value["embeds"].clone()).unwrap_or_default(),
        reply_to,
    })
}

fn parse_message_update(value: &Value) -> Option<GatewayEvent> {
    Some(GatewayEvent::MessageUpdate {
        channel_id: value["channel_id"].as_str().map(str::to_string),
        id: value["id"].as_str()?.to_string(),
        content: value["content"].as_str().map(str::to_string),
        embeds: value
            .get("embeds")
            .and_then(|embeds| serde_json::from_value(embeds.clone()).ok()),
    })
}

fn parse_message_delete(value: &Value) -> Option<GatewayEvent> {
    Some(GatewayEvent::MessageDelete {
        channel_id: value["channel_id"].as_str().map(str::to_string),
        id: value["id"].as_str()?.to_string(),
    })
}

/// `VOICE_STATE_UPDATE` carries the user either as `member.user` (a guild) or as
/// `user` (a DM call), and leaves `channel_id` null when they disconnected.
fn parse_voice_state(data: &Value) -> Option<GatewayEvent> {
    let user_id = data["user_id"].as_str()?.to_string();
    if user_id.is_empty() {
        return None;
    }

    let username = data["member"]["user"]["username"]
        .as_str()
        .or_else(|| data["user"]["username"].as_str())
        .unwrap_or("unknown")
        .to_string();

    let channel_id = data["channel_id"].as_str().map(str::to_string);

    Some(GatewayEvent::VoiceState {
        channel_id,
        user_id,
        username,
    })
}

/// Custom emoji carry an `id`; Unicode ones carry the glyph in `name`.
fn emoji_label(value: &Value) -> Option<String> {
    let emoji = value.get("emoji")?;
    emoji["name"]
        .as_str()
        .or_else(|| emoji["id"].as_str())
        .filter(|label| !label.is_empty())
        .map(str::to_string)
}

fn parse_reaction(value: &Value, added: bool) -> Option<GatewayEvent> {
    Some(GatewayEvent::Reaction {
        channel_id: value["channel_id"].as_str().map(str::to_string),
        message_id: value["message_id"].as_str()?.to_string(),
        emoji: emoji_label(value)?,
        user_id: value["user_id"].as_str().map(str::to_string),
        added,
    })
}

fn parse_reactions_removed(value: &Value, emoji_only: bool) -> Option<GatewayEvent> {
    Some(GatewayEvent::ReactionsRemoved {
        channel_id: value["channel_id"].as_str().map(str::to_string),
        message_id: value["message_id"].as_str()?.to_string(),
        emoji: if emoji_only {
            Some(emoji_label(value)?)
        } else {
            None
        },
    })
}

fn parse_typing(value: &Value) -> Option<GatewayEvent> {
    // Guild typing carries the member; DM typing carries the user directly.
    let username = value["member"]["user"]["username"]
        .as_str()
        .or_else(|| value["user"]["username"].as_str())
        .unwrap_or("someone");

    Some(GatewayEvent::Typing {
        channel_id: value["channel_id"].as_str()?.to_string(),
        user_id: value["user_id"].as_str()?.to_string(),
        username: username.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disallowed_intent_close_is_recognised() {
        assert!(is_disallowed_intents(Some(4014)));
        assert!(!is_disallowed_intents(Some(4004)));
        assert!(!is_disallowed_intents(Some(1000)));
        assert!(!is_disallowed_intents(None));
    }

    #[test]
    fn a_guild_keeps_its_icon_and_member_total() {
        let (guild, _) = parse_guild(&json!({
            "id": "10",
            "name": "Rustaceans",
            "icon": "abc123",
            "member_count": 42,
            "channels": []
        }))
        .expect("guild");
        assert_eq!(guild.icon.as_deref(), Some("abc123"));
        assert_eq!(guild.member_count, Some(42));
        assert_eq!(
            guild.icon_url().as_deref(),
            Some("https://cdn.discordapp.com/icons/10/abc123.png?size=96")
        );
    }

    #[test]
    fn ready_carries_whole_guilds() {
        // Shape observed from the live gateway: READY lists full guild objects
        // (channels and all) and no GUILD_CREATE follows.
        let payload = json!({
            "op": 0,
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "bot" },
                "guilds": [
                    {
                        "id": "900",
                        "name": "Available",
                        "channels": [
                            { "id": "901", "name": "general", "type": 0 },
                            { "id": "902", "name": "voice", "type": 2 }
                        ]
                    },
                    { "id": "903", "unavailable": true }
                ]
            }
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&payload, &tx);

        match rx.try_recv().expect("ready") {
            GatewayEvent::Ready { guild_count, .. } => assert_eq!(guild_count, 2),
            other => panic!("expected Ready, got {other:?}"),
        }

        match rx.try_recv().expect("guild from ready") {
            GatewayEvent::GuildCreate { guild, channels } => {
                assert_eq!(guild.name, "Available");
                assert_eq!(channels.len(), 2);
            }
            other => panic!("expected GuildCreate, got {other:?}"),
        }

        // The unavailable stub is not a guild we can draw.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn parses_a_reply_reference() {
        let payload = json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "d": {
                "id": "101",
                "channel_id": "300",
                "content": "replying",
                "timestamp": "2026-01-01T00:00:00.000000+00:00",
                "author": { "id": "2", "username": "bob", "avatar": null },
                "referenced_message": {
                    "id": "100",
                    "content": "the original",
                    "author": { "id": "1", "username": "alice", "avatar": null }
                }
            }
        });

        let event = parse_message(&payload["d"]).expect("message");
        let reply = event.reply_to.expect("reply reference");
        assert_eq!(reply.message_id, "100");
        assert_eq!(reply.author, "alice");
        assert_eq!(reply.preview(40), "the original");
        assert_eq!(reply.channel_id.as_deref(), Some("300"));

        // A deleted target leaves only `message_reference`, so no preview.
        let orphan = json!({
            "id": "102",
            "channel_id": "300",
            "content": "still here",
            "timestamp": "2026-01-01T00:00:00.000000+00:00",
            "author": { "id": "2", "username": "bob", "avatar": null },
            "message_reference": { "message_id": "99" }
        });
        assert!(parse_message(&orphan).expect("message").reply_to.is_none());
    }

    #[test]
    fn parses_a_voice_join_and_a_leave() {
        let joined = json!({
            "op": 0,
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": "1",
                "channel_id": "50",
                "user_id": "7",
                "member": { "user": { "username": "alice" } }
            }
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&joined, &tx);

        match rx.try_recv().expect("voice state") {
            GatewayEvent::VoiceState {
                channel_id,
                user_id,
                username,
            } => {
                assert_eq!(channel_id.as_deref(), Some("50"));
                assert_eq!(user_id, "7");
                assert_eq!(username, "alice");
            }
            other => panic!("expected VoiceState, got {other:?}"),
        }

        // Leaving sends a null channel; a DM call carries `user` instead of `member`.
        let left = json!({
            "op": 0,
            "t": "VOICE_STATE_UPDATE",
            "d": { "channel_id": null, "user_id": "7", "user": { "username": "alice" } }
        });
        handle_payload(&left, &tx);
        match rx.try_recv().expect("voice state") {
            GatewayEvent::VoiceState { channel_id, .. } => assert!(channel_id.is_none()),
            other => panic!("expected VoiceState, got {other:?}"),
        }

        // A payload without a user id is not an event.
        handle_payload(&json!({ "op": 0, "t": "VOICE_STATE_UPDATE", "d": {} }), &tx);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn normalizes_bot_token_prefix() {
        let client = GatewayClient::new("abc.def", 0);
        assert_eq!(client.token, "Bot abc.def");

        let client = GatewayClient::new("Bot abc.def", 0);
        assert_eq!(client.token, "Bot abc.def");
    }

    #[test]
    fn parses_message_create_payload() {
        let payload = json!({
            "op": 0,
            "s": 42,
            "t": "MESSAGE_CREATE",
            "d": {
                "id": "100",
                "channel_id": "300",
                "content": "hello from gateway",
                "timestamp": "2026-01-01T00:00:00.000000+00:00",
                "author": {
                    "id": "200",
                    "username": "alice",
                    "avatar": null
                }
            }
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&payload, &tx);

        match rx.try_recv().expect("expected gateway event") {
            GatewayEvent::Message(message) => {
                assert_eq!(message.content, "hello from gateway");
                assert_eq!(message.author.username, "alice");
                assert_eq!(message.id, "100");
                assert_eq!(message.channel_id.as_deref(), Some("300"));
            }
            other => panic!("expected Message event, got {other:?}"),
        }
    }

    fn dispatch(payload: serde_json::Value) -> Option<GatewayEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&payload, &tx);
        rx.try_recv().ok()
    }

    #[test]
    fn parses_message_update_with_content_and_embeds() {
        let event = dispatch(json!({
            "op": 0,
            "t": "MESSAGE_UPDATE",
            "d": {
                "id": "100",
                "channel_id": "300",
                "content": "edited",
                "embeds": [{ "title": "a link", "color": 5793266 }]
            }
        }))
        .expect("event");

        match event {
            GatewayEvent::MessageUpdate {
                id,
                channel_id,
                content,
                embeds,
            } => {
                assert_eq!(id, "100");
                assert_eq!(channel_id.as_deref(), Some("300"));
                assert_eq!(content.as_deref(), Some("edited"));
                let embeds = embeds.expect("embeds");
                assert_eq!(embeds.len(), 1);
                assert_eq!(embeds[0].title.as_deref(), Some("a link"));
            }
            other => panic!("expected MessageUpdate, got {other:?}"),
        }
    }

    #[test]
    fn a_partial_message_update_is_still_parsed() {
        let event =
            dispatch(json!({ "t": "MESSAGE_UPDATE", "d": { "id": "100" } })).expect("event");

        match event {
            GatewayEvent::MessageUpdate {
                id,
                content,
                embeds,
                ..
            } => {
                assert_eq!(id, "100");
                assert!(content.is_none());
                assert!(embeds.is_none());
            }
            other => panic!("expected MessageUpdate, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_delete() {
        let event = dispatch(json!({
            "t": "MESSAGE_DELETE",
            "d": { "id": "100", "channel_id": "300" }
        }))
        .expect("event");

        match event {
            GatewayEvent::MessageDelete { id, channel_id } => {
                assert_eq!(id, "100");
                assert_eq!(channel_id.as_deref(), Some("300"));
            }
            other => panic!("expected MessageDelete, got {other:?}"),
        }
    }

    #[test]
    fn parses_unicode_and_custom_reactions() {
        let unicode = dispatch(json!({
            "t": "MESSAGE_REACTION_ADD",
            "d": {
                "user_id": "1", "message_id": "100", "channel_id": "300",
                "emoji": { "name": "🔥", "id": null }
            }
        }))
        .expect("event");

        match unicode {
            GatewayEvent::Reaction {
                emoji,
                user_id,
                added,
                ..
            } => {
                assert_eq!(emoji, "🔥");
                assert_eq!(user_id.as_deref(), Some("1"));
                assert!(added);
            }
            other => panic!("expected Reaction, got {other:?}"),
        }

        let custom = dispatch(json!({
            "t": "MESSAGE_REACTION_ADD",
            "d": {
                "user_id": "2", "message_id": "100", "channel_id": "300",
                "emoji": { "name": "party", "id": "555" }
            }
        }))
        .expect("event");

        match custom {
            GatewayEvent::Reaction { emoji, .. } => assert_eq!(emoji, "party"),
            other => panic!("expected Reaction, got {other:?}"),
        }
    }

    #[test]
    fn a_removal_and_a_removal_of_everything_are_distinct() {
        let removed = dispatch(json!({
            "t": "MESSAGE_REACTION_REMOVE",
            "d": {
                "user_id": "1", "message_id": "100", "channel_id": "300",
                "emoji": { "name": "🔥" }
            }
        }))
        .expect("event");

        match removed {
            GatewayEvent::Reaction { added, .. } => assert!(!added),
            other => panic!("expected Reaction, got {other:?}"),
        }

        let all = dispatch(json!({
            "t": "MESSAGE_REACTION_REMOVE_ALL",
            "d": { "message_id": "100", "channel_id": "300" }
        }))
        .expect("event");

        match all {
            GatewayEvent::ReactionsRemoved {
                emoji, message_id, ..
            } => {
                assert!(emoji.is_none());
                assert_eq!(message_id, "100");
            }
            other => panic!("expected ReactionsRemoved, got {other:?}"),
        }

        let one = dispatch(json!({
            "t": "MESSAGE_REACTION_REMOVE_EMOJI",
            "d": {
                "message_id": "100", "channel_id": "300",
                "emoji": { "name": "🎨", "id": "77" }
            }
        }))
        .expect("event");

        match one {
            GatewayEvent::ReactionsRemoved { emoji, .. } => {
                assert_eq!(emoji.as_deref(), Some("🎨"));
            }
            other => panic!("expected ReactionsRemoved, got {other:?}"),
        }
    }

    #[test]
    fn parses_typing_from_a_guild_and_a_dm() {
        let guild = dispatch(json!({
            "t": "TYPING_START",
            "d": {
                "channel_id": "300", "user_id": "1",
                "member": { "user": { "username": "alice" } }
            }
        }))
        .expect("event");

        match guild {
            GatewayEvent::Typing { username, .. } => assert_eq!(username, "alice"),
            other => panic!("expected Typing, got {other:?}"),
        }

        let dm = dispatch(json!({
            "t": "TYPING_START",
            "d": { "channel_id": "300", "user_id": "2", "user": { "username": "bob" } }
        }))
        .expect("event");

        match dm {
            GatewayEvent::Typing { username, .. } => assert_eq!(username, "bob"),
            other => panic!("expected Typing, got {other:?}"),
        }
    }

    #[test]
    fn malformed_lifecycle_payloads_do_not_produce_events() {
        assert!(dispatch(json!({ "t": "MESSAGE_UPDATE", "d": {} })).is_none());
        assert!(dispatch(json!({ "t": "MESSAGE_DELETE", "d": {} })).is_none());
        assert!(
            dispatch(json!({ "t": "MESSAGE_REACTION_ADD", "d": { "message_id": "1" } })).is_none()
        );
        assert!(dispatch(json!({ "t": "MESSAGE_REACTION_REMOVE_ALL", "d": {} })).is_none());
        assert!(dispatch(json!({ "t": "TYPING_START", "d": {} })).is_none());
        // An event this client does not know is ignored, not an error.
        assert!(dispatch(json!({ "t": "SOMETHING_NEW", "d": {} })).is_none());
    }

    #[test]
    fn parses_guild_create_channels() {
        let payload = json!({
            "op": 0,
            "s": 2,
            "t": "GUILD_CREATE",
            "d": {
                "id": "999",
                "name": "Test Guild",
                "icon": null,
                "channels": [
                    { "id": "1", "name": "general", "type": 0 },
                    { "id": "2", "name": "voice", "type": 2 },
                    { "id": "3", "name": "Category", "type": 4 },
                    { "id": "4", "name": "announcement", "type": 5 }
                ]
            }
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&payload, &tx);

        match rx.try_recv().expect("expected guild event") {
            GatewayEvent::GuildCreate { guild, channels } => {
                assert_eq!(guild.id, "999");
                assert_eq!(guild.name, "Test Guild");
                assert_eq!(channels.len(), 3);
                assert_eq!(channels[0].kind, ChannelKind::Text);
                assert_eq!(channels[1].kind, ChannelKind::Voice);
                assert_eq!(channels[2].kind, ChannelKind::Category);
            }
            other => panic!("expected GuildCreate event, got {other:?}"),
        }
    }

    #[test]
    fn parses_ready_payload() {
        let payload = json!({
            "op": 0,
            "s": 1,
            "t": "READY",
            "d": {
                "user": {
                    "id": "900",
                    "username": "rustcord-bot",
                    "global_name": "Rustcord",
                    "avatar": "hash",
                    "avatar_decoration_data": { "asset": "deco" }
                },
                "guilds": [{}, {}, {}]
            }
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_payload(&payload, &tx);

        match rx.try_recv().expect("expected ready event") {
            GatewayEvent::Ready {
                username,
                user_id,
                avatar,
                global_name,
                avatar_decoration,
                guild_count,
            } => {
                assert_eq!(username, "rustcord-bot");
                assert_eq!(user_id, "900");
                assert_eq!(avatar.as_deref(), Some("hash"));
                assert_eq!(global_name.as_deref(), Some("Rustcord"));
                assert_eq!(avatar_decoration.as_deref(), Some("deco"));
                assert_eq!(guild_count, 3);
            }
            other => panic!("expected Ready event, got {other:?}"),
        }
    }

    #[test]
    fn backoff_doubles_and_caps_at_one_minute() {
        assert_eq!(backoff_secs(1), 2);
        assert_eq!(backoff_secs(2), 4);
        assert_eq!(backoff_secs(5), 32);
        assert_eq!(backoff_secs(6), 60);
        assert_eq!(backoff_secs(50), 60);
    }

    #[test]
    fn resume_url_gets_required_query() {
        assert_eq!(
            normalize_resume_url("wss://gateway.discord.gg"),
            "wss://gateway.discord.gg/?v=10&encoding=json"
        );
        assert_eq!(
            normalize_resume_url("wss://gateway.discord.gg/?v=10&encoding=json"),
            "wss://gateway.discord.gg/?v=10&encoding=json"
        );
    }
}
