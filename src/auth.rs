use anyhow::{Context, Result};

/// Keyring service name for all Rustcord credentials.
const SERVICE: &str = "rustcord";

/// Bot token (for messaging, gateways, REST). The only token the client uses
/// to act on Discord. Stored separately from identity and personal-account
/// credentials so they can never be confused.
const BOT_TOKEN_ACCOUNT: &str = "discord_bot_token";

/// OAuth2 *identity* session (username, avatar, id), kept apart from the bot
/// token so neither can be mistaken for the other. Holds no messaging grant.
const OAUTH_ACCOUNT: &str = "discord_oauth_identity";

/// A personal-account token pasted by the user or obtained through QR linking.
/// Stored in its own keyring entry; never used for messaging unless the user
/// explicitly chooses it (and accepts the self-bot risk documented in D2/D15).
const PERSONAL_TOKEN_ACCOUNT: &str = "discord_personal_token";

/// The Spotify session (access + refresh token) for the now-playing poller. A
/// separate entry so it cannot be mistaken for a Discord credential.
const SPOTIFY_ACCOUNT: &str = "spotify_session";

/// Store the Discord bot token in the OS keyring (Secret Service on Arch).
/// Tokens are never written to `settings.json`.
pub fn store_discord_token(token: &str) -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, BOT_TOKEN_ACCOUNT).context("could not open the system keyring entry")?;
    entry
        .set_password(token)
        .context("could not store the token in the system keyring")?;
    Ok(())
}

pub fn load_discord_token() -> Option<String> {
    keyring::Entry::new(SERVICE, BOT_TOKEN_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|token| !token.trim().is_empty())
}

/// Store a personal-account token in the OS keyring. The caller is responsible
/// for ensuring the user understands the self-bot implications (see DECISIONS.md
/// D2/D15). The token is stored raw (no `Bearer`/`Bot` prefix) so the caller
/// chooses the auth scheme at request time.
pub fn store_personal_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, PERSONAL_TOKEN_ACCOUNT)
        .context("could not open the system keyring entry for the personal token")?;
    entry
        .set_password(token)
        .context("could not store the personal token in the system keyring")?;
    Ok(())
}

pub fn load_personal_token() -> Option<String> {
    keyring::Entry::new(SERVICE, PERSONAL_TOKEN_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|token| !token.trim().is_empty())
}

pub fn clear_personal_token() -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, PERSONAL_TOKEN_ACCOUNT)
        .context("could not open the system keyring entry for the personal token")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting a missing credential is not an error for our purposes.
        Err(_) => Ok(()),
    }
}

/// Store the serialised OAuth2 identity session. Callers pass JSON so the shape
/// stays in `api::oauth`; this module only knows about the keyring.
pub fn store_oauth_session(json: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, OAUTH_ACCOUNT)
        .context("could not open the keyring entry for the Discord identity")?;
    entry
        .set_password(json)
        .context("could not store the Discord identity in the system keyring")?;
    Ok(())
}

pub fn load_oauth_session() -> Option<String> {
    keyring::Entry::new(SERVICE, OAUTH_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|json| !json.trim().is_empty())
}

pub fn clear_oauth_session() -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, OAUTH_ACCOUNT)
        .context("could not open the keyring entry for the Discord identity")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting a missing credential is not an error for our purposes.
        Err(_) => Ok(()),
    }
}

pub fn store_spotify_session(json: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, SPOTIFY_ACCOUNT)
        .context("could not open the keyring entry for Spotify")?;
    entry
        .set_password(json)
        .context("could not store the Spotify session in the system keyring")?;
    Ok(())
}

pub fn load_spotify_session() -> Option<String> {
    keyring::Entry::new(SERVICE, SPOTIFY_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|json| !json.trim().is_empty())
}

pub fn clear_spotify_session() -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, SPOTIFY_ACCOUNT)
        .context("could not open the keyring entry for Spotify")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(_) => Ok(()),
    }
}

pub fn clear_discord_token() -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, BOT_TOKEN_ACCOUNT).context("could not open the system keyring entry")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting a non-existent credential is not an error for our purposes.
        Err(_) => Ok(()),
    }
}
