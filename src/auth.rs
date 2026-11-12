use anyhow::{Context, Result};

const SERVICE: &str = "rustcord";
const ACCOUNT: &str = "discord_bot_token";

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

pub fn clear_discord_token() -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, ACCOUNT).context("could not open the system keyring entry")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting a non-existent credential is not an error for our purposes.
        Err(_) => Ok(()),
    }
}
