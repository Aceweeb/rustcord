//! Sign-in page.
//!
//! Rustcord signs in with a **bot token**, never an account password: automating
//! a user account is against Discord's terms (`DECISIONS.md` D2). The page is
//! therefore a credential entry screen, not an email/password form.
//!
//! Submitting stores the token in the system keyring and starts the gateway. The
//! page hides as soon as the token is accepted; connection progress then shows in
//! the status bar, and a failure is visible in **Settings → Privacy**, which can
//! sign out and bring this page back.

use crate::app::RustcordApp;
use egui::RichText;

pub struct LoginState {
    pub visible: bool,
    /// Only ever held in memory, cleared once the token is stored.
    pub token: String,
    pub error: Option<String>,
}

impl LoginState {
    pub fn new(visible: bool) -> Self {
        Self {
            visible,
            token: String::new(),
            error: None,
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

pub fn show(app: &mut RustcordApp, ui: &mut egui::Ui) {
    // Cloned: the closure below mutates `app`.
    let theme = app.plugins.theme().clone();
    let mut submit = false;
    let mut demo = false;

    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space((ui.available_height() * 0.14).max(16.0));

        ui.vertical_centered(|ui| {
            ui.set_max_width(400.0);

            let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 23.0, theme.accent);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "R",
                egui::FontId::proportional(24.0),
                theme.background,
            );

            ui.add_space(8.0);
            ui.label(
                RichText::new("Rustcord")
                    .size(24.0)
                    .strong()
                    .color(theme.text),
            );
            ui.label(
                RichText::new("Native Discord client · bot accounts only")
                    .small()
                    .color(theme.muted),
            );
            ui.add_space(16.0);

            egui::Frame::group(ui.style())
                .fill(theme.surface)
                .inner_margin(20.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.label(RichText::new("Sign in").strong().color(theme.text));
                    ui.label(
                        RichText::new("Paste a bot token from the Discord Developer Portal.")
                            .small()
                            .color(theme.muted),
                    );
                    ui.add_space(10.0);

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
                        ui.label(RichText::new(error).small().color(theme.danger));
                    }

                    ui.add_space(10.0);
                    if ui
                        .link(
                            RichText::new("Continue in demo mode")
                                .small()
                                .color(theme.muted),
                        )
                        .clicked()
                    {
                        demo = true;
                    }

                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(
                            "The token is stored in the system keyring — never in settings.json, \
                             logs or diagnostics.",
                        )
                        .small()
                        .color(theme.muted),
                    );
                });

            ui.add_space(12.0);
            // The invite is the step that actually makes a token useful: a bot in
            // no server has nothing to read, and that failure looks like a bug.
            egui::Frame::group(ui.style())
                .fill(theme.surface)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Add the bot to a server").strong().color(theme.text));
                    ui.label(
                        RichText::new(
                            "Invites the bot that the token above belongs to, asking only for what \
                             Rustcord uses: view channels, read history, send messages, embed links, \
                             attach files and add reactions.",
                        )
                        .small()
                        .color(theme.muted),
                    );

                    if let Some(client_id) = app.settings.oauth_client_id.clone() {
                        let url = crate::api::oauth::bot_invite_url(&client_id);
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button("Open the invite page").clicked() {
                                if let Err(err) = crate::api::oauth::open_browser(&url) {
                                    app.oauth.error = Some(format!(
                                        "{err:#} — open the link below yourself"
                                    ));
                                }
                            }
                            if ui.button("Copy invite link").clicked() {
                                ui.ctx().copy_text(url.clone());
                                app.status_line = Some("invite link copied".to_string());
                            }
                        });
                        ui.label(RichText::new(&url).small().color(theme.muted));
                    } else {
                        ui.label(
                            RichText::new(
                                "Set the client ID in the identity card below first; the invite is \
                                 built from it.",
                            )
                            .small()
                            .color(theme.muted),
                        );
                    }

                    ui.label(
                        RichText::new(
                            "A bot cannot read a server it has not joined, so this comes before the \
                             token can do anything.",
                        )
                        .small()
                        .color(theme.muted),
                    );
                });

            ui.add_space(12.0);
            // Discord identity (OAuth2): shows who you are, nothing more. It is
            // deliberately separate from the token above, and the copy says so.
            egui::Frame::group(ui.style())
                .fill(theme.surface)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Discord identity").strong().color(theme.text));
                    ui.label(
                        RichText::new(
                            "Optional. Shows your username, avatar and user id. Read-only: this \
                             cannot read or send messages, and Rustcord never asks for your \
                             password or a user token.",
                        )
                        .small()
                        .color(theme.muted),
                    );

                    if let Some(session) = app.identity.clone() {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                            ui.painter()
                                .circle_filled(rect.center(), 16.0, theme.accent);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                session
                                    .identity
                                    .display_name()
                                    .chars()
                                    .next()
                                    .unwrap_or('?')
                                    .to_uppercase()
                                    .to_string(),
                                egui::FontId::proportional(15.0),
                                theme.background,
                            );
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(session.identity.display_name())
                                        .strong()
                                        .color(theme.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "@{} · id {}",
                                        session.identity.username, session.identity.id
                                    ))
                                    .small()
                                    .color(theme.muted),
                                );
                            });
                        });

                        if app.identity_expired {
                            ui.label(
                                RichText::new(
                                    "This session has expired; sign in again to refresh it.",
                                )
                                .small()
                                .color(theme.danger),
                            );
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("Switch account").clicked() {
                                app.start_identity_sign_in();
                            }
                            if ui.button("Sign out of identity").clicked() {
                                app.sign_out_identity();
                            }
                        });
                    } else {
                        ui.add_space(6.0);
                        if app.settings.oauth_client_id.is_none() {
                            ui.label(
                                RichText::new(
                                    "Client ID from your Discord application (OAuth2 page). It is \
                                     public, not a secret.",
                                )
                                .small()
                                .color(theme.muted),
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
                                .color(theme.muted),
                            );
                            ui.add_space(4.0);
                        }

                        if app.oauth.pending.is_some() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                if ui.button("Cancel").clicked() {
                                    app.cancel_identity_sign_in();
                                }
                            });
                        } else if ui
                            .add_enabled(
                                app.settings.oauth_client_id.is_some(),
                                egui::Button::new("Sign in with Discord"),
                            )
                            .clicked()
                        {
                            app.start_identity_sign_in();
                        }
                    }

                    if let Some(status) = &app.oauth.status {
                        ui.label(RichText::new(status).small().color(theme.muted));
                    }
                    if let Some(error) = &app.oauth.error {
                        ui.label(RichText::new(error).small().color(theme.danger));
                    }
                });

            ui.add_space(12.0);
            // No forced `open` state: egui remembers the toggle per id, and
            // forcing it every frame would make the section uncloseable.
            egui::CollapsingHeader::new(
                RichText::new("Where do I get a token?")
                    .small()
                    .color(theme.muted),
            )
            .show(ui, |ui| {
                for step in [
                    "1. Open discord.com/developers/applications and pick your app.",
                    "2. Bot → Reset Token, then copy the whole string.",
                    "3. On the same Bot tab, enable Message Content Intent.",
                    "4. Invite it with the bot scope and View Channels + Read Message History.",
                ] {
                    ui.label(RichText::new(step).small().color(theme.muted));
                }
            });

            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Account passwords are not supported: Discord does not allow third-party \
                     user-account clients.",
                )
                .small()
                .color(theme.muted),
            );
        });
    });

    if submit {
        app.sign_in();
    }
    if demo {
        app.dismiss_login();
    }
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
}
