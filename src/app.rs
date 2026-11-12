use crate::api::discord::DiscordClient;
use crate::api::gateway::{GatewayClient, GatewayEvent, GatewayHandle, DEFAULT_INTENTS};
use crate::api::oauth::SessionStore as _;
use crate::auth;
use crate::diagnostics;
use crate::models::{Channel, ChannelKind, Guild, Message, Presence, Reaction, User};
use crate::monitor::MemoryMonitor;
use crate::plugins::PluginManager;
use crate::settings::Settings;
use crate::shortcuts::ShortcutAction;
use crate::sysinfo;
use crate::ui;
use crate::ui::settings::SettingsUiState;
use crate::ui::virtual_list::RowMetrics;
use chrono::{Duration as ChronoDuration, Utc};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// Messages requested per history page (Discord's maximum).
const HISTORY_PAGE: u8 = 100;

/// Transient in-app banner shown above the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Info,
    Warn,
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub text: String,
    pub kind: AlertKind,
    /// Wall-clock deadline from `egui`'s input time, in seconds.
    /// `f64::MAX` means "stamp me on the next frame".
    pub until: f64,
    /// How long the banner should stay up, in seconds.
    pub duration: f64,
}

pub struct RustcordApp {
    pub(crate) settings: Settings,
    pub(crate) guilds: Vec<Guild>,
    pub(crate) channels: Vec<Channel>,
    pub(crate) messages: Vec<Message>,
    pub(crate) selected_guild: Option<String>,
    pub(crate) selected_channel: Option<String>,
    pub(crate) draft: String,
    pub(crate) plugins: PluginManager,
    pub(crate) current_user: User,
    pub(crate) show_settings: bool,
    pub(crate) status_line: Option<String>,
    pub(crate) gateway: Option<GatewayHandle>,
    pub(crate) runtime: Option<tokio::runtime::Runtime>,
    pub(crate) gateway_status: String,
    pub(crate) token_draft: String,
    pub(crate) token_status: String,
    pub(crate) live_data: bool,
    pub(crate) login: crate::ui::login::LoginState,
    /// Who is typing, per channel. Expires on its own; no timer or thread.
    pub(crate) typing: crate::models::TypingTracker,
    /// The bot's own user id from READY, so a reaction can be marked as ours.
    pub(crate) bot_user_id: Option<String>,
    /// OAuth2 *identity* (username, avatar, id). Never used for messaging.
    pub(crate) identity: Option<crate::api::oauth::Session>,
    pub(crate) identity_expired: bool,
    pub(crate) oauth: OAuthFlow,
    /// Local search overlay over the loaded history of the open channel.
    pub(crate) search: crate::search::SearchState,
    /// Who is in which voice channel, from `VOICE_STATE_UPDATE`.
    pub(crate) voice: crate::models::VoiceTracker,
    /// The message being edited, if the composer is in edit mode.
    pub(crate) editing: Option<String>,
    /// The message the next send will reference, if any.
    pub(crate) replying_to: Option<crate::models::ReplyRef>,
    /// A delete waiting for confirmation.
    pub(crate) pending_delete: Option<String>,
    action_tx: Sender<MessageAction>,
    action_results: Receiver<MessageAction>,
    /// A refresh-token exchange in flight for the identity session.
    identity_refresh: Option<IdentityRefresh>,
    pub(crate) monitor: MemoryMonitor,
    pub(crate) settings_ui: SettingsUiState,
    pub(crate) alert: Option<Alert>,
    pub(crate) frames: u64,
    applied_ui_scale: f32,
    /// Hash of the palette currently installed in egui. Guards against
    /// re-applying visuals every frame, which would request a repaint each
    /// time and turn the idle app into a spin loop.
    applied_theme: Option<u64>,
    frames_at_last_tick: u64,
    /// Decoded attachment/embed textures, byte-bounded and LRU-evicted.
    pub(crate) images: crate::images::ImageLoader,
    /// Pagination state per channel id, so switching back does not refetch.
    pub(crate) history: HashMap<String, ChannelHistory>,
    /// Arrow-key focus over the rail and channel list (no overlay).
    pub(crate) navigator: crate::nav::Navigator,
    /// Folders currently expanded in the rail. Session-only; a folder opens
    /// collapsed again next launch, like Discord's.
    pub(crate) rail_expanded: std::collections::BTreeSet<String>,
    /// Ctrl+K overlay state (query, cursor).
    pub(crate) switcher: crate::switcher::SwitcherState,
    /// Locally tracked unread/mention counters; bots get no server read state.
    pub(crate) unread: crate::models::UnreadTracker,
    /// Guild rosters, fetched over REST with the Members intent. Keyed by guild.
    pub(crate) members: std::collections::BTreeMap<String, Vec<User>>,
    members_loading: std::collections::BTreeSet<String>,
    /// Set when Discord refuses the privileged intent, so the UI can say why the
    /// roster is empty instead of retrying every frame.
    pub(crate) member_intent_missing: bool,
    member_tx: Sender<MemberResult>,
    member_results: Receiver<MemberResult>,
    /// Measured row heights for the virtualised message list.
    pub(crate) metrics: RowMetrics,
    pub(crate) metrics_channel: Option<String>,
    history_tx: Sender<HistoryResult>,
    history_rx: Receiver<HistoryResult>,
    send_tx: Sender<SendResult>,
    send_results: Receiver<SendResult>,
}

/// An OAuth2 identity sign-in in progress.
#[derive(Default)]
pub(crate) struct OAuthFlow {
    pub(crate) pending: Option<PendingOAuth>,
    pub(crate) status: Option<String>,
    pub(crate) error: Option<String>,
    /// Typed into the login page when no client id is configured yet.
    pub(crate) client_id_draft: String,
}

struct IdentityRefresh {
    rx: Receiver<Result<crate::api::oauth::Session, String>>,
}

pub(crate) struct PendingOAuth {
    callback: crate::api::oauth::BrowserCallback,
    verifier: String,
    client_id: String,
    /// Set once the browser handed back a code and the exchange thread started.
    session_rx: Option<Receiver<Result<crate::api::oauth::Session, String>>>,
    started: std::time::Instant,
}

/// A guild roster arriving from the REST call.
enum MemberResult {
    Loaded {
        guild_id: String,
        members: Vec<User>,
    },
    Failed {
        guild_id: String,
        error: String,
    },
}

/// Result of an outbound edit or delete, applied on the UI thread.
enum MessageAction {
    Edited(Box<Message>),
    Deleted(String),
    ReactFailed {
        message_id: String,
        emoji: String,
        removed: bool,
        error: String,
    },
    Failed(String),
}

enum SendResult {
    /// Boxed: `Message` now carries attachments and embeds, so the inline
    /// variant would dwarf `Failed` and bloat every `SendResult` on the wire.
    Sent(Box<Message>),
    Failed(String),
}

/// Per-channel history pagination state.
#[derive(Debug, Clone, Default)]
pub struct ChannelHistory {
    pub loading: bool,
    /// At least one page has been fetched successfully.
    pub loaded: bool,
    /// Discord returned a short page: there is nothing older to fetch.
    pub exhausted: bool,
    pub error: Option<String>,
}

struct HistoryPage {
    messages: Vec<Message>,
    exhausted: bool,
}

struct HistoryResult {
    channel_id: String,
    result: Result<HistoryPage, String>,
}

impl RustcordApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mut settings: Settings) -> Self {
        let mut plugins = PluginManager::new();
        plugins.init_all(&mut settings);

        let theme = plugins.theme().clone();
        theme.apply(&cc.egui_ctx);

        let guilds = vec![
            Guild {
                id: "1".to_string(),
                name: "Rustcord Lounge".to_string(),
                icon: None,
            },
            Guild {
                id: "2".to_string(),
                name: "Arch Linux".to_string(),
                icon: None,
            },
            Guild {
                id: "3".to_string(),
                name: "Rustaceans".to_string(),
                icon: None,
            },
        ];

        let channels = vec![
            Channel {
                id: "10".to_string(),
                name: "general".to_string(),
                kind: ChannelKind::Text,
                guild_id: "1".to_string(),
            },
            Channel {
                id: "11".to_string(),
                name: "dev".to_string(),
                kind: ChannelKind::Text,
                guild_id: "1".to_string(),
            },
            Channel {
                id: "12".to_string(),
                name: "voice-lounge".to_string(),
                kind: ChannelKind::Voice,
                guild_id: "1".to_string(),
            },
            Channel {
                id: "13".to_string(),
                name: "off-topic".to_string(),
                kind: ChannelKind::Text,
                guild_id: "1".to_string(),
            },
            Channel {
                id: "20".to_string(),
                name: "pacman".to_string(),
                kind: ChannelKind::Text,
                guild_id: "2".to_string(),
            },
            Channel {
                id: "21".to_string(),
                name: "wayland".to_string(),
                kind: ChannelKind::Text,
                guild_id: "2".to_string(),
            },
            Channel {
                id: "30".to_string(),
                name: "cargo".to_string(),
                kind: ChannelKind::Text,
                guild_id: "3".to_string(),
            },
        ];

        let selected_guild = guilds.first().map(|guild| guild.id.clone());
        let selected_channel = Some("10".to_string());

        let current_user = User {
            id: "self".to_string(),
            username: "jason".to_string(),
            avatar: None,
            status: Presence::Online,
        };

        let messages = demo_messages(&current_user);

        let token_draft = auth::load_discord_token().unwrap_or_default();
        let token_status = if token_draft.is_empty() {
            "No bot token stored in the system keyring".to_string()
        } else {
            "Bot token loaded from the system keyring".to_string()
        };

        let (runtime, gateway, gateway_status) = if token_draft.trim().is_empty() {
            (
                None,
                None,
                "No bot token stored — using local demo data".to_string(),
            )
        } else {
            match start_gateway(&token_draft) {
                Ok((runtime, gateway)) => (
                    Some(runtime),
                    Some(gateway),
                    "Connecting to Discord gateway...".to_string(),
                ),
                Err(err) => (None, None, format!("Could not start gateway: {err:#}")),
            }
        };

        // Restore a stored OAuth2 identity session. Expired sessions are still
        // shown (so the account is recognisable) but flagged for re-sign-in.
        let (identity_session, identity_expired) = match crate::api::oauth::restore(
            &crate::api::oauth::KeyringStore,
            crate::api::oauth::now_unix(),
        ) {
            crate::api::oauth::Restore::Missing => (None, false),
            crate::api::oauth::Restore::Active(session) => (Some(session), false),
            crate::api::oauth::Restore::Expired(session) => (Some(session), true),
        };

        // First run (nothing in the keyring) opens on the sign-in page; a
        // configured install goes straight to the workspace.
        let login = crate::ui::login::LoginState::new(token_draft.trim().is_empty());

        let (send_tx, send_results) = mpsc::channel();
        let (history_tx, history_rx) = mpsc::channel();
        let (action_tx, action_results) = mpsc::channel();
        let (member_tx, member_results) = mpsc::channel();

        Self {
            settings,
            guilds,
            channels,
            messages,
            selected_guild,
            selected_channel,
            draft: String::new(),
            plugins,
            current_user,
            show_settings: false,
            status_line: Some("demo history loaded".to_string()),
            gateway,
            runtime,
            gateway_status,
            token_draft,
            token_status,
            live_data: false,
            login,
            typing: crate::models::TypingTracker::default(),
            bot_user_id: None,
            identity: identity_session,
            identity_expired,
            oauth: OAuthFlow::default(),
            search: crate::search::SearchState::default(),
            switcher: crate::switcher::SwitcherState::default(),
            rail_expanded: std::collections::BTreeSet::new(),
            navigator: crate::nav::Navigator::default(),
            voice: crate::models::VoiceTracker::default(),
            editing: None,
            replying_to: None,
            pending_delete: None,
            action_tx,
            action_results,
            identity_refresh: None,
            monitor: MemoryMonitor::new(),
            settings_ui: SettingsUiState::default(),
            alert: None,
            frames: 0,
            applied_ui_scale: 0.0,
            applied_theme: None,
            frames_at_last_tick: 0,
            images: crate::images::ImageLoader::new(
                usize::try_from(crate::settings::DEFAULT_IMAGE_CACHE_BYTES).unwrap_or(0),
            ),
            history: HashMap::new(),
            unread: crate::models::UnreadTracker::default(),
            members: std::collections::BTreeMap::new(),
            members_loading: std::collections::BTreeSet::new(),
            member_intent_missing: false,
            member_tx,
            member_results,
            metrics: RowMetrics::new(),
            metrics_channel: None,
            history_tx,
            history_rx,
            send_tx,
            send_results,
        }
    }

    // -----------------------------------------------------------------------
    // Settings integration
    // -----------------------------------------------------------------------

    /// Persist and apply a settings change: normalise, rebuild the theme, save.
    pub(crate) fn settings_changed(&mut self) {
        self.settings.normalize();
        self.plugins.on_settings(&mut self.settings);
        self.settings_ui.hex_draft.clear();
        if let Err(err) = self.settings.save() {
            log::warn!("could not persist settings: {err:#}");
            self.status_line = Some(format!("could not save settings: {err:#}"));
        }
    }

    /// Install the current palette, but only when it actually changed.
    ///
    /// `egui::Context::set_visuals` calls `request_repaint`, so applying the
    /// theme on every frame keeps the event loop awake forever: the app idles
    /// at 0% CPU until the window is mapped, then burns ~5–8% of a core doing
    /// nothing. The signature makes the call idempotent.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        let theme = self.plugins.theme();
        let signature = theme_signature(theme);
        if self.applied_theme != Some(signature) {
            let theme = theme.clone();
            theme.apply(ctx);
            self.applied_theme = Some(signature);
        }
    }

    /// Keep the cache budget in step with the setting without reallocating on
    /// every frame (`set_budget` is a no-op comparison here).
    fn sync_image_budget(&mut self) {
        let bytes =
            usize::try_from(u64::from(self.settings.image_cache_mb) * 1024 * 1024).unwrap_or(0);
        if self.images.budget() != bytes {
            self.images.set_budget(bytes);
        }
    }

    fn apply_ui_scale(&mut self, ctx: &egui::Context) {
        if (self.applied_ui_scale - self.settings.ui_scale).abs() > f32::EPSILON {
            ctx.set_pixels_per_point(self.settings.ui_scale);
            self.applied_ui_scale = self.settings.ui_scale;
        }
    }

    pub(crate) fn push_alert(&mut self, text: String, kind: AlertKind, seconds: f64) {
        self.alert = Some(Alert {
            text,
            kind,
            // Stamped against the egui clock on the next frame, so the banner
            // never inherits a stale wall-clock baseline.
            until: f64::MAX,
            duration: seconds.max(1.0),
        });
    }

    pub(crate) fn dismiss_alert(&mut self) {
        self.alert = None;
    }

    /// Clamp the message buffer to the configured history cap.
    pub(crate) fn trim_history(&mut self) {
        let cap = self.settings.resource_priority.history_cap();
        if self.messages.len() > cap {
            let excess = self.messages.len() - cap;
            self.messages.drain(0..excess);
        }
    }

    // -----------------------------------------------------------------------
    // Channel history
    // -----------------------------------------------------------------------

    /// Fetch the first page for the selected channel exactly once.
    ///
    /// Called every frame; the guards make it a no-op once the channel is
    /// loaded, loading, exhausted, or when running on demo data.
    pub(crate) fn ensure_history(&mut self) {
        if !self.live_data || self.gateway.is_none() {
            return;
        }
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        let is_text = self
            .channels
            .iter()
            .any(|channel| channel.id == channel_id && channel.kind == ChannelKind::Text);
        if !is_text {
            return;
        }

        if let Some(state) = self.history.get(&channel_id) {
            if state.loading || state.loaded || state.exhausted {
                return;
            }
        }
        self.start_history_fetch(channel_id, None);
    }

    /// The roster for the selected guild, once fetched.
    pub(crate) fn roster(&self) -> &[User] {
        self.selected_guild
            .as_ref()
            .and_then(|id| self.members.get(id))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Fetch the selected guild's roster once. The Members intent is privileged;
    /// `member_intent_missing` stops the retry loop when Discord refuses it.
    fn ensure_members(&mut self) {
        if !self.live_data || self.member_intent_missing {
            return;
        }
        let Some(guild_id) = self.selected_guild.clone() else {
            return;
        };
        if self.members.contains_key(&guild_id) || self.members_loading.contains(&guild_id) {
            return;
        }

        let Some(runtime) = &self.runtime else {
            return;
        };

        let token = self.token_draft.clone();
        let tx = self.member_tx.clone();
        self.members_loading.insert(guild_id.clone());

        runtime.spawn(async move {
            let result = match DiscordClient::new(token) {
                Ok(client) => client
                    .fetch_members(&guild_id)
                    .await
                    .map_err(|err| format!("{err:#}")),
                Err(err) => Err(format!("{err:#}")),
            };

            let _ = tx.send(match result {
                Ok(members) => MemberResult::Loaded { guild_id, members },
                Err(error) => MemberResult::Failed { guild_id, error },
            });
        });
    }

    fn poll_members(&mut self) {
        while let Ok(result) = self.member_results.try_recv() {
            match result {
                MemberResult::Loaded { guild_id, members } => {
                    self.members_loading.remove(&guild_id);
                    self.members.insert(guild_id, members);
                }
                MemberResult::Failed { guild_id, error } => {
                    self.members_loading.remove(&guild_id);
                    self.status_line = Some(format!("member list unavailable: {error}"));
                }
            }
        }
    }

    /// Request the page before the oldest message currently held.
    pub(crate) fn load_older(&mut self) {
        let Some(channel_id) = self.selected_channel.clone() else {
            return;
        };
        let oldest = self
            .messages
            .iter()
            .filter(|message| message.channel_id.as_deref() == Some(channel_id.as_str()))
            .min_by(|a, b| a.timestamp.cmp(&b.timestamp))
            .map(|message| message.id.clone());

        // With no messages yet, `before: None` loads the newest page.
        self.start_history_fetch(channel_id, oldest);
    }

    fn start_history_fetch(&mut self, channel_id: String, before: Option<String>) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        if self.token_draft.trim().is_empty() {
            return;
        }

        if let Some(state) = self.history.get_mut(&channel_id) {
            state.loading = true;
            state.error = None;
        } else {
            self.history.insert(
                channel_id.clone(),
                ChannelHistory {
                    loading: true,
                    ..ChannelHistory::default()
                },
            );
        }

        let token = self.token_draft.clone();
        let tx = self.history_tx.clone();
        runtime.spawn(async move {
            let result = match DiscordClient::new(token) {
                Ok(client) => client
                    .fetch_messages(&channel_id, before.as_deref(), HISTORY_PAGE)
                    .await
                    .map(|messages| HistoryPage {
                        exhausted: messages.len() < usize::from(HISTORY_PAGE),
                        messages,
                    })
                    .map_err(|err| format!("{err:#}")),
                Err(err) => Err(format!("{err:#}")),
            };
            let _ = tx.send(HistoryResult { channel_id, result });
        });
    }

    fn poll_history(&mut self) {
        let mut added_total = 0usize;

        while let Ok(HistoryResult { channel_id, result }) = self.history_rx.try_recv() {
            match result {
                Ok(page) => {
                    let mut added = 0;
                    for message in page.messages {
                        if !self
                            .messages
                            .iter()
                            .any(|existing| existing.id == message.id)
                        {
                            self.messages.push(message);
                            added += 1;
                        }
                    }
                    added_total += added;

                    if let Some(state) = self.history.get_mut(&channel_id) {
                        state.loading = false;
                        state.loaded = true;
                        state.exhausted = page.exhausted;
                        state.error = None;
                    }
                    self.status_line = Some(format!("loaded {added} message(s)"));
                }
                Err(err) => {
                    if let Some(state) = self.history.get_mut(&channel_id) {
                        state.loading = false;
                        state.error = Some(err.clone());
                    }
                    self.status_line = Some(format!("history unavailable: {err}"));
                }
            }
        }

        if added_total > 0 {
            // Pages arrive newest-first within a page but older pages arrive
            // later, so keep the buffer in chronological order.
            self.messages.sort_by_key(|message| message.timestamp);
            self.trim_history();
            self.mark_search_stale();
        }
    }

    pub(crate) fn channel_history(&self, channel_id: &str) -> Option<&ChannelHistory> {
        self.history.get(channel_id)
    }

    // -----------------------------------------------------------------------
    // Message actions
    // -----------------------------------------------------------------------

    /// Only our own messages are editable or deletable: the invite requests no
    /// Manage Messages permission, so Discord would refuse anything else.
    pub(crate) fn can_modify(&self, message: &Message) -> bool {
        if !self.live_data {
            return message.author.username == self.current_user.username;
        }
        message.author.username == self.current_user.username
            || self.bot_user_id.as_deref() == Some(message.author.id.as_str())
    }

    pub(crate) fn begin_edit(&mut self, message: &Message) {
        self.replying_to = None;
        self.editing = Some(message.id.clone());
        self.draft = message.content.clone();
        self.status_line = Some("editing message — Enter saves, Esc cancels".to_string());
    }

    pub(crate) fn cancel_edit(&mut self) {
        if self.editing.take().is_some() {
            self.draft.clear();
            self.status_line = Some("edit cancelled".to_string());
        }
    }

    /// Reply to any message, ours or not — that needs no extra permission.
    pub(crate) fn begin_reply(&mut self, message: &Message) {
        self.cancel_edit();
        self.replying_to = Some(crate::models::ReplyRef::new(message));
        self.status_line = Some(format!("replying to {}", message.author.username));
    }

    pub(crate) fn cancel_reply(&mut self) {
        if self.replying_to.take().is_some() {
            self.status_line = Some("reply cancelled".to_string());
        }
    }

    /// One click on a chip toggles our reaction: add it when we have not reacted,
    /// remove it when we have. Both directions are optimistic and both roll back
    /// if the request fails, so the chip never lies about the server state.
    pub(crate) fn react_to(&mut self, message_id: String, emoji: String) {
        let Some(message) = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
        else {
            return;
        };

        let removing = message
            .reactions
            .iter()
            .any(|reaction| reaction.emoji == emoji && reaction.me);

        if removing {
            message.remove_own_reaction(&emoji);
        } else if !message.add_own_reaction(&emoji) {
            return;
        }

        let verb = if removing { "removed" } else { "reacted with" };

        if !self.live_data {
            self.status_line = Some(format!("{verb} {emoji} (demo)"));
            return;
        }

        let (Some(runtime), Some(channel_id)) = (&self.runtime, self.selected_channel.clone())
        else {
            return;
        };

        let token = self.token_draft.clone();
        let tx = self.action_tx.clone();
        let id = message_id.clone();
        let reaction = emoji.clone();
        runtime.spawn(async move {
            let result = match DiscordClient::new(token) {
                Ok(client) if removing => client.remove_reaction(&channel_id, &id, &reaction).await,
                Ok(client) => client.add_reaction(&channel_id, &id, &reaction).await,
                Err(err) => Err(err),
            }
            .map_err(|err| format!("{err:#}"));

            if let Err(error) = result {
                let _ = tx.send(MessageAction::ReactFailed {
                    message_id: id,
                    emoji: reaction,
                    removed: removing,
                    error,
                });
            }
        });

        self.status_line = Some(format!("{verb} {emoji}"));
    }

    pub(crate) fn request_delete(&mut self, message_id: String) {
        self.pending_delete = Some(message_id);
    }

    /// Send the PATCH for the message being edited. Demo mode edits locally.
    fn submit_edit(&mut self, message_id: String) {
        let Some(content) = crate::api::discord::valid_content(&self.draft) else {
            self.status_line = Some("a message needs 1–2000 characters".to_string());
            return;
        };
        let content = content.to_string();

        if !self.live_data {
            if let Some(message) = self.messages.iter_mut().find(|m| m.id == message_id) {
                message.content = content;
            }
            self.metrics.clear();
            self.editing = None;
            self.draft.clear();
            self.status_line = Some("message edited (demo)".to_string());
            self.mark_search_stale();
            return;
        }

        let (Some(runtime), Some(channel_id)) = (&self.runtime, self.selected_channel.clone())
        else {
            return;
        };

        let token = self.token_draft.clone();
        let tx = self.action_tx.clone();
        runtime.spawn(async move {
            let result = match DiscordClient::new(token) {
                Ok(client) => client
                    .edit_message(&channel_id, &message_id, &content)
                    .await
                    .map_err(|err| format!("{err:#}")),
                Err(err) => Err(format!("{err:#}")),
            };

            let _ = tx.send(match result {
                Ok(message) => MessageAction::Edited(Box::new(message)),
                Err(err) => MessageAction::Failed(err),
            });
        });

        self.editing = None;
        self.draft.clear();
        self.status_line = Some("saving edit…".to_string());
    }

    /// Delete after confirmation.
    pub(crate) fn delete_message(&mut self, message_id: String) {
        self.pending_delete = None;

        if !self.live_data {
            crate::models::remove_message(&mut self.messages, &message_id, None);
            self.metrics.clear();
            self.status_line = Some("message deleted (demo)".to_string());
            self.mark_search_stale();
            return;
        }

        let (Some(runtime), Some(channel_id)) = (&self.runtime, self.selected_channel.clone())
        else {
            return;
        };

        let token = self.token_draft.clone();
        let tx = self.action_tx.clone();
        let id = message_id.clone();
        runtime.spawn(async move {
            let result = match DiscordClient::new(token) {
                Ok(client) => client
                    .delete_message(&channel_id, &id)
                    .await
                    .map_err(|err| format!("{err:#}")),
                Err(err) => Err(format!("{err:#}")),
            };

            let _ = tx.send(match result {
                Ok(()) => MessageAction::Deleted(id),
                Err(err) => MessageAction::Failed(err),
            });
        });

        self.status_line = Some("deleting message…".to_string());
    }

    fn poll_action_results(&mut self) {
        while let Ok(action) = self.action_results.try_recv() {
            match action {
                MessageAction::Edited(message) => {
                    if let Some(existing) = self
                        .messages
                        .iter_mut()
                        .find(|existing| existing.id == message.id)
                    {
                        *existing = *message;
                    }
                    self.metrics.clear();
                    self.status_line = Some("message edited".to_string());
                    self.mark_search_stale();
                }
                MessageAction::Deleted(id) => {
                    crate::models::remove_message(&mut self.messages, &id, None);
                    self.metrics.clear();
                    self.status_line = Some("message deleted".to_string());
                    self.mark_search_stale();
                }
                MessageAction::ReactFailed {
                    message_id,
                    emoji,
                    removed,
                    error,
                } => {
                    if let Some(message) = self
                        .messages
                        .iter_mut()
                        .find(|message| message.id == message_id)
                    {
                        // Undo whichever direction we optimistically applied.
                        if removed {
                            message.add_own_reaction(&emoji);
                        } else {
                            message.remove_own_reaction(&emoji);
                        }
                    }
                    self.status_line = Some(format!("reaction failed: {error}"));
                }
                MessageAction::Failed(err) => {
                    self.status_line = Some(format!("message action failed: {err}"));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    pub(crate) fn open_search(&mut self) {
        // One palette at a time: the switcher sits on top of the same screen,
        // and a palette owns the keys while it is open.
        self.switcher.close();
        self.navigator.stop();
        self.search.open = true;
        self.search.just_opened = true;
        self.refresh_search();
    }

    pub(crate) fn close_search(&mut self) {
        self.search.open = false;
    }

    // -----------------------------------------------------------------------
    // Quick switcher
    // -----------------------------------------------------------------------

    pub(crate) fn open_switcher(&mut self) {
        self.close_search();
        self.navigator.stop();
        self.switcher.open_now();
    }

    pub(crate) fn close_switcher(&mut self) {
        self.switcher.close();
    }

    /// A server row opens its first text channel; a channel row opens itself and
    /// its parent server. Either way the overlay closes and the channel is read.
    pub(crate) fn activate_switcher(&mut self, entry: crate::switcher::Entry) {
        self.switcher.close();
        self.selected_guild = Some(entry.guild_id.clone());

        match entry.kind {
            crate::switcher::EntryKind::Channel => {
                self.select_channel(entry.id.clone());
                self.status_line = Some(format!(
                    "jumped to {} in {}",
                    entry.primary, entry.secondary
                ));
            }
            crate::switcher::EntryKind::Guild => {
                if let Some(channel) = self
                    .channels
                    .iter()
                    .find(|channel| {
                        channel.guild_id == entry.id && channel.kind == ChannelKind::Text
                    })
                    .map(|channel| channel.id.clone())
                {
                    self.select_channel(channel);
                }
                self.status_line = Some(format!("switched to {}", entry.primary));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard navigation
    // -----------------------------------------------------------------------

    /// Channel ids in the sidebar's own order: text channels, then voice.
    fn navigable_channels(&self) -> Vec<String> {
        let selected = self.selected_guild.as_ref();
        let text = self.channels.iter().filter(|channel| {
            Some(&channel.guild_id) == selected && channel.kind == ChannelKind::Text
        });
        let voice = self.channels.iter().filter(|channel| {
            Some(&channel.guild_id) == selected && channel.kind == ChannelKind::Voice
        });
        text.chain(voice)
            .map(|channel| channel.id.clone())
            .collect()
    }

    fn rail_items(&self) -> Vec<crate::rail::RailItem> {
        self.settings.guild_rail.items(&self.guilds)
    }

    /// Start arrow-key navigation where the user already is.
    pub(crate) fn begin_navigation(&mut self) {
        let items = self.rail_items();
        let rail = self
            .selected_guild
            .as_ref()
            .and_then(|id| items.iter().position(|item| item.contains(id)))
            .unwrap_or(0);
        let channels = self.navigable_channels();
        let channel = self
            .selected_channel
            .as_ref()
            .and_then(|id| channels.iter().position(|candidate| candidate == id))
            .unwrap_or(0);

        self.navigator.begin(rail, channel);
        self.status_line = Some(
            "keyboard nav: ↑↓ move · ←/→ rail or channels · Enter open · Esc exit".to_string(),
        );
    }

    pub(crate) fn end_navigation(&mut self) {
        self.navigator.stop();
        self.status_line = Some("keyboard navigation off".to_string());
    }

    fn handle_navigation(&mut self, ctx: &egui::Context) {
        // Palettes and settings own the keyboard while they are up.
        if !self.navigator.active || self.search.open || self.switcher.open || self.show_settings {
            return;
        }
        // A focused text field owns Enter, the arrows and Escape (the composer
        // is the common case), so navigation stays out of typing's way.
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }

        let keys: Vec<crate::nav::NavKey> = ctx.input(|input| {
            use crate::nav::NavKey;
            let mut keys = Vec::new();
            if input.key_pressed(egui::Key::ArrowUp) {
                keys.push(NavKey::Up);
            }
            if input.key_pressed(egui::Key::ArrowDown) {
                keys.push(NavKey::Down);
            }
            if input.key_pressed(egui::Key::ArrowLeft) {
                keys.push(NavKey::Left);
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                keys.push(NavKey::Right);
            }
            if input.key_pressed(egui::Key::Enter) {
                keys.push(NavKey::Enter);
            }
            if input.key_pressed(egui::Key::Escape) {
                keys.push(NavKey::Escape);
            }
            keys
        });

        for key in keys {
            let items = self.rail_items();
            let channels = self.navigable_channels();

            match self.navigator.handle(key, items.len(), channels.len()) {
                crate::nav::NavOutcome::Exit => {
                    self.status_line = Some("keyboard navigation off".to_string());
                }
                crate::nav::NavOutcome::Activate => match self.navigator.area {
                    crate::nav::NavArea::Rail => match items.get(self.navigator.rail).cloned() {
                        Some(crate::rail::RailItem::Folder(folder)) => {
                            if !self.rail_expanded.remove(&folder.id) {
                                self.rail_expanded.insert(folder.id.clone());
                            }
                            self.status_line = Some(format!("folder {}", folder.name));
                        }
                        Some(crate::rail::RailItem::Guild(id)) => {
                            self.selected_guild = Some(id.clone());
                            let first = self
                                .channels
                                .iter()
                                .find(|channel| {
                                    channel.guild_id == id && channel.kind == ChannelKind::Text
                                })
                                .map(|channel| channel.id.clone());
                            if let Some(channel) = first {
                                self.select_channel(channel);
                                self.navigator.area = crate::nav::NavArea::Channels;
                                self.navigator.channel = 0;
                            }
                            let name = self
                                .guilds
                                .iter()
                                .find(|guild| guild.id == id)
                                .map(|guild| guild.name.clone())
                                .unwrap_or_else(|| "server".to_string());
                            self.status_line = Some(format!("opened {name}"));
                        }
                        None => {}
                    },
                    crate::nav::NavArea::Channels => {
                        if let Some(id) = channels.get(self.navigator.channel).cloned() {
                            self.select_channel(id.clone());
                            let name = self
                                .channels
                                .iter()
                                .find(|channel| channel.id == id)
                                .map(|channel| channel.name.clone())
                                .unwrap_or_default();
                            self.status_line = Some(format!("opened #{name}"));
                        }
                    }
                },
                _ => {}
            }
        }
    }

    /// Recompute results from the messages currently held.
    pub(crate) fn refresh_search(&mut self) {
        let query = crate::search::SearchQuery::parse(&self.search.query);
        self.search.results =
            crate::search::search(&self.messages, &query, crate::search::MAX_RESULTS);
        self.search.needs_refresh = false;
    }

    /// Something changed the message list while the overlay may be open.
    fn mark_search_stale(&mut self) {
        if self.search.open {
            self.search.needs_refresh = true;
        }
    }

    /// Open a channel and treat it as read from that moment on.
    pub(crate) fn select_channel(&mut self, channel_id: String) {
        self.unread.mark_read(&channel_id);
        self.selected_channel = Some(channel_id);
    }

    /// Open the channel a search result belongs to.
    pub(crate) fn jump_to_channel(&mut self, channel_id: &str) {
        if let Some(channel) = self
            .channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .cloned()
        {
            self.selected_guild = Some(channel.guild_id);
            self.unread.mark_read(&channel.id);
            self.selected_channel = Some(channel.id);
            self.status_line = Some(format!("opened #{}", channel.name));
        }
        self.close_search();
    }

    // -----------------------------------------------------------------------
    // Shortcuts
    // -----------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // The shortcuts page owns key input while a rebind is pending.
        if self.settings_ui.rebinding.is_some() {
            return;
        }

        let events: Vec<(String, bool, bool, bool)> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((
                        format!("{key:?}"),
                        modifiers.ctrl || modifiers.command,
                        modifiers.shift,
                        modifiers.alt,
                    )),
                    _ => None,
                })
                .collect()
        });

        for (key, ctrl, shift, alt) in events {
            for action in ShortcutAction::ALL {
                if self
                    .settings
                    .chord_for(action)
                    .matches(&key, ctrl, shift, alt)
                {
                    self.dispatch(action);
                    break;
                }
            }
        }
    }

    fn dispatch(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::Search => {
                if self.search.open {
                    self.close_search();
                } else {
                    self.open_search();
                }
            }
            ShortcutAction::QuickSwitcher => {
                if self.switcher.open {
                    self.close_switcher();
                } else {
                    self.open_switcher();
                }
            }
            ShortcutAction::KeyboardNavigation => {
                if self.navigator.active {
                    self.end_navigation();
                } else {
                    self.begin_navigation();
                }
            }
            ShortcutAction::ToggleSettings => self.show_settings = !self.show_settings,
            ShortcutAction::NextChannel => self.cycle_channel(1),
            ShortcutAction::PrevChannel => self.cycle_channel(-1),
            ShortcutAction::NextGuild => self.cycle_guild(1),
            ShortcutAction::PrevGuild => self.cycle_guild(-1),
            ShortcutAction::RefreshView => {
                self.plugins.on_settings(&mut self.settings);
                self.status_line = Some("theme reloaded".to_string());
            }
            ShortcutAction::ToggleCompact => {
                self.settings.compact_mode = !self.settings.compact_mode;
                self.settings_changed();
                self.status_line = Some(format!(
                    "compact mode {}",
                    if self.settings.compact_mode {
                        "on"
                    } else {
                        "off"
                    }
                ));
            }
            ShortcutAction::ExportDiagnostics => match diagnostics::export(&self.settings) {
                Ok(path) => {
                    self.settings_ui.export_status = Some(format!("written to {}", path.display()));
                    self.status_line = Some(format!("diagnostics written to {}", path.display()));
                }
                Err(err) => {
                    self.settings_ui.export_status = Some(format!("export failed: {err:#}"));
                    self.status_line = Some(format!("export failed: {err:#}"));
                }
            },
        }
    }

    fn cycle_channel(&mut self, delta: i32) {
        let guild = self.selected_guild.clone();
        let ids: Vec<String> = self
            .channels
            .iter()
            .filter(|channel| {
                Some(&channel.guild_id) == guild.as_ref() && channel.kind == ChannelKind::Text
            })
            .map(|channel| channel.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }

        let current = self
            .selected_channel
            .as_ref()
            .and_then(|id| ids.iter().position(|candidate| candidate == id))
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(ids.len() as i32) as usize;
        self.select_channel(ids[next].clone());
    }

    fn cycle_guild(&mut self, delta: i32) {
        if self.guilds.is_empty() {
            return;
        }
        let current = self
            .selected_guild
            .as_ref()
            .and_then(|id| self.guilds.iter().position(|guild| &guild.id == id))
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(self.guilds.len() as i32) as usize;
        let guild_id = self.guilds[next].id.clone();
        self.selected_guild = Some(guild_id.clone());

        if let Some(first) = self
            .channels
            .iter()
            .find(|channel| channel.guild_id == guild_id && channel.kind == ChannelKind::Text)
            .map(|channel| channel.id.clone())
        {
            self.select_channel(first);
        }
    }

    // -----------------------------------------------------------------------
    // Telemetry
    // -----------------------------------------------------------------------

    fn tick_telemetry(&mut self, ctx: &egui::Context) {
        let interval = if self.show_settings {
            Duration::from_millis(1000)
        } else {
            Duration::from_millis(5000)
        };

        // Frame accounting: a repaint loop shows up here as hundreds of frames
        // per tick, which is the cheapest way to answer "why is idle CPU
        // nonzero?" without a profiler. Run with `RUST_LOG=debug`.
        let sampled = self.monitor.maybe_sample(interval);
        if sampled {
            let delta = self.frames.saturating_sub(self.frames_at_last_tick);
            self.frames_at_last_tick = self.frames;
            log::debug!(
                "ui frames: {delta} per {interval:?} ({:.1} fps)",
                delta as f64 / interval.as_secs_f64()
            );
        }

        if sampled
            && self.settings.memory_alerts
            && self.monitor.over_budget(self.settings.memory_budget_mb)
            && self.alert.is_none()
        {
            self.monitor.alerts_fired += 1;
            let rss = self.monitor.rss_bytes().unwrap_or(0);
            let text = format!(
                "Memory budget exceeded: {} resident against a {} MB budget",
                sysinfo::human_bytes(rss),
                self.settings.memory_budget_mb
            );
            self.push_alert(text, AlertKind::Warn, 10.0);
        }

        // Stamp and expire banners against the egui clock.
        let now = ctx.input(|input| input.time);
        if let Some(alert) = &mut self.alert {
            if alert.until == f64::MAX {
                alert.until = now + alert.duration;
            }
            if now > alert.until {
                self.alert = None;
            }
        }

        // Keep repaints cheap but bounded: this is the only timer in the app.
        ctx.request_repaint_after(interval);
    }

    // -----------------------------------------------------------------------
    // Discord plumbing
    // -----------------------------------------------------------------------

    /// Find a message by id, and by channel when the event names one.
    fn find_message_mut(&mut self, id: &str, channel_id: Option<&str>) -> Option<&mut Message> {
        self.messages
            .iter_mut()
            .find(|message| message.matches_identity(id, channel_id))
    }

    fn poll_gateway(&mut self, now: f64) {
        let mut events = Vec::new();
        if let Some(gateway) = &mut self.gateway {
            while let Ok(event) = gateway.events.try_recv() {
                events.push(event);
            }
        }

        let mut received_message = false;

        for event in events {
            match event {
                GatewayEvent::Connected => {
                    self.gateway_status = "Gateway connected; identifying...".to_string();
                }
                GatewayEvent::Ready {
                    username,
                    user_id,
                    guild_count,
                } => {
                    self.gateway_status = format!("Ready as {username} · {guild_count} guild(s)");
                    self.current_user.username = username;
                    self.current_user.status = Presence::Online;
                    self.bot_user_id = Some(user_id);
                }
                GatewayEvent::GuildCreate { guild, channels } => {
                    if !self.live_data {
                        self.guilds.clear();
                        self.channels.clear();
                        self.messages.clear();
                        self.typing.clear();
                        self.voice.clear();
                        self.selected_guild = None;
                        self.selected_channel = None;
                        self.live_data = true;
                    }

                    let first_text = channels
                        .iter()
                        .find(|channel| channel.kind == ChannelKind::Text)
                        .map(|channel| channel.id.clone());

                    if self.selected_guild.is_none() {
                        self.selected_guild = Some(guild.id.clone());
                    }
                    if self.selected_channel.is_none() {
                        if let Some(channel) = first_text {
                            self.select_channel(channel);
                        }
                    }

                    // A guild can arrive twice (READY's list plus a later event,
                    // or after a resume), so update in place rather than
                    // growing duplicate icons and channels.
                    if let Some(existing) = self
                        .guilds
                        .iter_mut()
                        .find(|existing| existing.id == guild.id)
                    {
                        existing.name = guild.name;
                        existing.icon = guild.icon;
                    } else {
                        self.guilds.push(guild);
                    }

                    for channel in channels {
                        if !self
                            .channels
                            .iter()
                            .any(|existing| existing.id == channel.id)
                        {
                            self.channels.push(channel);
                        }
                    }
                }
                GatewayEvent::Message(message) => {
                    let channel = message.channel_id.clone().unwrap_or_default();
                    let is_open = self.selected_channel.as_deref() == Some(channel.as_str());
                    let is_own = self.bot_user_id.as_deref() == Some(message.author.id.as_str())
                        || message.author.username == self.current_user.username;
                    let is_mention = crate::models::mentions_user(
                        &message.content,
                        self.bot_user_id.as_deref().unwrap_or(""),
                    );

                    self.unread.observe(&channel, is_mention, is_open, is_own);

                    self.messages.push(*message);
                    received_message = true;
                    self.mark_search_stale();
                }
                GatewayEvent::MemberIntentMissing => {
                    self.member_intent_missing = true;
                    self.status_line = Some(
                        "member roster needs the Server Members Intent in the Developer Portal"
                            .to_string(),
                    );
                }
                GatewayEvent::VoiceState {
                    channel_id,
                    user_id,
                    username,
                } => {
                    self.voice
                        .update(channel_id.as_deref(), &user_id, &username);
                }
                GatewayEvent::MessageUpdate {
                    id,
                    channel_id,
                    content,
                    embeds,
                } => {
                    let changed = crate::models::apply_update(
                        &mut self.messages,
                        &id,
                        channel_id.as_deref(),
                        content.as_deref(),
                        embeds,
                    );
                    if changed {
                        // An edit can change the row's measured height.
                        self.metrics.clear();
                        self.status_line = Some("message edited".to_string());
                        self.mark_search_stale();
                    }
                }
                GatewayEvent::MessageDelete { id, channel_id } => {
                    if crate::models::remove_message(&mut self.messages, &id, channel_id.as_deref())
                    {
                        // Rows below the gap shift up, so heights are stale.
                        self.metrics.clear();
                        self.status_line = Some("message deleted".to_string());
                        self.mark_search_stale();
                    }
                }
                GatewayEvent::Reaction {
                    message_id,
                    emoji,
                    user_id,
                    added,
                    ..
                } => {
                    let by_me = matches!((&self.bot_user_id, &user_id), (Some(bot), Some(user)) if bot == user);
                    let changed = self
                        .find_message_mut(&message_id, None)
                        .map(|message| {
                            if added {
                                message.add_reaction(&emoji, user_id.as_deref(), by_me)
                            } else {
                                message.remove_reaction(&emoji, user_id.as_deref(), by_me)
                            }
                        })
                        .unwrap_or(false);
                    if changed {
                        // A chip row can appear or vanish.
                        self.metrics.clear();
                    }
                }
                GatewayEvent::ReactionsRemoved {
                    message_id, emoji, ..
                } => {
                    let changed = self
                        .find_message_mut(&message_id, None)
                        .map(|message| match emoji.as_deref() {
                            Some(emoji) => message.remove_reaction_emoji(emoji),
                            None => message.clear_reactions(),
                        })
                        .unwrap_or(false);
                    if changed {
                        self.metrics.clear();
                    }
                }
                GatewayEvent::Typing {
                    channel_id,
                    user_id,
                    username,
                } => {
                    // The bot's own typing is not worth showing.
                    if self.bot_user_id.as_deref() != Some(user_id.as_str()) {
                        self.typing.record(&channel_id, &user_id, &username, now);
                    }
                }
                GatewayEvent::Reconnecting {
                    attempt,
                    delay_secs,
                    reason,
                } => {
                    self.gateway_status =
                        format!("Reconnecting in {delay_secs}s (attempt {attempt}) — {reason}");
                }
                GatewayEvent::Resumed => {
                    self.gateway_status = "Gateway session resumed".to_string();
                }
                GatewayEvent::Error(err) => {
                    self.gateway_status = format!("Gateway error: {err}");
                    self.gateway = None;
                }
            }
        }

        if received_message {
            self.trim_history();
        }
    }

    /// Submit the sign-in page.
    ///
    /// Validates the token shape, stores it in the keyring and starts the
    /// gateway. A keyring failure is not fatal: the session continues with the
    /// token in memory, which is the behaviour documented for a missing Secret
    /// Service provider.
    pub(crate) fn sign_in(&mut self) {
        let raw = self.login.token.trim().to_string();
        let token = raw.strip_prefix("Bot ").unwrap_or(&raw).trim().to_string();

        if token.is_empty() {
            self.login.error = Some("Paste a bot token to sign in.".to_string());
            return;
        }

        if !crate::ui::login::looks_like_bot_token(&token) {
            self.login.error = Some(
                "That does not look like a bot token. Expected \
                 <id>.<timestamp>.<signature> — copy the whole string from \
                 Developer Portal → Bot → Reset Token."
                    .to_string(),
            );
            return;
        }

        self.token_status = match auth::store_discord_token(&token) {
            Ok(()) => "Token saved to keyring".to_string(),
            Err(err) => {
                format!("Keyring unavailable ({err:#}); the token will only last this session")
            }
        };

        self.token_draft = token;
        self.login.token.clear();
        self.login.error = None;
        self.login.visible = false;
        self.status_line = Some("signing in…".to_string());
        self.restart_gateway();
    }

    /// Skip sign-in and stay on the local demo data.
    pub(crate) fn dismiss_login(&mut self) {
        self.login.visible = false;
        self.login.token.clear();
        self.login.error = None;
        self.status_line = Some("continuing with demo data".to_string());
    }

    /// Begin the OAuth2 identity flow: state + PKCE, loopback listener, browser.
    pub(crate) fn start_identity_sign_in(&mut self) {
        self.oauth.error = None;
        self.oauth.status = None;

        let Some(client_id) = self
            .settings
            .oauth_client_id
            .clone()
            .filter(|id| !id.trim().is_empty())
        else {
            self.oauth.error = Some(
                "Add your Discord application's client ID first; it is on the app's \
                 OAuth2 page in the Developer Portal."
                    .to_string(),
            );
            return;
        };

        if let Some(pending) = self.oauth.pending.take() {
            pending.callback.cancel();
        }

        let (state, verifier) = match (
            crate::api::oauth::new_state(),
            crate::api::oauth::new_verifier(),
        ) {
            (Ok(state), Ok(verifier)) => (state, verifier),
            _ => {
                self.oauth.error = Some("no system randomness available".to_string());
                return;
            }
        };

        let callback = match crate::api::oauth::listen_for_callback(state.clone()) {
            Ok(callback) => callback,
            Err(err) => {
                self.oauth.error = Some(format!("{err:#}"));
                return;
            }
        };

        let challenge = crate::api::oauth::pkce_challenge(&verifier);
        let url = crate::api::oauth::authorize_url(&client_id, &state, &challenge);
        if let Err(err) = crate::api::oauth::open_browser(&url) {
            callback.cancel();
            self.oauth.error = Some(format!("{err:#} — open this URL yourself: {url}"));
            return;
        }

        self.oauth.status = Some("waiting for your browser…".to_string());
        self.oauth.pending = Some(PendingOAuth {
            callback,
            verifier,
            client_id,
            session_rx: None,
            started: std::time::Instant::now(),
        });
    }

    /// Ask for a new access token with the stored refresh token, off the UI
    /// thread. A failed refresh flags the session so the UI offers a sign-in.
    pub(crate) fn start_identity_refresh(&mut self) {
        if self.identity_refresh.is_some() {
            return;
        }
        let (Some(session), Some(client_id)) =
            (self.identity.clone(), self.settings.oauth_client_id.clone())
        else {
            return;
        };
        if crate::api::oauth::refresh_token_for(&session).is_err() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("rustcord-oauth-refresh".to_string())
            .spawn(move || {
                let result = (|| -> Result<_, String> {
                    let refreshed = crate::api::oauth::refresh_session(&client_id, &session)
                        .map_err(|err| format!("{err:#}"))?;
                    crate::api::oauth::KeyringStore
                        .save(&refreshed)
                        .map_err(|err| format!("could not store the refreshed session: {err:#}"))?;
                    Ok(refreshed)
                })();
                let _ = tx.send(result);
            });

        self.identity_refresh = Some(IdentityRefresh { rx });
    }

    fn poll_identity_refresh(&mut self) {
        let Some(refresh) = self.identity_refresh.take() else {
            return;
        };

        match refresh.rx.try_recv() {
            Ok(Ok(session)) => {
                self.identity = Some(session);
                self.identity_expired = false;
                self.oauth.status = Some("identity session refreshed".to_string());
                self.oauth.error = None;
            }
            Ok(Err(err)) => {
                self.identity_expired = true;
                self.oauth.error = Some(format!("could not refresh the identity: {err}"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.identity_refresh = Some(refresh);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.oauth.error = Some("the refresh worker stopped".to_string());
            }
        }
    }

    pub(crate) fn cancel_identity_sign_in(&mut self) {
        if let Some(pending) = self.oauth.pending.take() {
            pending.callback.cancel();
        }
        self.oauth.status = None;
        self.oauth.error = None;
    }

    /// Advance an identity sign-in: the browser callback, then the exchange.
    pub(crate) fn poll_oauth(&mut self) {
        let Some(mut pending) = self.oauth.pending.take() else {
            return;
        };
        let mut finished = false;

        if let Some(rx) = &pending.session_rx {
            match rx.try_recv() {
                Ok(Ok(session)) => {
                    self.identity = Some(session);
                    self.identity_expired = false;
                    self.oauth.status = Some("signed in with Discord".to_string());
                    finished = true;
                }
                Ok(Err(err)) => {
                    self.oauth.error = Some(err);
                    finished = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.oauth.error = Some("the sign-in worker stopped".to_string());
                    finished = true;
                }
            }
        } else {
            match pending.callback.receiver.try_recv() {
                Ok(Ok(crate::api::oauth::Callback::Code(code))) => {
                    let (tx, rx) = mpsc::channel();
                    let client_id = pending.client_id.clone();
                    let verifier = pending.verifier.clone();

                    // Blocking HTTP stays off the UI thread, like images.
                    let _ = std::thread::Builder::new()
                        .name("rustcord-oauth-token".to_string())
                        .spawn(move || {
                            let result = (|| -> Result<_, String> {
                                let tokens =
                                    crate::api::oauth::exchange_code(&client_id, &code, &verifier)
                                        .map_err(|err| format!("{err:#}"))?;
                                let identity =
                                    crate::api::oauth::fetch_identity(&tokens.access_token)
                                        .map_err(|err| format!("{err:#}"))?;
                                let session = crate::api::oauth::Session { tokens, identity };
                                crate::api::oauth::KeyringStore
                                    .save(&session)
                                    .map_err(|err| {
                                        format!("could not store the session: {err:#}")
                                    })?;
                                Ok(session)
                            })();
                            let _ = tx.send(result);
                        });

                    pending.session_rx = Some(rx);
                    self.oauth.status = Some("exchanging the code with Discord…".to_string());
                }
                Ok(Ok(crate::api::oauth::Callback::Denied(reason))) => {
                    self.oauth.error = Some(format!("sign-in was cancelled ({reason})"));
                    finished = true;
                }
                Ok(Err(err)) => {
                    self.oauth.error = Some(format!("{err:#}"));
                    finished = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.oauth.error = Some("the sign-in listener stopped".to_string());
                    finished = true;
                }
            }
        }

        // The listener has its own timeout; this one covers a stalled exchange.
        if !finished && pending.started.elapsed() > Duration::from_secs(240) {
            pending.callback.cancel();
            self.oauth.error = Some("sign-in timed out".to_string());
            finished = true;
        }

        if !finished {
            self.oauth.pending = Some(pending);
        }
    }

    /// Forget the identity session. The bot token is untouched.
    pub(crate) fn sign_out_identity(&mut self) {
        if let Some(pending) = self.oauth.pending.take() {
            pending.callback.cancel();
        }
        crate::api::oauth::KeyringStore.clear();
        self.identity = None;
        self.identity_expired = false;
        self.oauth.status = Some("signed out of Discord identity".to_string());
        self.oauth.error = None;
    }

    /// Drop the session and keyring entry, then return to the sign-in page.
    pub(crate) fn sign_out(&mut self) {
        if let Some(handle) = self.gateway.take() {
            handle.shutdown();
        }
        self.runtime = None;
        self.live_data = false;
        self.show_settings = false;
        self.token_draft.clear();
        self.history.clear();
        self.metrics.clear();
        self.metrics_channel = None;
        self.images.clear();
        self.typing.clear();
        self.voice.clear();
        self.unread.clear();
        self.members.clear();
        self.members_loading.clear();
        self.member_intent_missing = false;
        self.cancel_edit();
        self.replying_to = None;
        self.pending_delete = None;
        self.bot_user_id = None;

        let _ = auth::clear_discord_token();
        // Signing out of the bot ends the session, so the identity goes too.
        self.sign_out_identity();
        self.token_status = "Signed out; token removed from the keyring".to_string();
        self.gateway_status = "No bot token stored".to_string();
        self.login = crate::ui::login::LoginState::new(true);
    }

    pub(crate) fn restart_gateway(&mut self) {
        if let Some(handle) = self.gateway.take() {
            handle.shutdown();
        }
        self.runtime = None;
        self.live_data = false;

        if self.token_draft.trim().is_empty() {
            self.gateway_status = "No bot token stored".to_string();
            return;
        }

        match start_gateway(&self.token_draft) {
            Ok((runtime, gateway)) => {
                self.runtime = Some(runtime);
                self.gateway = Some(gateway);
                self.gateway_status = "Connecting to Discord gateway...".to_string();
            }
            Err(err) => {
                self.gateway_status = format!("Could not start gateway: {err:#}");
            }
        }
    }

    pub(crate) fn selected_channel_name(&self) -> Option<&str> {
        self.selected_channel
            .as_ref()
            .and_then(|id| self.channels.iter().find(|channel| &channel.id == id))
            .map(|channel| channel.name.as_str())
    }

    pub(crate) fn send_current_draft(&mut self) {
        if let Some(message_id) = self.editing.clone() {
            self.submit_edit(message_id);
            return;
        }

        let content = self.draft.trim().to_string();
        if content.is_empty() {
            return;
        }

        let selected_is_text = self
            .selected_channel
            .as_ref()
            .and_then(|id| self.channels.iter().find(|channel| &channel.id == id))
            .map(|channel| channel.kind == ChannelKind::Text)
            .unwrap_or(false);

        if self.live_data && !selected_is_text {
            return;
        }

        if self.live_data {
            if let (Some(runtime), Some(channel_id)) =
                (&self.runtime, self.selected_channel.clone())
            {
                if self.gateway.is_some() {
                    // A reply only makes sense in the channel it came from.
                    let reply_id = self
                        .replying_to
                        .take()
                        .filter(|reply| reply.channel_id.as_deref() == Some(channel_id.as_str()))
                        .map(|reply| reply.message_id);
                    let token = self.token_draft.clone();
                    let tx = self.send_tx.clone();
                    runtime.spawn(async move {
                        let result = match DiscordClient::new(token) {
                            Ok(client) => match reply_id {
                                Some(target) => {
                                    client.send_reply(&channel_id, &content, &target).await
                                }
                                None => client.send_message(&channel_id, &content).await,
                            }
                            .map_err(|err| format!("{err:#}")),
                            Err(err) => Err(format!("{err:#}")),
                        };

                        match result {
                            Ok(message) => {
                                let _ = tx.send(SendResult::Sent(Box::new(message)));
                            }
                            Err(err) => {
                                let _ = tx.send(SendResult::Failed(err));
                            }
                        }
                    });
                    self.draft.clear();
                    return;
                }
            }
        }

        let mut message = Message::new(self.current_user.clone(), content);
        message.channel_id = self.selected_channel.clone();
        message.reply_to = self.replying_to.take();
        self.plugins.on_message(&mut message);
        self.messages.push(message);
        self.draft.clear();
        self.trim_history();
    }

    fn poll_send_results(&mut self) {
        let mut sent = false;
        while let Ok(result) = self.send_results.try_recv() {
            match result {
                SendResult::Sent(message) => {
                    self.messages.push(*message);
                    self.status_line = Some("message sent".to_string());
                    sent = true;
                }
                SendResult::Failed(err) => {
                    self.status_line = Some(format!("send failed: {err}"));
                }
            }
        }
        if sent {
            self.trim_history();
        }
    }
}

impl eframe::App for RustcordApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames = self.frames.wrapping_add(1);

        // Both are change-gated: applying the palette unconditionally would
        // request a repaint every frame (see `apply_theme`).
        self.apply_ui_scale(&ctx);
        self.apply_theme(&ctx);

        let now = ctx.input(|input| input.time) as f32;
        self.plugins.spotify.tick(now);
        self.poll_gateway(f64::from(now));
        self.poll_send_results();
        self.poll_action_results();
        self.poll_history();
        self.poll_members();
        self.ensure_history();
        self.ensure_members();
        self.sync_image_budget();
        if self.images.poll(&ctx) > 0 {
            // Freshly decoded textures only show up on the next frame.
            ctx.request_repaint();
        }
        self.handle_shortcuts(&ctx);
        self.handle_navigation(&ctx);
        self.tick_telemetry(&ctx);

        if self.settings.compact_mode {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
        }

        if self.search.open && self.search.needs_refresh {
            self.refresh_search();
        }

        // An expired session with a refresh token recovers on its own; without
        // one it stays flagged and the UI offers a sign-in.
        if self.identity_expired && self.identity_refresh.is_none() {
            self.start_identity_refresh();
        }
        self.poll_identity_refresh();
        if self.identity_refresh.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        self.poll_oauth();
        if self.oauth.pending.is_some() {
            // A browser round trip is in flight; wake often enough to notice it.
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        if self.login.visible {
            // Sign-in replaces the whole workspace: with no session there is no
            // guild, channel or message worth drawing behind it.
            ui::login::show(self, ui);
            return;
        }

        ui::sidebar::show(self, ui);
        ui::chat::show(self, ui);
        ui::settings::show(self, &ctx);
        ui::search::show(self, &ctx);
        ui::switcher::show(self, &ctx);
    }
}

/// FNV-1a over the palette's colours and name. No allocation, so it is cheap
/// enough to compute every frame.
fn theme_signature(theme: &crate::ui::theme::AppTheme) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u8| {
        hash ^= u64::from(value);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };

    for color in [
        theme.background,
        theme.surface,
        theme.surface_alt,
        theme.accent,
        theme.text,
        theme.muted,
        theme.danger,
        theme.success,
    ] {
        feed(color.r());
        feed(color.g());
        feed(color.b());
        feed(color.a());
    }
    feed(u8::from(theme.is_dark));
    for byte in theme.name.as_bytes() {
        feed(*byte);
    }

    hash
}

fn demo_messages(current_user: &User) -> Vec<Message> {
    let alice = User {
        id: "2".to_string(),
        username: "alice".to_string(),
        avatar: None,
        status: Presence::Online,
    };
    let bob = User {
        id: "3".to_string(),
        username: "bob".to_string(),
        avatar: None,
        status: Presence::Idle,
    };
    let carol = User {
        id: "4".to_string(),
        username: "carol".to_string(),
        avatar: None,
        status: Presence::Dnd,
    };

    let mut messages = vec![
        message_at(
            &alice,
            "welcome to rustcord — the egui-shaped hole in your heart",
            90,
        ),
        message_at(&bob, "does it actually stay under 150 MB?", 75),
        message_at(
            &alice,
            "idle yes; once you load a theme with 10k selectors, all bets are off",
            74,
        ),
        message_at(
            &carol,
            "the Vencord CSS parser is a subset, but accent/background/text vars work",
            60,
        ),
        message_at(
            current_user,
            "streaming indicator is wired to the plugin hook too",
            30,
        ),
        message_at(&bob, "spotify status + click-to-open is a nice touch", 12),
    ];

    messages[0].reactions.push(Reaction {
        emoji: "🚀".to_string(),
        count: 7,
        me: true,
        users: Vec::new(),
    });
    messages[3].reactions.push(Reaction {
        emoji: "🎨".to_string(),
        count: 3,
        me: false,
        users: Vec::new(),
    });

    messages
}

fn message_at(author: &User, content: &str, minutes_ago: i64) -> Message {
    let mut message = Message::new(author.clone(), content);
    message.channel_id = Some("10".to_string());
    message.timestamp = Utc::now() - ChronoDuration::minutes(minutes_ago);
    message
}

fn start_gateway(token: &str) -> anyhow::Result<(tokio::runtime::Runtime, GatewayHandle)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        // One worker is enough for a heartbeat + socket read loop. Keeping the
        // pool small is part of the low-resource goal.
        .worker_threads(1)
        .thread_name("rustcord-gateway")
        .enable_all()
        .build()?;
    let gateway = GatewayClient::new(token, DEFAULT_INTENTS).start(&runtime);
    Ok((runtime, gateway))
}
