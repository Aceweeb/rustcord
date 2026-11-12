use anyhow::{Context, Result};

const SERVICE: &str = "rustcord";
const ACCOUNT: &str = "discord_bot_token";

/// The OAuth2 *identity* session (username, avatar, id), kept apart from the bot
/// token so neither can be mistaken for the other. Holds no messaging grant.
const OAUTH_ACCOUNT: &str = "discord_oauth_identity";

/// Store the Discord bot token in the OS keyring (Secret Service on Arch).
/// Tokens are never written to `settings.json`.
pub fn store_discord_token(token: &str) -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, ACCOUNT).context("could not open the system keyring entry")?;
    entry
        .set_password(token)
        .context("could not store the token in the system keyring")?;
    Ok(())
}

pub fn load_discord_token() -> Option<String> {
    keyring::Entry::new(SERVICE, ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|token| !token.trim().is_empty())
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

pub fn clear_discord_token() -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, ACCOUNT).context("could not open the system keyring entry")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting a non-existent credential is not an error for our purposes.
        Err(_) => Ok(()),
    }
}
