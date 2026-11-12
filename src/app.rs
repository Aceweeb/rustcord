use crate::api::discord::DiscordClient;
use crate::api::gateway::{GatewayClient, GatewayEvent, GatewayHandle, DEFAULT_INTENTS};
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
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

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
    pub(crate) monitor: MemoryMonitor,
    pub(crate) settings_ui: SettingsUiState,
    pub(crate) alert: Option<Alert>,
    pub(crate) frames: u64,
    applied_ui_scale: f32,
    send_tx: Sender<SendResult>,
    send_results: Receiver<SendResult>,
}

enum SendResult {
    Sent(Message),
    Failed(String),
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

        let (send_tx, send_results) = mpsc::channel();

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
            monitor: MemoryMonitor::new(),
            settings_ui: SettingsUiState::default(),
            alert: None,
            frames: 0,
            applied_ui_scale: 0.0,
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
        self.selected_channel = Some(ids[next].clone());
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
        {
            self.selected_channel = Some(first.id.clone());
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

        if self.monitor.maybe_sample(interval)
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

    fn poll_gateway(&mut self) {
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
                    guild_count,
                } => {
                    self.gateway_status = format!("Ready as {username} · {guild_count} guild(s)");
                    self.current_user.username = username;
                    self.current_user.status = Presence::Online;
                }
                GatewayEvent::GuildCreate { guild, channels } => {
                    if !self.live_data {
                        self.guilds.clear();
                        self.channels.clear();
                        self.messages.clear();
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
                        self.selected_channel = first_text;
                    }

                    self.guilds.push(guild);
                    self.channels.extend(channels);
                }
                GatewayEvent::Message(message) => {
                    self.messages.push(message);
                    received_message = true;
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
                    let token = self.token_draft.clone();
                    let tx = self.send_tx.clone();
                    runtime.spawn(async move {
                        let result = match DiscordClient::new(token) {
                            Ok(client) => client
                                .send_message(&channel_id, &content)
                                .await
                                .map_err(|err| format!("{err:#}")),
                            Err(err) => Err(format!("{err:#}")),
                        };

                        match result {
                            Ok(message) => {
                                let _ = tx.send(SendResult::Sent(message));
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
                    self.messages.push(message);
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

        // Re-apply every frame so a preset switch or theme-file reload shows at
        // once; this only writes a handful of colours.
        self.apply_ui_scale(&ctx);
        self.plugins.theme().apply(&ctx);

        let now = ctx.input(|input| input.time) as f32;
        self.plugins.spotify.tick(now);
        self.poll_gateway();
        self.poll_send_results();
        self.handle_shortcuts(&ctx);
        self.tick_telemetry(&ctx);

        if self.settings.compact_mode {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
        }

        ui::sidebar::show(self, ui);
        ui::chat::show(self, ui);
        ui::settings::show(self, &ctx);
    }
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
    });
    messages[3].reactions.push(Reaction {
        emoji: "🎨".to_string(),
        count: 3,
        me: false,
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
