//! Sign-in page.
//!
//! Rustcord supports four ways to start a session:
//!
//! 1. **Bot token** — paste a bot token from the Discord Developer Portal.
//!    This is the only token Rustcord uses to act on Discord (send messages,
//!    read history, add reactions). Stored in the OS keyring.
//! 2. **Personal account token** — paste a user-account token. Discord's
//!    Terms of Service prohibit automating user accounts (self-bots); this
//!    path is provided for compatibility and identity use only, and the user
//!    accepts the ban risk (see DECISIONS.md D2/D15).
//! 3. **QR code** — display a QR code that the Discord mobile app can scan to
//!    link a session. The token obtained this way is treated as a personal
//!    account token.
//! 4. **Demo mode** — skip sign-in entirely and use local demo data.
//!
//! Submitting stores the token in the system keyring and starts the gateway.
//! The page hides as soon as the token is accepted; connection progress then
//! shows in the status bar, and a failure is visible in **Settings → Privacy**,
//! which can sign out and bring this page back.

use crate::app::RustcordApp;
use egui::RichText;

/// Which credential kind the user is entering on the login page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginKind {
    Bot,
    Personal,
    Qr,
}

impl Default for LoginKind {
    fn default() -> Self {
        Self::Bot
    }
}

impl LoginKind {
    pub const ALL: [LoginKind; 3] = [Self::Bot, Self::Personal, Self::Qr];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bot => "Bot token",
            Self::Personal => "Personal account token",
            Self::Qr => "QR code",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Bot => "Bot token",
            Self::Personal => "Paste your account token",
            Self::Qr => "Scan the QR code with Discord",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Bot => "From Developer Portal → Bot → Reset Token",
            Self::Personal => "User-account token. Self-botting risks a ban.",
            Self::Qr => "Discord mobile app → scan this code",
        }
    }
}

/// State for QR-code authentication. The QR image is regenerated when the
/// session ID changes; the app polls Discord to learn when the code was
/// scanned and a token was issued.
pub struct QrState {
    pub visible: bool,
    /// Random session identifier embedded in the QR code.
    pub session_id: String,
    /// Rendered QR bitmap, regenerated when `session_id` changes.
    pub image: Option<egui::ColorImage>,
    /// Polling status shown under the QR code.
    pub status: Option<String>,
    /// Error message, if any.
    pub error: Option<String>,
    /// Obtained personal token after a successful scan.
    pub token: Option<String>,
}

impl QrState {
    pub fn new() -> Self {
        Self {
            visible: false,
            session_id: String::new(),
            image: None,
            status: None,
            error: None,
            token: None,
        }
    }
}

/// Sign-in page state. Holds the active login kind, the entered credential,
/// QR state and any error.
pub struct LoginState {
    pub visible: bool,
    /// Bot, personal or QR — selects which credential form is shown.
    pub kind: LoginKind,
    /// Only ever held in memory, cleared once the token is stored.
    pub token: String,
    pub error: Option<String>,
    /// QR-code sign-in state.
    pub qr: QrState,
}

impl LoginState {
    pub fn new(visible: bool) -> Self {
        Self {
            visible,
            kind: LoginKind::default(),
            token: String::new(),
            error: None,
            qr: QrState::new(),
        }
    }
}

/// A bot token is `<bot id>.<timestamp>.<signature>`: three non-empty
/// dot-separated parts, ~59 characters or more.
///
/// This is a cheap shape check, not authentication — its job is to catch an
/// application id or a truncated paste before spending a request on a 401.
pub fn looks_like_bot_token(input: &str) -> bool {
    let trimmed = input.trim();
    let token = trimmed.strip_prefix("Bot ").unwrap_or(trimmed).trim();

    if token.len() < 50 {
        return false;
    }

    let mut parts = token.split('.');
    let (Some(id), Some(timestamp), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };

    !id.is_empty() && !timestamp.is_empty() && !signature.is_empty()
}

/// A personal-account token is opaque — Discord does not document its shape,
/// and it must not be parsed. This only rejects empty input so a blank paste
/// never reaches the keyring.
pub fn looks_like_personal_token(input: &str) -> bool {
    !input.trim().is_empty()
}

/// Generate a QR code image for the given data.
///
/// Uses `qrcode::QrCode` to compute the module matrix, then renders it
/// manually into an `egui::ColorImage` to avoid depending on qrcode's
/// `image` feature (which pulls a different `image` crate version).
fn render_qr_code(data: &str) -> Option<egui::ColorImage> {
    use qrcode::QrCode;
    let code = QrCode::new(data).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    let mut pixels = Vec::with_capacity(width * width);
    for y in 0..width {
        for x in 0..width {
            let on = colors[y * width + x] != qrcode::types::Color::Light;
            pixels.push(if on {
                egui::Color32::BLACK
            } else {
                egui::Color32::WHITE
            });
        }
    }
    Some(egui::ColorImage {
        size: [width, width],
        source_size: egui::Vec2::new(width as f32, width as f32),
        pixels,
    })
}

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    // Copy theme fields into locals to avoid holding a borrow of `app`
    // across the CentralPanel closure, which captures `app`.
    let theme_background = app.plugins.theme().background;
    let theme_surface = app.plugins.theme().surface;
    let theme_accent = app.plugins.theme().accent;
    let theme_text = app.plugins.theme().text;
    let theme_muted = app.plugins.theme().muted;
    let theme_danger = app.plugins.theme().danger;
    let mut submit = false;
    let mut demo = false;
    let mut qr_toggle = false;
    let mut qr_refresh = false;

    // Copy login state into a local to avoid holding a borrow of `app`
    // across the CentralPanel closure.
    let mut login = LoginState {
        visible: app.login.visible,
        kind: app.login.kind,
        token: app.login.token.clone(),
        error: app.login.error.clone(),
        qr: QrState {
            visible: app.login.qr.visible,
            session_id: app.login.qr.session_id.clone(),
            image: app.login.qr.image.clone(),
            status: app.login.qr.status.clone(),
            error: app.login.qr.error.clone(),
            token: app.login.qr.token.clone(),
        },
    };

    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space((ui.available_height() * 0.14).max(16.0));

        ui.vertical_centered(|ui| {
            ui.set_max_width(420.0);

            let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 23.0, theme_accent);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "R",
                egui::FontId::proportional(24.0),
                theme_background,
            );

            ui.add_space(8.0);
            ui.label(
                RichText::new("Rustcord")
                    .size(24.0)
                    .strong()
                    .color(theme_text),
            );
            ui.label(
                RichText::new("Native Discord client — sign in")
                    .small()
                    .color(theme_muted),
            );
            ui.add_space(16.0);

            // ── Login-kind selector ──────────────────────────────────────
            egui::Frame::group(ui.style())
                .fill(theme_surface)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Sign in with").strong().color(theme_text));
                    ui.add_space(6.0);

                    egui::Grid::new("login_kind_grid")
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            for kind in LoginKind::ALL {
                                let selected = app.login.kind == kind;
                                let radio = egui::RadioButton::new(selected, kind.label());
                                let response = ui.add(radio);
                                if response.clicked() {
                                    app.login.kind = kind;
                                    app.login.error = None;
                                }
                                ui.label(
                                    RichText::new(kind.description())
                                        .small()
                                        .color(theme_muted),
                                );
                                ui.end_row();
                            }
                        });

                    ui.add_space(10.0);

                    match app.login.kind {
                        LoginKind::Bot => {
                            ui.label(
                                RichText::new("Paste a bot token from the Discord Developer Portal.")
                                    .small()
                                    .color(theme_muted),
                            );
                            ui.add_space(8.0);

                            let response = ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::TextEdit::singleline(&mut app.login.token)
                                    .password(true)
                                    .hint_text("Bot token"),
                            );

                            if response.changed() {
                                app.login.error = None;
                            }
                            if response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                submit = true;
                            }

                            ui.add_space(8.0);

                            let can_submit = !app.login.token.trim().is_empty();
                            if ui
                                .add_enabled(
                                    can_submit,
                                    egui::Button::new(RichText::new("Sign in").strong())
                                        .min_size(egui::vec2(ui.available_width(), 34.0)),
                                )
                                .clicked()
                            {
                                submit = true;
                            }

                            if let Some(error) = &app.login.error {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(error)
                                        .small()
                                        .color(theme_danger),
                                );
                            }

                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "The token is stored in the system keyring — never in settings.json, \\
                                     logs or diagnostics.",
                                )
                                .small()
                                .color(theme_muted),
                            );
                        }
                        LoginKind::Personal => {
                            ui.label(
                                RichText::new("Paste your Discord account token.")
                                    .small()
                                    .color(theme_muted),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("⚠️ Automating a user account is against Discord's Terms of Service. \\
                                               You accept the ban risk.")
                                    .small()
                                    .color(theme_danger),
                            );
                            ui.add_space(8.0);

                            let response = ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::TextEdit::singleline(&mut app.login.token)
                                    .password(true)
                                    .hint_text("Account token"),
                            );

                            if response.changed() {
                                app.login.error = None;
                            }
                            if response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                submit = true;
                            }

                            ui.add_space(8.0);

                            let can_submit =
                                crate::ui::login::looks_like_personal_token(&app.login.token);
                            if ui
                                .add_enabled(
                                    can_submit,
                                    egui::Button::new(RichText::new("Sign in").strong())
                                        .min_size(egui::vec2(ui.available_width(), 34.0)),
                                )
                                .clicked()
                            {
                                submit = true;
                            }

                            if let Some(error) = &app.login.error {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(error)
                                        .small()
                                        .color(theme_danger),
                                );
                            }

                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "Stored in the OS keyring only. Never in settings.json or logs. \\
                                     You can revoke the token at any time from Discord.",
                                )
                                .small()
                                .color(theme_muted),
                            );
                        }
                        LoginKind::Qr => {
                            show_qr_panel(&mut login, ui, app.plugins.theme(), &mut qr_toggle, &mut qr_refresh);
                        }
                    }
                });

            ui.add_space(12.0);

            // ── Demo mode link ───────────────────────────────────────────
            if ui
                .link(
                    RichText::new("Continue in demo mode")
                        .small()
                        .color(theme_muted),
                )
                .clicked()
            {
                demo = true;
            }

            ui.add_space(10.0);

            // ── OAuth2 identity sign-in (separate from messaging) ────────
            egui::Frame::group(ui.style())
                .fill(theme_surface)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("Also: Discord identity").strong().color(theme_text),
                    );
                    ui.label(
                        RichText::new("OAuth2 `identify` only — shows your name and avatar, never sends messages.")
                            .small()
                            .color(theme_muted),
                    );
                    ui.add_space(8.0);

                    if app.settings.oauth_client_id.is_some() {
                        if ui
                            .add(
                                egui::Button::new("Sign in with Discord")
                                    .min_size(egui::vec2(ui.available_width(), 34.0)),
                            )
                            .clicked()
                        {
                            app.start_identity_sign_in();
                        }
                    } else {
                        ui.label(
                            RichText::new("Client ID from your Discord application (OAuth2 page). It is public, not a secret.")
                                .small()
                                .color(theme_muted),
                        );
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut app.oauth.client_id_draft)
                                    .hint_text("Application client ID"),
                            )
                            .changed()
                        {
                            let trimmed = app.oauth.client_id_draft.trim().to_string();
                            app.settings.oauth_client_id = if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed)
                            };
                            app.settings_changed();
                        }
                        ui.label(
                            RichText::new(format!(
                                "Add {} as a redirect URI on that same page.",
                                crate::api::oauth::redirect_uri()
                            ))
                            .small()
                            .color(theme_muted),
                        );
                    }

                    if app.oauth.pending.is_some() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            if ui.button("Cancel").clicked() {
                                app.cancel_identity_sign_in();
                            }
                        });
                    } else if app.settings.oauth_client_id.is_some() {
                        // Button already shown above.
                    }

                    if let Some(status) = &app.oauth.status {
                        ui.label(RichText::new(status).small().color(theme_muted));
                    }
                    if let Some(error) = &app.oauth.error {
                        ui.label(RichText::new(error).small().color(theme_danger));
                    }
                });

            ui.add_space(12.0);

            // ── Bot invite link ─────────────────────────────────────────
            if app.login.kind == LoginKind::Bot {
                egui::Frame::group(ui.style())
                    .fill(theme_surface)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Add the bot to a server")
                                .strong()
                                .color(theme_text),
                        );
                        ui.label(
                            RichText::new(
                                "Invites the bot that the token above belongs to, asking only for what \\
                                 Rustcord uses: view channels, read history, send messages, embed links, \\
                                 attach files and add reactions.",
                            )
                            .small()
                            .color(theme_muted),
                        );
                        ui.add_space(8.0);

                        if let Some(client_id) = &app.settings.oauth_client_id {
                            let invite_url = crate::api::oauth::bot_invite_url(client_id);
                            if ui.button("Copy invite URL").clicked() {
                                ui.ctx().open_url(egui::OpenUrl::same_tab(&invite_url));
                            }
                            ui.label(
                                RichText::new(&invite_url)
                                    .small()
                                    .color(theme_muted)
                                    .monospace(),
                            );
                        } else {
                            ui.label(
                                RichText::new(
                                    "Set a client ID above to generate the bot invite link.",
                                )
                                .small()
                                .color(theme_muted),
                            );
                        }
                    });
            }

            ui.add_space(12.0);

            // ── How to get a token ─────────────────────────────────────
            egui::CollapsingHeader::new(
                RichText::new("Where do I get a token?")
                    .small()
                    .color(theme_muted),
            )
            .show(ui, |ui| {
                match app.login.kind {
                    LoginKind::Bot => {
                        for step in [
                            "1. Open discord.com/developers/applications and pick your app.",
                            "2. Bot → Reset Token, then copy the whole string.",
                            "3. On the same Bot tab, enable Message Content Intent.",
                            "4. Invite it with the bot scope and View Channels + Read Message History.",
                        ] {
                            ui.label(RichText::new(step).small().color(theme_muted));
                        }
                    }
                    LoginKind::Personal => {
                        ui.label(
                            RichText::new("1. Open Discord in your browser and open DevTools (F12).")
                                .small()
                                .color(theme_muted),
                        );
                        ui.label(
                            RichText::new("2. Go to the Network tab, refresh, and filter for `?locationToken`.")
                                .small()
                                .color(theme_muted),
                        );
                        ui.label(
                            RichText::new("3. The `authorization` header value is your token.")
                                .small()
                                .color(theme_muted),
                        );
                        ui.label(
                            RichText::new("⚠️ Never share this token — anyone with it can access your account.")
                                .small()
                                .color(theme_danger),
                        );
                    }
                    LoginKind::Qr => {
                        ui.label(
                            RichText::new("1. Open Discord on your phone.")
                                .small()
                                .color(theme_muted),
                        );
                        ui.label(
                            RichText::new("2. Tap your avatar → Account → Scan QR Code.")
                                .small()
                                .color(theme_muted),
                        );
                        ui.label(
                            RichText::new("3. Scan the code above and confirm the login on your phone.")
                                .small()
                                .color(theme_muted),
                        );
                    }
                }
            });

            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Account passwords are not supported: Discord does not allow third-party \\
                     user-account clients. Bot tokens and QR-linked identity sessions are the supported paths.",
                )
                .small()
                .color(theme_muted),
            );
        });
    });

    if submit {
        app.sign_in_with_kind(app.login.kind);
    }
    if demo {
        app.dismiss_login();
    }
    if qr_toggle {
        app.toggle_qr();
    }
    if qr_refresh {
        app.refresh_qr();
    }

    // ── Logout button (when a session is active) ─────────────────
    if !app.login.token.is_empty() || app.identity.is_some() {
        egui::Frame::group(ui.style())
            .fill(theme_surface)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(8.0);
                ui.label(RichText::new("Signed in as").small().color(theme_muted));
                ui.add_space(4.0);
                if let Some(identity) = &app.identity {
                    ui.horizontal(|ui| {
                        if identity.identity.avatar.is_some() {
                            ui.add_space(20.0);
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} (token: {})",
                                identity.identity.username,
                                if app.token_is_bot { "bot" } else { "personal" },
                            ))
                            .small()
                            .color(theme_text),
                        );
                    });
                } else if !app.token_draft.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new(if app.token_is_bot {
                                "Bot token (signed in)"
                            } else {
                                "Personal-account token (signed in)"
                            })
                            .small()
                            .color(theme_text),
                        );
                    });
                }
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Sign out").color(theme_background).size(15.0),
                        )
                        .fill(theme_danger.gamma_multiply(0.15))
                        .min_size(egui::vec2(ui.available_width(), 34.0)),
                    )
                    .clicked()
                {
                    app.sign_out();
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Signed-out tokens are removed from the keyring.")
                        .small()
                        .color(theme_muted),
                );
            });
    }
}

/// Action returned by the QR panel.
enum QrPanelAction {
    None,
    UseToken(String),
}

/// Render the QR-code sign-in panel: code, refresh button, status.
fn show_qr_panel(
    login: &mut LoginState,
    ui: &mut egui::Ui,
    theme: &crate::ui::theme::AppTheme,
    qr_toggle: &mut bool,
    qr_refresh: &mut bool,
) -> QrPanelAction {
    // Lazy-generate a session ID on first display.
    if login.qr.session_id.is_empty() {
        login.qr.session_id = format!(
            "rustcord-qr-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        login.qr.image = render_qr_code(&login.qr.session_id);
    }

    ui.label(
        RichText::new("Scan with the Discord mobile app")
            .strong()
            .color(theme.text),
    );
    ui.label(
        RichText::new("Account → Scan QR Code, then confirm on your phone.")
            .small()
            .color(theme.muted),
    );
    ui.add_space(8.0);

    // QR code image.
    if let Some(image) = &login.qr.image {
        let size = ui.available_width().min(280.0);
        let texture =
            ui.ctx()
                .load_texture("qr_code", image.clone(), egui::TextureOptions::default());
        let rect = ui
            .allocate_exact_size(egui::vec2(size, size), egui::Sense::hover())
            .0;
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    ui.add_space(8.0);

    ui.horizontal(|ui| {
        if ui.button("Refresh code").clicked() {
            *qr_refresh = true;
        }
        if ui.button("Use token instead").clicked() {
            *qr_toggle = true;
        }
    });

    if let Some(status) = &login.qr.status {
        ui.add_space(6.0);
        ui.label(RichText::new(status).small().color(theme.muted));
    }
    if let Some(error) = &login.qr.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(theme.danger));
    }

    // If a token was obtained from QR scanning, offer to use it.
    if let Some(token) = &login.qr.token {
        ui.add_space(10.0);
        ui.label(
            RichText::new("QR scan succeeded — token received.")
                .small()
                .color(theme.text),
        );
        if ui
            .add(
                egui::Button::new("Use this token")
                    .min_size(egui::vec2(ui.available_width(), 30.0)),
            )
            .clicked()
        {
            return QrPanelAction::UseToken(token.clone());
        }
    }

    QrPanelAction::None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like a real token: 18-digit id, timestamp, 38-char signature.
    const VALID: &str = "123456789012345678.YxAbCd.EfGhIjKlMnOpQrStUvWxYz0123456789ab";

    #[test]
    fn accepts_a_well_formed_token() {
        assert!(looks_like_bot_token(VALID));
        assert!(looks_like_bot_token(&format!("  {VALID}  ")));
    }

    #[test]
    fn accepts_a_token_pasted_with_the_bot_prefix() {
        assert!(looks_like_bot_token(&format!("Bot {VALID}")));
    }

    #[test]
    fn rejects_an_application_id() {
        // The exact failure from a real session: 26 characters, no separators.
        assert!(!looks_like_bot_token("31yxv325u5bx256umg65lfceozs4"));
        assert!(!looks_like_bot_token("1234567890123456789012345678901234"));
    }

    #[test]
    fn rejects_wrong_part_counts() {
        assert!(!looks_like_bot_token(
            "123456789012345678.YxAbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcdef"
        ));
        assert!(!looks_like_bot_token(
            "123456789012345678.YxAbCd.EfGhIjKlMnOpQrStUvWxYz0123456789ab.extra"
        ));
        assert!(!looks_like_bot_token("".trim()));
        assert!(!looks_like_bot_token("   "));
    }

    #[test]
    fn rejects_empty_parts() {
        assert!(!looks_like_bot_token(
            "123456789012345678..EfGhIjKlMnOpQrStUvWxYz0123456789ab"
        ));
    }

    #[test]
    fn accepts_any_non_empty_personal_token() {
        assert!(looks_like_personal_token("anything-non-empty"));
        assert!(!looks_like_personal_token(""));
        assert!(!looks_like_personal_token("   "));
    }

    #[test]
    fn login_kind_labels_and_descriptions() {
        assert_eq!(LoginKind::Bot.label(), "Bot token");
        assert_eq!(LoginKind::Personal.label(), "Personal account token");
        assert_eq!(LoginKind::Qr.label(), "QR code");
        assert!(!LoginKind::Bot.description().is_empty());
        assert!(!LoginKind::Personal.description().is_empty());
        assert!(!LoginKind::Qr.description().is_empty());
    }
}
