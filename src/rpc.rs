//! Discord Rich Presence over the official local IPC socket (`DECISIONS.md`
//! D41).
//!
//! This is the socket the Discord desktop client opens at
//! `$XDG_RUNTIME_DIR/discord-ipc-0` (inside the Flatpak/Snap sandbox it lives in
//! the app's own runtime directory). No token, no HTTP, no gateway: the client
//! sends `SET_ACTIVITY` frames to the Discord instance already running on this
//! machine, which is the same path games and music players use.
//!
//! The connection is owned by one worker thread so the UI never blocks on a
//! socket that is missing, slow or closing. The thread reconnects on its own and
//! replays the last presence it was told about, while the UI keeps only a
//! `Sender` and the current client id.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde_json::{json, Value};

use crate::settings::Settings;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
/// Discord's IPC payloads are small; anything larger is a corrupt header.
pub const MAX_FRAME: usize = 64 * 1024;
/// How long to wait for a fresh socket before trying again.
const RECONNECT: Duration = Duration::from_secs(15);
/// Poll interval while connected, so a `Set` is applied promptly.
const POLL: Duration = Duration::from_millis(500);

/// The activity Rustcord shows on the Discord profile of the account signed in
/// to the local client. `started_unix` renders as elapsed time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub details: String,
    pub state: Option<String>,
    pub started_unix: i64,
}

/// What the UI thread asks the worker to do.
#[derive(Debug, Clone)]
enum Command {
    Set(Presence),
    Clear,
}

/// Handle held by the app. Dropping it stops the worker once it next wakes.
#[derive(Debug, Default)]
pub struct Rpc {
    client_id: Option<String>,
    tx: Option<Sender<Command>>,
}

impl Rpc {
    /// Apply the current settings. Enabling starts a worker; disabling or
    /// clearing the client id drops it, which makes Discord remove the activity
    /// when the socket closes.
    pub fn sync(&mut self, settings: &Settings, presence: Option<Presence>) {
        let wanted = settings.rpc_app_id();
        if !settings.discord_rpc || wanted.is_none() {
            self.tx = None;
            self.client_id = None;
            return;
        }

        let client_id = wanted.unwrap_or_default();
        if self.tx.is_none() || self.client_id.as_deref() != Some(client_id.as_str()) {
            self.tx = Some(spawn(client_id.clone()));
            self.client_id = Some(client_id);
        }

        if let Some(tx) = &self.tx {
            let command = match presence {
                Some(presence) => Command::Set(presence),
                None => Command::Clear,
            };
            let _ = tx.send(command);
        }
    }

    /// True when a worker is meant to be running; used by diagnostics and tests.
    pub fn active(&self) -> bool {
        self.tx.is_some()
    }

    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
}

fn spawn(client_id: String) -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("discord-rpc".to_string())
        .spawn(move || worker(rx, client_id));
    if let Err(err) = spawned {
        log::warn!("could not start the Discord RPC thread: {err}");
    }
    tx
}

fn worker(rx: Receiver<Command>, client_id: String) {
    let mut last: Option<Presence> = None;
    let mut warned = false;

    loop {
        let mut stream = match connect() {
            Some((stream, path)) => {
                log::info!("discord rpc: connected to {}", path.display());
                stream
            }
            None => {
                if !warned {
                    // Once per worker: a missing client is normal when Discord
                    // is not running, and this thread keeps retrying quietly.
                    log::info!("discord rpc: no Discord IPC socket found; will keep checking");
                    warned = true;
                }
                if !wait_offline(&rx, &mut last) {
                    return;
                }
                continue;
            }
        };

        if !handshake(&mut stream, &client_id) {
            // Bad or revoked client id: park the same way as a missing socket so
            // a corrected id in settings is picked up without a restart.
            if !wait_offline(&rx, &mut last) {
                return;
            }
            continue;
        }

        if let Some(presence) = &last {
            if !send_activity(&mut stream, presence) {
                continue;
            }
        }

        // Connected and caught up: apply changes as they arrive.
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Command::Set(presence)) => {
                    last = Some(presence.clone());
                    if !send_activity(&mut stream, &presence) {
                        break;
                    }
                }
                Ok(Command::Clear) => {
                    last = None;
                    if !send_clear(&mut stream) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if stream_closed(&mut stream) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Sleep while there is no connection, still absorbing presence changes so the
/// replay after reconnecting is current. Returns false when the app dropped us.
fn wait_offline(rx: &Receiver<Command>, last: &mut Option<Presence>) -> bool {
    match rx.recv_timeout(RECONNECT) {
        Ok(Command::Set(presence)) => {
            *last = Some(presence);
            true
        }
        Ok(Command::Clear) => {
            *last = None;
            true
        }
        Err(RecvTimeoutError::Timeout) => true,
        Err(RecvTimeoutError::Disconnected) => false,
    }
}

/// First socket that accepts a connection, in preference order.
fn connect() -> Option<(UnixStream, PathBuf)> {
    for path in socket_candidates(
        std::env::var_os("XDG_RUNTIME_DIR")
            .as_deref()
            .map(Path::new),
    ) {
        if !path.exists() {
            continue;
        }
        if let Ok(stream) = UnixStream::connect(&path) {
            return Some((stream, path));
        }
    }
    None
}

/// Discord's socket, native and inside the Flatpak/Snap sandboxes, in the order
/// it makes sense to try them.
pub fn socket_candidates(runtime_dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(runtime) = runtime_dir else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for sandbox in [
        "app/com.discordapp.Discord",
        "app/com.discordapp.DiscordCanary",
        "app/dev.vencord.Vesktop",
        "snap.discord",
        "snap.discord-canary",
    ] {
        for index in 0..10 {
            out.push(runtime.join(sandbox).join(format!("discord-ipc-{index}")));
        }
    }
    for index in 0..10 {
        out.push(runtime.join(format!("discord-ipc-{index}")));
    }
    out
}

fn handshake(stream: &mut UnixStream, client_id: &str) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let bytes = frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }));
    if stream.write_all(&bytes).is_err() {
        return false;
    }
    let Some(reply) = read_frame(stream) else {
        log::warn!("discord rpc: the client did not answer the handshake");
        return false;
    };

    match reply.get("evt").and_then(Value::as_str) {
        Some("READY") => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            true
        }
        _ => {
            let code = reply
                .pointer("/data/code")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let message = reply
                .pointer("/data/message")
                .and_then(Value::as_str)
                .unwrap_or("no reason given");
            log::warn!("discord rpc: handshake refused ({code}): {message}");
            false
        }
    }
}

fn send_activity(stream: &mut UnixStream, presence: &Presence) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let pid = std::process::id();
    let bytes = frame(OP_FRAME, &activity_command(presence, pid));
    stream.write_all(&bytes).is_ok()
}

fn send_clear(stream: &mut UnixStream) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let pid = std::process::id();
    let body = json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": Value::Null },
        "nonce": next_nonce(),
    });
    stream.write_all(&frame(OP_FRAME, &body)).is_ok()
}

/// Read whatever the client sent without blocking; `Err`/`UnexpectedEof` on the
/// socket means the connection is gone.
fn stream_closed(stream: &mut UnixStream) -> bool {
    let mut header = [0u8; 8];
    match stream.read_exact(&mut header) {
        Ok(()) => {
            // Drain the body of frames we do not act on (pings, dispatch).
            if let Some((_, length)) = frame_header(header) {
                let mut body = vec![0u8; length as usize];
                let _ = stream.read_exact(&mut body);
            }
            false
        }
        Err(err) => !matches!(
            err.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

fn read_frame(stream: &mut UnixStream) -> Option<Value> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).ok()?;
    let (_, length) = frame_header(header)?;
    let mut body = vec![0u8; length as usize];
    stream.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// One IPC message: little-endian opcode and length, then the JSON body.
pub fn frame(op: u32, body: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(body).unwrap_or_default();
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Validate an 8-byte header; `None` means the stream is not the IPC protocol.
pub fn frame_header(bytes: [u8; 8]) -> Option<(u32, u32)> {
    let op = u32::from_le_bytes(bytes[..4].try_into().ok()?);
    let length = u32::from_le_bytes(bytes[4..].try_into().ok()?);
    if length as usize > MAX_FRAME || op > 4 {
        return None;
    }
    Some((op, length))
}

/// `SET_ACTIVITY` for a presence. Discord ignores text shorter than two
/// characters, so `presence_field` drops it rather than sending an empty line.
pub fn activity_command(presence: &Presence, pid: u32) -> Value {
    let mut activity = serde_json::Map::new();
    if let Some(details) = presence_field(&presence.details) {
        activity.insert("details".to_string(), Value::String(details));
    }
    if let Some(state) = presence.state.as_deref().and_then(presence_field) {
        activity.insert("state".to_string(), Value::String(state));
    }
    if presence.started_unix > 0 {
        activity.insert(
            "timestamps".to_string(),
            json!({ "start": presence.started_unix.saturating_mul(1000) }),
        );
    }
    activity.insert("instance".to_string(), Value::Bool(false));

    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": Value::Object(activity) },
        "nonce": next_nonce(),
    })
}

/// Discord's fields are 2–128 characters; anything else is dropped.
pub fn presence_field(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.chars().count() < 2 {
        return None;
    }
    Some(trimmed.chars().take(128).collect())
}

fn next_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos}-{value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_an_opcode_and_length() {
        let bytes = frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY" }));
        assert_eq!(&bytes[..4], &1u32.to_le_bytes());
        let length = u32::from_le_bytes(bytes[4..8].try_into().expect("length"));
        assert_eq!(length as usize, bytes.len() - 8);
        assert_eq!(
            frame_header(bytes[..8].try_into().expect("header")),
            Some((1, length))
        );
        // The body is exactly the JSON that was framed.
        let body: Value = serde_json::from_slice(&bytes[8..]).expect("json");
        assert_eq!(body["cmd"], "SET_ACTIVITY");
    }

    #[test]
    fn corrupt_headers_are_refused() {
        assert_eq!(frame_header([0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]), None);
        assert_eq!(frame_header([9, 0, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(frame_header([0, 0, 0, 0, 0, 0, 0, 0]), Some((0, 0)));
    }

    #[test]
    fn sandboxed_clients_are_tried_before_the_bare_socket() {
        let paths = socket_candidates(Some(Path::new("/run/user/1000")));
        assert!(paths[0].ends_with("app/com.discordapp.Discord/discord-ipc-0"));
        assert!(paths
            .iter()
            .any(|path| path.ends_with("snap.discord/discord-ipc-0")));
        assert_eq!(
            paths.last().expect("last"),
            &PathBuf::from("/run/user/1000/discord-ipc-9")
        );
        assert!(socket_candidates(None).is_empty());
    }

    #[test]
    fn activity_keeps_short_fields_out_and_has_a_start_time() {
        let presence = Presence {
            details: "Rustcord".to_string(),
            state: Some("#".to_string()),
            started_unix: 1_700_000_000,
        };
        let command = activity_command(&presence, 42);
        assert_eq!(command["cmd"], "SET_ACTIVITY");
        assert_eq!(command["args"]["pid"], 42);
        assert_eq!(command["args"]["activity"]["details"], "Rustcord");
        // A one-character state is not sent at all.
        assert!(command["args"]["activity"].get("state").is_none());
        assert_eq!(
            command["args"]["activity"]["timestamps"]["start"],
            1_700_000_000_000i64
        );
        assert_eq!(command["args"]["activity"]["instance"], false);
        assert!(command["nonce"]
            .as_str()
            .is_some_and(|nonce| !nonce.is_empty()));
    }

    #[test]
    fn presence_text_is_trimmed_and_capped() {
        assert_eq!(presence_field("  ok "), Some("ok".to_string()));
        assert_eq!(presence_field("x"), None);
        assert_eq!(presence_field("   "), None);
        let long = "a".repeat(400);
        assert_eq!(
            presence_field(&long).map(|text| text.chars().count()),
            Some(128)
        );
    }

    #[test]
    fn a_disabled_setting_never_starts_a_worker() {
        let mut settings = Settings::default();
        let mut rpc = Rpc::default();
        rpc.sync(&settings, None);
        assert!(!rpc.active(), "off by default");

        settings.discord_rpc = true;
        rpc.sync(&settings, None);
        assert!(!rpc.active(), "a client id is required");

        settings.rpc_client_id = Some("123456789".to_string());
        rpc.sync(&settings, None);
        assert_eq!(rpc.client_id(), Some("123456789"));
        assert!(rpc.active());

        settings.discord_rpc = false;
        rpc.sync(&settings, None);
        assert!(!rpc.active());
    }
}
