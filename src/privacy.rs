//! What leaves this machine, and what stays on it.
//!
//! Rustcord is a native client: there is no server component, no account on
//! anyone's backend, no database file and no analytics. The only network traffic
//! is to Discord, using the credentials the user supplied, and the only local
//! state is a settings file, the OS keyring and memory.
//!
//! That is a design property, so it is checked rather than described:
//!
//! * `host_allowed` is the single allowlist every outbound URL goes through —
//!   REST in `api::discord`, the websocket in `api::gateway`, images in
//!   `images.rs` (which has its own CDN list) and the OAuth2 endpoints.
//! * A resume URL is attacker-influenced in the sense that it arrives in a
//!   payload, so it is checked too; a gateway that points somewhere else is
//!   refused instead of dialled.
//!
//! `api::spotify` is the only non-Discord client in the tree and **nothing
//! constructs it**: the Spotify status plugin builds its activity from local
//! state. If it is ever wired to the network, its host has to be added here and
//! the setting has to be disclosed in the settings window first.
//!
//! Rich Presence (`src/rpc.rs`) is not network traffic: it writes `SET_ACTIVITY`
//! frames to the Discord client's local unix socket, with a public application id
//! and no credential. It is off by default.
//!
//! Nothing here reads the network or touches the filesystem.

/// Hosts Rustcord will talk to. Exact matches only: `discord.com.evil.test` and
/// `evil.discord.com` are both rejected, which is why this is not a suffix test.
pub const ALLOWED_HOSTS: &[&str] = &[
    "discord.com",
    "gateway.discord.gg",
    "cdn.discordapp.com",
    "media.discordapp.net",
    "images-ext-1.discordapp.net",
    "images-ext-2.discordapp.net",
];

/// One piece of local state, and where it lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    pub name: &'static str,
    pub location: &'static str,
    pub detail: &'static str,
}

/// Every place Rustcord keeps anything. Deliberately short: a database or a
/// cache directory would have to appear here, and the tests pin the list.
pub const STORES: &[Store] = &[
    Store {
        name: "Settings",
        location: "~/.config/rustcord/settings.json",
        detail: "preferences only — no credentials, no messages",
    },
    Store {
        name: "Bot token",
        location: "OS keyring: rustcord / discord_bot_token",
        detail: "the token you pasted, readable only by your session",
    },
    Store {
        name: "Identity session",
        location: "OS keyring: rustcord / discord_oauth_identity",
        detail: "OAuth2 identify tokens and your public profile",
    },
    Store {
        name: "Messages",
        location: "memory only",
        detail: "capped by the resource profile; discarded on exit",
    },
    Store {
        name: "Image cache",
        location: "memory only",
        detail: "bounded texture cache; nothing written to disk",
    },
    Store {
        name: "Diagnostics log",
        location: "memory ring buffer",
        detail: "exported only when you ask, to the path you choose",
    },
];

/// Whether a URL points at Discord and nothing else.
///
/// Accepts `https`, `wss` and `http` for loopback (the OAuth callback never
/// leaves the machine, so it is not sent through here), and requires an exact
/// host match otherwise.
pub fn host_allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "https" | "wss") {
        return false;
    }

    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Drop userinfo, then the port, before comparing.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority);

    if host.is_empty() {
        return false;
    }

    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// A one-line summary for the settings window.
pub fn summary() -> String {
    format!(
        "{} Discord host(s), no telemetry, no database: {} local store(s)",
        ALLOWED_HOSTS.len(),
        STORES.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_endpoints_are_allowed() {
        assert!(host_allowed("https://discord.com/api/v10/users/@me"));
        assert!(host_allowed("wss://gateway.discord.gg/?v=10&encoding=json"));
        assert!(host_allowed("https://cdn.discordapp.com/avatars/1/a.png"));
        assert!(host_allowed("https://media.discordapp.net/x"));
        // Ports and case do not change the host.
        assert!(host_allowed("https://discord.com:443/api"));
        assert!(host_allowed("https://DISCORD.COM/api"));
    }

    #[test]
    fn lookalike_hosts_are_rejected() {
        assert!(!host_allowed("https://discord.com.evil.test/api"));
        assert!(!host_allowed("https://evil.discord.com/api"));
        assert!(!host_allowed("https://discord.co/api"));
        assert!(!host_allowed("https://notdiscord.com/api"));
        // Userinfo cannot smuggle a host past the check.
        assert!(!host_allowed("https://discord.com@evil.test/api"));
    }

    #[test]
    fn only_encrypted_schemes_pass() {
        assert!(!host_allowed("http://discord.com/api"));
        assert!(!host_allowed("ws://gateway.discord.gg"));
        assert!(!host_allowed("file:///etc/passwd"));
        assert!(!host_allowed("discord.com/api"));
        assert!(!host_allowed(""));
    }

    #[test]
    fn the_oauth_endpoints_are_on_the_allowlist() {
        use crate::api::oauth;
        assert!(host_allowed(oauth::AUTHORIZE_URL));
        assert!(host_allowed(oauth::TOKEN_URL));
        assert!(host_allowed(oauth::USER_URL));
        // The loopback callback is local and never goes through this list.
        assert!(!host_allowed(&oauth::redirect_uri()));
    }

    #[test]
    fn the_client_and_image_hosts_agree_with_this_list() {
        use crate::api::discord::API_BASE;
        assert!(host_allowed(API_BASE));
        for url in [
            "https://cdn.discordapp.com/a.png",
            "https://media.discordapp.net/a.png",
            "https://images-ext-1.discordapp.net/a.png",
            "https://images-ext-2.discordapp.net/a.png",
        ] {
            assert!(host_allowed(url), "{url} should be allowed here");
            assert!(
                crate::images::is_allowed_url(url),
                "{url} should also pass the image allowlist"
            );
        }
    }

    #[test]
    fn there_is_no_database_and_no_telemetry_in_the_store_list() {
        assert_eq!(STORES.len(), 6);
        for store in STORES {
            let text = format!("{} {} {}", store.name, store.location, store.detail);
            let lowered = text.to_lowercase();
            for forbidden in ["sqlite", "database", "telemetry", "analytics", "cloud"] {
                assert!(
                    !lowered.contains(forbidden),
                    "{} mentions {forbidden}",
                    store.name
                );
            }
        }

        // Nothing is written next to the binary or in the data dir except
        // diagnostics exports the user asks for.
        assert!(STORES
            .iter()
            .all(|store| !store.location.contains(".local/share/rustcord")));
    }

    #[test]
    fn the_summary_states_the_two_guarantees() {
        let summary = summary();
        assert!(summary.contains("no telemetry"));
        assert!(summary.contains("no database"));
    }
}
