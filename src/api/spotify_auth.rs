//! Spotify OAuth2 (PKCE) and session storage, for the live now-playing poller.
//!
//! No client secret: the authorisation-code flow with PKCE is the one Spotify
//! documents for public clients, so the only thing stored is a refresh token in
//! the OS keyring (never in `settings.json`). The scope is deliberately the
//! smallest one that answers "what is playing": `user-read-currently-playing`.
//!
//! Flow, mirroring `api::oauth` for Discord:
//!
//! 1. A state value and PKCE verifier are generated; only the challenge is sent.
//! 2. The browser opens `accounts.spotify.com/authorize`; a loopback listener on
//!    `127.0.0.1:53135` waits for the redirect (a different port from the Discord
//!    identity flow, so the two can run at once).
//! 3. The callback's `state` must match, or the code is refused.
//! 4. The code is exchanged with the verifier; the session is saved in the keyring.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
pub const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
pub const SCOPE: &str = "user-read-currently-playing";
pub const REDIRECT_PORT: u16 = 53135;
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{REDIRECT_PORT}/callback")
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

pub fn authorize_url(client_id: &str, state: &str, challenge: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?client_id={client_id}&response_type=code&redirect_uri={redirect}\
         &scope={scope}&state={state}&code_challenge={challenge}&code_challenge_method=S256",
        redirect = url_encode(&redirect_uri()),
        scope = url_encode(SCOPE),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    #[serde(default)]
    pub scope: String,
    /// Which application issued the grant, so a changed id in settings does not
    /// silently reuse tokens from the old one.
    #[serde(default)]
    pub client_id: Option<String>,
}

impl Tokens {
    /// A minute of slack so a token cannot expire mid-request.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.saturating_sub(60) <= now
    }

    pub fn refresh_token(&self) -> Option<String> {
        self.refresh_token.clone().filter(|token| !token.is_empty())
    }
}

/// Spotify omits the refresh token on refresh grants sometimes; the old one stays
/// valid and must be kept or the next refresh has nothing to use.
pub fn merge_refresh(previous: &Tokens, mut fresh: Tokens) -> Tokens {
    if fresh.refresh_token.is_none() {
        fresh.refresh_token = previous.refresh_token.clone();
    }
    if fresh.client_id.is_none() {
        fresh.client_id = previous.client_id.clone();
    }
    fresh
}

pub fn parse_token_response(body: &str, now: u64) -> Result<Tokens> {
    #[derive(Deserialize)]
    struct Raw {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        scope: Option<String>,
    }

    let raw: Raw =
        serde_json::from_str(body).context("could not decode the Spotify token response")?;
    let access_token = raw
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow!("Spotify returned no access token"))?;

    Ok(Tokens {
        access_token,
        refresh_token: raw.refresh_token.filter(|token| !token.is_empty()),
        expires_at: now + raw.expires_in.unwrap_or(3600),
        scope: raw.scope.unwrap_or_else(|| SCOPE.to_string()),
        client_id: None,
    })
}

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
    body.chars().take(120).collect()
}

fn token_request(form: &[(&str, &str)]) -> Result<Tokens> {
    let response = reqwest::blocking::Client::builder()
        .user_agent("Rustcord/0.1")
        .timeout(Duration::from_secs(20))
        .build()
        .context("could not build the Spotify HTTP client")?
        .post(TOKEN_URL)
        .form(form)
        .send()
        .context("the Spotify token request did not leave the machine")?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Spotify refused the token request (HTTP {status}): {}",
            error_hint(&body)
        );
    }
    parse_token_response(&body, crate::api::oauth::now_unix())
}

pub fn exchange_code(client_id: &str, code: &str, verifier: &str) -> Result<Tokens> {
    let redirect = redirect_uri();
    let mut tokens = token_request(&[
        ("client_id", client_id),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect.as_str()),
        ("code_verifier", verifier),
    ])?;
    tokens.client_id = Some(client_id.to_string());
    Ok(tokens)
}

pub fn refresh_tokens(client_id: &str, refresh_token: &str) -> Result<Tokens> {
    token_request(&[
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ])
}

pub fn load_session() -> Option<Tokens> {
    crate::auth::load_spotify_session().and_then(|json| serde_json::from_str(&json).ok())
}

pub fn save_session(tokens: &Tokens) -> Result<()> {
    let json = serde_json::to_string(tokens).context("could not serialise the Spotify session")?;
    crate::auth::store_spotify_session(&json)
}

pub fn clear_session() {
    let _ = crate::auth::clear_spotify_session();
}

/// A running loopback listener for the Spotify redirect.
pub struct BrowserCallback {
    pub receiver: Receiver<Result<String>>,
    cancel: Arc<AtomicBool>,
}

impl BrowserCallback {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn page_for(result: &Result<String>) -> String {
    let (heading, detail) = match result {
        Ok(_) => (
            "Spotify is connected",
            "You can close this tab and return to Rustcord.",
        ),
        Err(_) => (
            "That connection did not work",
            "Return to Rustcord for the reason. You can close this tab.",
        ),
    };
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Rustcord</title>\
         <body style=\"font-family:system-ui,sans-serif;background:#121212;color:#eaeaea;\
         display:grid;place-items:center;height:100vh;margin:0\">\
         <main style=\"text-align:center\"><h1 style=\"font-size:20px\">{heading}</h1>\
         <p style=\"color:#a7a7a7\">{detail}</p></main></body>"
    )
}

pub fn listen_for_callback(expected_state: String) -> Result<BrowserCallback> {
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).with_context(|| {
        format!("could not listen on 127.0.0.1:{REDIRECT_PORT} — another connection may hold it")
    })?;
    listener.set_nonblocking(true)?;

    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);

    std::thread::Builder::new()
        .name("rustcord-spotify-auth".to_string())
        .spawn(move || {
            let deadline = Instant::now() + CALLBACK_TIMEOUT;
            loop {
                if flag.load(Ordering::Relaxed) {
                    let _ = sender.send(Err(anyhow!("connection cancelled")));
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

                        // The same callback parser as the Discord flow: the state
                        // check happens before the code is looked at.
                        let result =
                            match crate::api::oauth::parse_callback(&target, &expected_state) {
                                Ok(crate::api::oauth::Callback::Code(code)) => Ok(code),
                                Ok(crate::api::oauth::Callback::Denied(reason)) => {
                                    Err(anyhow!("Spotify reported {reason}"))
                                }
                                Err(err) => Err(err),
                            };

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
        .context("could not start the Spotify callback listener")?;

    Ok(BrowserCallback { receiver, cancel })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_responses_are_parsed_with_an_expiry() {
        let tokens = parse_token_response(
            r#"{"access_token":"abc","token_type":"Bearer","expires_in":3600,"refresh_token":"r1","scope":"user-read-currently-playing"}"#,
            1_000,
        )
        .expect("parse");
        assert_eq!(tokens.access_token, "abc");
        assert_eq!(tokens.refresh_token.as_deref(), Some("r1"));
        assert_eq!(tokens.expires_at, 4_600);
        assert!(!tokens.is_expired(4_000));
        // A minute of slack: 4,541 is within 60 s of expiry.
        assert!(tokens.is_expired(4_541));

        // Refresh grants may omit the scope.
        let refreshed = parse_token_response(r#"{"access_token":"abc2","expires_in":3600}"#, 10)
            .expect("parse");
        assert!(refreshed.scope.contains("currently-playing"));
        assert!(parse_token_response("{}", 0).is_err());
    }

    #[test]
    fn a_refresh_response_keeps_the_previous_refresh_token() {
        let previous = Tokens {
            access_token: "old".to_string(),
            refresh_token: Some("keep-me".to_string()),
            expires_at: 10,
            scope: SCOPE.to_string(),
            client_id: Some("app1".to_string()),
        };
        let fresh = Tokens {
            access_token: "new".to_string(),
            refresh_token: None,
            expires_at: 3_610,
            scope: SCOPE.to_string(),
            client_id: None,
        };
        let merged = merge_refresh(&previous, fresh.clone());
        assert_eq!(merged.refresh_token.as_deref(), Some("keep-me"));
        assert_eq!(merged.access_token, "new");
        assert_eq!(merged.client_id.as_deref(), Some("app1"));

        // A rotated token wins.
        let rotated = Tokens {
            refresh_token: Some("rotated".to_string()),
            ..fresh
        };
        assert_eq!(
            merge_refresh(&previous, rotated).refresh_token.as_deref(),
            Some("rotated")
        );
        assert!(previous.refresh_token().is_some());
    }

    #[test]
    fn the_authorize_url_asks_for_one_scope_and_a_challenge() {
        let url = authorize_url("abc123", "state-value", "challenge-value");
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=user-read-currently-playing"));
        // The redirect is percent-encoded, so its own `:` and `/` cannot split
        // the query.
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53135%2Fcallback"));
        assert!(url.contains("state=state-value"));
        // Ports differ from the Discord identity flow, so both can run.
        assert_ne!(REDIRECT_PORT, crate::api::oauth::REDIRECT_PORT);
    }
}
