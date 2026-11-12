use crate::api::discord::DiscordClient;
use crate::api::gateway::{GatewayClient, GatewayEvent, GatewayHandle, DEFAULT_INTENTS};
use crate::auth;
use crate::models::{Channel, ChannelKind, Guild, Message, Presence, Reaction, User};
use crate::plugins::PluginManager;
use crate::settings::{ResourcePriority, Settings};
use crate::ui;
use chrono::{Duration, Utc};
use egui::RichText;
use std::sync::mpsc::{self, Receiver, Sender};

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
            send_tx,
            send_results,
        }
    }

    fn poll_gateway(&mut self) {
        let mut events = Vec::new();
        if let Some(gateway) = &mut self.gateway {
            while let Ok(event) = gateway.events.try_recv() {
                events.push(event);
            }
        }

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
    }

    fn restart_gateway(&mut self) {
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
    }

    fn poll_send_results(&mut self) {
        while let Ok(result) = self.send_results.try_recv() {
            match result {
                SendResult::Sent(message) => {
                    self.messages.push(message);
                    self.status_line = Some("message sent".to_string());
                }
                SendResult::Failed(err) => {
                    self.status_line = Some(format!("send failed: {err}"));
                }
            }
        }
    }

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut changed = false;
        let mut connect_requested = false;

        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.heading("Discord connection");
                ui.label(
                    "Bot token. Stored in the OS keyring, never in settings.json. \
                     Email/password login is intentionally not supported.",
                );

                let mut token = self.token_draft.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut token)
                            .password(true)
                            .hint_text("Bot <token>"),
                    )
                    .changed()
                {
                    self.token_draft = token;
                    self.token_status = "Unsaved token".to_string();
                }

                ui.horizontal(|ui| {
                    if ui.button("Save token").clicked() {
                        match auth::store_discord_token(self.token_draft.trim()) {
                            Ok(()) => {
                                self.token_status = "Token saved to keyring".to_string();
                            }
                            Err(err) => {
                                self.token_status = format!("Could not save token: {err:#}");
                            }
                        }
                    }
                    if ui.button("Connect gateway").clicked() {
                        connect_requested = true;
                    }
                    if ui.button("Clear token").clicked() {
                        match auth::clear_discord_token() {
                            Ok(()) => {
                                self.token_draft.clear();
                                self.token_status = "Token cleared".to_string();
                            }
                            Err(err) => {
                                self.token_status = format!("Could not clear token: {err:#}");
                            }
                        }
                    }
                });

                ui.label(
                    RichText::new(&self.token_status)
                        .small()
                        .color(self.plugins.theme().muted),
                );
                ui.label(
                    RichText::new(&self.gateway_status)
                        .small()
                        .color(self.plugins.theme().muted),
                );

                ui.separator();
                ui.heading("Appearance");
                changed |= ui
                    .checkbox(
                        &mut self.settings.vencord_themes,
                        "Enable Vencord-compatible themes",
                    )
                    .changed();

                let mut theme_path = self
                    .settings
                    .theme_file
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();

                ui.horizontal(|ui| {
                    ui.label("Theme file:");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut theme_path)
                                .hint_text("~/.config/rustcord/theme.theme"),
                        )
                        .changed()
                    {
                        self.settings.theme_file = if theme_path.trim().is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(theme_path.trim()))
                        };
                        changed = true;
                    }
                });

                ui.separator();
                ui.heading("Integrations");
                changed |= ui
                    .checkbox(
                        &mut self.settings.spotify_status_sync,
                        "Sync Spotify now-playing to profile",
                    )
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut self.settings.streaming_notifications,
                        "Show streaming indicators",
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.settings.show_blyat_banner, "Show Blyat banner")
                    .changed();

                ui.separator();
                ui.heading("Performance");
                ui.horizontal(|ui| {
                    ui.label("Resource priority:");
                    changed |= ui
                        .selectable_value(
                            &mut self.settings.resource_priority,
                            ResourcePriority::Low,
                            "Low",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut self.settings.resource_priority,
                            ResourcePriority::Balanced,
                            "Balanced",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut self.settings.resource_priority,
                            ResourcePriority::Performance,
                            "Performance",
                        )
                        .changed();
                });

                ui.separator();
                ui.label(format!(
                    "Loaded plugins: {}",
                    self.plugins.names().join(", ")
                ));

                if ui.button("Save settings").clicked() {
                    if let Err(err) = self.settings.save() {
                        self.status_line = Some(format!("could not save settings: {err:#}"));
                    } else {
                        self.status_line = Some("settings saved".to_string());
                    }
                }
            });

        self.show_settings = open;

        if connect_requested {
            self.restart_gateway();
        }

        if changed {
            self.plugins.on_settings(&mut self.settings);
            if let Err(err) = self.settings.save() {
                log::warn!("could not persist settings: {err:#}");
            }
        }
    }
}

impl eframe::App for RustcordApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Re-apply every frame so a theme-file reload is reflected immediately.
        self.plugins.theme().apply(&ctx);

        let now = ctx.input(|input| input.time) as f32;
        self.plugins.spotify.tick(now);
        self.poll_gateway();
        self.poll_send_results();

        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Comma)) {
            self.show_settings = true;
        }

        ui::sidebar::show(self, ui);
        ui::chat::show(self, ui);

        if self.show_settings {
            self.show_settings_window(&ctx);
        }
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
    message.timestamp = Utc::now() - Duration::minutes(minutes_ago);
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
