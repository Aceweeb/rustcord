//! Live "now playing" polling for the Spotify plugin (`DECISIONS.md` D43).
//!
//! One worker thread owns the session and asks Spotify every few seconds what is
//! playing, refreshing the access token in place when it is close to expiry. The
//! UI only drains updates, so a slow or failing request can never block a frame.
//! The thread stops when the poller is dropped (the stop channel disconnects).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::api::spotify::{SpotifyClient, SpotifyTrack};
use crate::api::spotify_auth::{self, Tokens};

/// How often Spotify is asked. Five seconds keeps the progress bar close without
/// spending the rate limit.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum Update {
    Track(SpotifyTrack),
    /// Nothing is playing (or playback is paused).
    Idle,
    /// Something went wrong; the poller keeps trying.
    Error(String),
}

/// Handle to the polling thread. Dropping it stops the thread at its next wake.
pub struct Poller {
    stop: Option<Sender<()>>,
    updates: Receiver<Update>,
}

impl Poller {
    pub fn start(client_id: String, tokens: Tokens) -> Self {
        let (stop_tx, stop_rx) = mpsc::channel();
        let (update_tx, updates) = mpsc::channel();

        let spawned = std::thread::Builder::new()
            .name("spotify-sync".to_string())
            .spawn(move || run(client_id, tokens, &stop_rx, &update_tx));
        if let Err(err) = spawned {
            log::warn!("could not start the Spotify poller: {err}");
        }

        Self {
            stop: Some(stop_tx),
            updates,
        }
    }

    pub fn try_recv(&self) -> Option<Update> {
        self.updates.try_recv().ok()
    }

    /// Stop the thread and drop the handle's stop channel.
    pub fn stop(&mut self) {
        self.stop = None;
    }

    pub fn active(&self) -> bool {
        self.stop.is_some()
    }
}

fn run(client_id: String, mut tokens: Tokens, stop: &Receiver<()>, updates: &Sender<Update>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = updates.send(Update::Error(format!(
                "could not start the Spotify runtime: {err}"
            )));
            return;
        }
    };

    let mut last_error: Option<String> = None;

    loop {
        match poll_once(&runtime, &client_id, &mut tokens) {
            Ok(Some(track)) => {
                last_error = None;
                let _ = updates.send(Update::Track(track));
            }
            Ok(None) => {
                last_error = None;
                let _ = updates.send(Update::Idle);
            }
            Err(err) => {
                // The same failure repeated every five seconds is noise; report it
                // once and stay quiet until something changes.
                let message = format!("{err:#}");
                if last_error.as_deref() != Some(message.as_str()) {
                    let _ = updates.send(Update::Error(message.clone()));
                    last_error = Some(message);
                }
                // Force a refresh attempt next round: the usual cause is an
                // expired token.
                tokens.expires_at = 0;
            }
        }

        match stop.recv_timeout(POLL_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn poll_once(
    runtime: &tokio::runtime::Runtime,
    client_id: &str,
    tokens: &mut Tokens,
) -> anyhow::Result<Option<SpotifyTrack>> {
    if tokens.is_expired(crate::api::oauth::now_unix()) {
        let refresh = tokens.refresh_token().ok_or_else(|| {
            anyhow::anyhow!("the Spotify session has no refresh token; reconnect")
        })?;
        let fresh = spotify_auth::refresh_tokens(client_id, &refresh)?;
        *tokens = spotify_auth::merge_refresh(tokens, fresh);
        // A refreshed token is worth keeping across restarts.
        if let Err(err) = spotify_auth::save_session(tokens) {
            log::warn!("could not save the refreshed Spotify session: {err:#}");
        }
    }

    let access_token = tokens.access_token.clone();
    runtime.block_on(async move { SpotifyClient::new(access_token)?.currently_playing().await })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_without_a_refresh_token_cannot_be_refreshed() {
        let tokens = Tokens {
            access_token: "abc".to_string(),
            refresh_token: None,
            expires_at: 0,
            scope: spotify_auth::SCOPE.to_string(),
            client_id: Some("app1".to_string()),
        };
        assert!(tokens.refresh_token().is_none());
    }

    #[test]
    fn an_idle_update_is_distinguishable_from_a_track() {
        let idle = Update::Idle;
        assert!(matches!(idle, Update::Idle));
        assert!(!matches!(Update::Error("x".to_string()), Update::Track(_)));
    }
}
