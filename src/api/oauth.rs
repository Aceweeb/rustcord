//! Discord OAuth2, used for **identity only**.
//!
//! This is deliberately not a way into a personal account. The `identify` scope
//! lets Rustcord show who signed in — username, avatar, id — and nothing else.
//! A user token for messaging would make this a self-bot, which `DECISIONS.md`
//! D2 forbids and which is what gets accounts terminated. The accumulated
//! evidence is that Discord offers no third-party API for reading or sending
//! messages as a person; Rustcord therefore keeps bot tokens for that job and
//! labels this sign-in as identity-only everywhere it appears.
//!
//! The flow:
//!
//! 1. `new_state()` and a PKCE verifier are generated; only their hashes leave
//!    the process (`authorize_url`).
//! 2. The browser is opened at Discord's authorise endpoint; a loopback listener
//!    waits for the redirect on `127.0.0.1:53134`.
//! 3. `parse_callback` rejects any callback whose `state` does not match, so a
//!    stray or forged redirect cannot inject a code.
//! 4. `exchange_code` trades the code (with the PKCE verifier, no client secret)
//!    for tokens; `fetch_identity` reads `/users/@me`.
//! 5. The session is stored in the OS keyring through `SessionStore`, which is a
//!    trait so restoration and logout are testable without a keyring.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
pub const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
pub const USER_URL: &str = "https://discord.com/api/v10/users/@me";
pub const SCOPE: &str = "identify";

/// Loopback port for the redirect. Must match the redirect URI registered on the
/// Discord application exactly; the login page says so.
pub const REDIRECT_PORT: u16 = 53134;

pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{REDIRECT_PORT}/callback")
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple as usize & 0x3f] as char);
        }
    }
    out
}

/// `BASE64URL(SHA256(verifier))`, as RFC 7636 defines it.
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64_url(&digest)
}

pub fn random_urlsafe(bytes: usize) -> Result<String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|err| anyhow!("no system randomness available: {err}"))?;
    Ok(base64_url(&buffer))
}

pub fn new_state() -> Result<String> {
    random_urlsafe(32)
}

/// The code verifier: 43–128 characters of `[A-Za-z0-9-._~]` per RFC 7636, which
/// 32 random bytes in base64url satisfy (43 characters).
pub fn new_verifier() -> Result<String> {
    random_urlsafe(32)
}

pub fn authorize_url(client_id: &str, state: &str, challenge: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?client_id={client_id}&response_type=code&redirect_uri={redirect}&scope={SCOPE}\
         &state={state}&code_challenge={challenge}&code_challenge_method=S256&prompt=consent",
        redirect = url_encode(&redirect_uri()),
    )
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let decoded = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(byte) = decoded {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Denied(String),
}

/// Validate a redirect request target (`/callback?code=…&state=…`).
///
/// The state check comes first: a callback carrying a code but the wrong state
/// is rejected as if it carried nothing at all.
pub fn parse_callback(target: &str, expected_state: &str) -> Result<Callback> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    if path != "/callback" && path != "/callback/" {
        bail!("unexpected callback path");
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;

    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match percent_decode(key).as_str() {
            "code" => code = Some(percent_decode(value)),
            "state" => state = Some(percent_decode(value)),
            "error" => error = Some(percent_decode(value)),
            _ => {}
        }
    }

    let state = state.ok_or_else(|| anyhow!("callback carried no state"))?;
    if state != expected_state {
        bail!("state mismatch: this callback did not start from this sign-in");
    }

    if let Some(error) = error {
        return Ok(Callback::Denied(error));
    }

    let code = code.ok_or_else(|| anyhow!("callback carried no code"))?;
    if code.is_empty() {
        bail!("callback carried an empty code");
    }

    Ok(Callback::Code(code))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

impl Identity {
    /// The name to show: Discord's display name when set, else the username.
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    pub fn avatar_url(&self) -> Option<String> {
        self.avatar.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=64",
                self.id
            )
        })
    }
}

pub fn parse_identity(body: &str) -> Result<Identity> {
    let identity: Identity =
        serde_json::from_str(body).context("could not decode the Discord user")?;
    if identity.id.is_empty() {
        bail!("Discord returned a user without an id");
    }
    Ok(identity)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    pub scope: String,
}

impl Tokens {
    /// Expiry with a minute of slack, so a token cannot die mid-request.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.saturating_sub(60) <= now
    }
}

pub fn parse_token_response(body: &str, now: u64) -> Result<Tokens> {
    #[derive(Deserialize)]
    struct Raw {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        scope: Option<String>,
    }

    let raw: Raw = serde_json::from_str(body).context("could not decode the token response")?;
    let access_token = raw
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow!("the token response carried no access token"))?;

    Ok(Tokens {
        access_token,
        refresh_token: raw.refresh_token.filter(|token| !token.is_empty()),
        expires_at: now + raw.expires_in.unwrap_or(6 * 24 * 60 * 60),
        scope: raw.scope.unwrap_or_else(|| SCOPE.to_string()),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub tokens: Tokens,
    pub identity: Identity,
}

/// Where a session is kept. The keyring in production, memory in tests.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Option<Session>;
    fn save(&self, session: &Session) -> Result<()>;
    fn clear(&self);
}

pub struct KeyringStore;

impl SessionStore for KeyringStore {
    fn load(&self) -> Option<Session> {
        crate::auth::load_oauth_session().and_then(|json| serde_json::from_str(&json).ok())
    }

    fn save(&self, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session).context("could not serialise the session")?;
        crate::auth::store_oauth_session(&json)
    }

    fn clear(&self) {
        let _ = crate::auth::clear_oauth_session();
    }
}

#[derive(Default)]
pub struct MemoryStore {
    slot: Mutex<Option<Session>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for MemoryStore {
    fn load(&self) -> Option<Session> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn save(&self, session: &Session) -> Result<()> {
        *self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session.clone());
        Ok(())
    }

    fn clear(&self) {
        *self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[derive(Debug, Clone)]
pub enum Restore {
    Missing,
    Active(Session),
    Expired(Session),
}

/// What a fresh start should do with whatever was stored last time.
pub fn restore(store: &dyn SessionStore, now: u64) -> Restore {
    match store.load() {
        None => Restore::Missing,
        Some(session) if session.tokens.is_expired(now) => Restore::Expired(session),
        Some(session) => Restore::Active(session),
    }
}

fn http() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("Rustcord/0.1")
        .timeout(Duration::from_secs(20))
        .build()
        .context("could not build the HTTP client")
}

/// Discord puts the reason in a short JSON body; never echo a whole body.
fn error_hint(body: &str) -> String {
    #[derive(Deserialize)]
    struct Failure {
        error: Option<String>,
        error_description: Option<String>,
    }

    if let Ok(failure) = serde_json::from_str::<Failure>(body) {
        if let Some(description) = failure.error_description.or(failure.error) {
            return description;
        }
    }

    let trimmed: String = body.chars().take(120).collect();
    if trimmed.is_empty() {
        "no details".to_string()
    } else {
        trimmed
    }
}

fn token_request(form: &[(&str, &str)]) -> Result<Tokens> {
    let response = http()?
        .post(TOKEN_URL)
        .form(form)
        .send()
        .context("the token request did not reach Discord")?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Discord refused the token request (HTTP {status}): {}",
            error_hint(&body)
        );
    }

    parse_token_response(&body, now_unix())
}

pub fn exchange_code(client_id: &str, code: &str, verifier: &str) -> Result<Tokens> {
    let redirect = redirect_uri();
    token_request(&[
        ("client_id", client_id),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect.as_str()),
        ("code_verifier", verifier),
    ])
}

pub fn refresh_tokens(client_id: &str, refresh_token: &str) -> Result<Tokens> {
    token_request(&[
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ])
}

pub fn fetch_identity(access_token: &str) -> Result<Identity> {
    let response = http()?
        .get(USER_URL)
        .bearer_auth(access_token)
        .send()
        .context("the identity request did not reach Discord")?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Discord refused the identity request (HTTP {status}): {}",
            error_hint(&body)
        );
    }

    parse_identity(&body)
}

/// A running loopback listener. Dropping it does not stop the thread; use
/// `cancel` for that.
pub struct BrowserCallback {
    pub receiver: Receiver<Result<Callback>>,
    cancel: Arc<AtomicBool>,
}

impl BrowserCallback {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The `error` parameter comes from the URL, so it is escaped before it reaches
/// the page rather than trusted.
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn page_for(result: &Result<Callback>) -> String {
    let (heading, detail) = match result {
        Ok(Callback::Code(_)) => (
            "Rustcord is signed in".to_string(),
            "You can close this tab and go back to Rustcord.".to_string(),
        ),
        Ok(Callback::Denied(reason)) => (
            "Sign-in was cancelled".to_string(),
            format!(
                "Discord reported \u{201c}{}\u{201d}. Nothing was stored.",
                escape_html(reason)
            ),
        ),
        Err(_) => (
            "That sign-in did not work".to_string(),
            "Return to Rustcord for the reason. You can close this tab.".to_string(),
        ),
    };

    // Deliberately boring HTML: no scripts, no remote assets.
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Rustcord</title>\
         <body style=\"font-family:system-ui,sans-serif;background:#1e1f22;color:#dbdee1;\
         display:grid;place-items:center;height:100vh;margin:0\">\
         <main style=\"text-align:center\"><h1 style=\"font-size:20px\">{heading}</h1>\
         <p style=\"color:#949ba4\">{detail}</p></main></body>"
    )
}

/// Listen for the OAuth redirect on the loopback port.
///
/// The listener is non-blocking and polls, so a cancel or a timeout is noticed
/// without needing to interrupt an `accept` call.
pub fn listen_for_callback(expected_state: String) -> Result<BrowserCallback> {
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).with_context(|| {
        format!(
            "could not listen on 127.0.0.1:{REDIRECT_PORT} — another sign-in or process may hold it"
        )
    })?;
    listener.set_nonblocking(true)?;

    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);

    std::thread::Builder::new()
        .name("rustcord-oauth".to_string())
        .spawn(move || {
            let deadline = Instant::now() + CALLBACK_TIMEOUT;

            loop {
                if flag.load(Ordering::Relaxed) {
                    let _ = sender.send(Err(anyhow!("sign-in cancelled")));
                    return;
                }
                if Instant::now() > deadline {
                    let _ = sender.send(Err(anyhow!(
                        "no answer from the browser within {}s",
                        CALLBACK_TIMEOUT.as_secs()
                    )));
                    return;
                }

                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0u8; 4096];
                        let read = stream.read(&mut buffer).unwrap_or(0);
                        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                        let target = request.split_whitespace().nth(1).unwrap_or("/").to_string();

                        let result = parse_callback(&target, &expected_state);
                        let body = page_for(&result);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();

                        let _ = sender.send(result);
                        return;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(err) => {
                        let _ = sender.send(Err(anyhow!("the callback listener failed: {err}")));
                        return;
                    }
                }
            }
        })
        .context("could not start the OAuth listener")?;

    Ok(BrowserCallback { receiver, cancel })
}

pub fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]).arg(url);
        command
    };

    command.spawn().context("could not open the browser")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(expires_at: u64) -> Session {
        Session {
            tokens: Tokens {
                access_token: "access".to_string(),
                refresh_token: Some("refresh".to_string()),
                expires_at,
                scope: SCOPE.to_string(),
            },
            identity: Identity {
                id: "900".to_string(),
                username: "alice".to_string(),
                global_name: Some("Alice".to_string()),
                avatar: Some("abc".to_string()),
            },
        }
    }

    #[test]
    fn base64url_matches_known_vectors() {
        assert_eq!(base64_url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"fo"), "Zm8");
        // The URL-safe alphabet, not '+'.
        assert_eq!(base64_url(&[0xfb, 0xff, 0xbf]), "-_-_");
    }

    #[test]
    fn pkce_challenge_matches_the_rfc_vector() {
        // RFC 7636, Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_values_are_urlsafe_and_distinct() {
        let first = new_state().expect("state");
        let second = new_state().expect("state");
        assert_ne!(first, second);
        assert_eq!(first.len(), 43);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn the_authorize_url_carries_everything_discord_needs() {
        let url = authorize_url("1234567890", "state-value", "challenge-value");

        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains("client_id=1234567890"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=identify"));
        assert!(url.contains("state=state-value"));
        assert!(url.contains("code_challenge=challenge-value"));
        assert!(url.contains("code_challenge_method=S256"));
        // The redirect URI is percent-encoded, and only the loopback host appears.
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53134%2Fcallback"));
    }

    #[test]
    fn a_matching_callback_yields_the_code() {
        let callback = parse_callback(
            "/callback?code=the-code&state=the-state&extra=1",
            "the-state",
        )
        .expect("callback");
        assert_eq!(callback, Callback::Code("the-code".to_string()));
    }

    #[test]
    fn a_forged_state_is_rejected_even_with_a_code() {
        let error = parse_callback("/callback?code=stolen&state=attacker", "the-state")
            .expect_err("state mismatch");
        assert!(error.to_string().contains("state mismatch"));
    }

    #[test]
    fn a_callback_without_state_or_code_is_rejected() {
        assert!(parse_callback("/callback?code=abc", "state").is_err());
        assert!(parse_callback("/callback?state=state", "state").is_err());
        assert!(parse_callback("/callback?code=&state=state", "state").is_err());
        assert!(parse_callback("/elsewhere?code=abc&state=state", "state").is_err());
    }

    #[test]
    fn a_denial_is_reported_as_a_denial_not_an_error() {
        let callback =
            parse_callback("/callback?error=access_denied&state=state", "state").expect("callback");
        assert_eq!(callback, Callback::Denied("access_denied".to_string()));
    }

    #[test]
    fn codes_are_percent_decoded() {
        let callback = parse_callback("/callback?code=a%2Bb%2Fc&state=the%20state", "the state")
            .expect("callback");
        assert_eq!(callback, Callback::Code("a+b/c".to_string()));
    }

    #[test]
    fn token_responses_are_parsed_with_an_expiry() {
        let body = r#"{
            "access_token": "at",
            "refresh_token": "rt",
            "expires_in": 604800,
            "scope": "identify",
            "token_type": "Bearer"
        }"#;

        let tokens = parse_token_response(body, 1_000).expect("tokens");
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert_eq!(tokens.expires_at, 1_000 + 604_800);
        assert!(!tokens.is_expired(1_000));
        assert!(tokens.is_expired(1_000 + 604_800));
    }

    #[test]
    fn a_token_response_without_an_access_token_is_an_error() {
        assert!(parse_token_response(r#"{"scope":"identify"}"#, 0).is_err());
        assert!(parse_token_response(r#"{"access_token":""}"#, 0).is_err());
        assert!(parse_token_response("not json", 0).is_err());
    }

    #[test]
    fn identity_parsing_prefers_the_display_name() {
        let identity = parse_identity(
            r#"{"id":"900","username":"alice","global_name":"Alice","avatar":"abc"}"#,
        )
        .expect("identity");
        assert_eq!(identity.display_name(), "Alice");
        assert_eq!(
            identity.avatar_url().as_deref(),
            Some("https://cdn.discordapp.com/avatars/900/abc.png?size=64")
        );

        let plain = parse_identity(r#"{"id":"901","username":"bob","global_name":null}"#)
            .expect("identity");
        assert_eq!(plain.display_name(), "bob");
        assert_eq!(plain.avatar_url(), None);
    }

    #[test]
    fn an_identity_without_an_id_is_an_error() {
        assert!(parse_identity(r#"{"username":"nobody"}"#).is_err());
        assert!(parse_identity("{}").is_err());
    }

    #[test]
    fn restoration_distinguishes_missing_active_and_expired() {
        let store = MemoryStore::new();
        assert!(matches!(restore(&store, 100), Restore::Missing));

        store.save(&session(1_000)).expect("save");
        assert!(matches!(restore(&store, 100), Restore::Active(_)));
        assert!(matches!(restore(&store, 2_000), Restore::Expired(_)));
    }

    #[test]
    fn a_session_round_trips_and_logout_forgets_it() {
        let store = MemoryStore::new();
        store.save(&session(5_000)).expect("save");

        let loaded = store.load().expect("session");
        assert_eq!(loaded.identity.username, "alice");
        assert_eq!(loaded.tokens.refresh_token.as_deref(), Some("refresh"));

        store.clear();
        assert!(store.load().is_none());
        assert!(matches!(restore(&store, 0), Restore::Missing));
    }

    #[test]
    fn a_stored_session_survives_json() {
        let json = serde_json::to_string(&session(1_700_000_000)).expect("serialise");
        let parsed: Session = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed.identity.id, "900");
        assert_eq!(parsed.tokens.scope, "identify");
        assert_eq!(parsed.tokens.expires_at, 1_700_000_000);
    }

    #[test]
    fn the_scope_is_identify_only() {
        // A guard against widening this flow by accident: messaging needs a bot.
        assert_eq!(SCOPE, "identify");
    }
}
