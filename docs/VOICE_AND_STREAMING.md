# Voice, Go Live and the DAVE gate

Status: **research complete, implementation deliberately not started.** This note
records why priority-roadmap items 6 (streaming/screen share) and 7 (voice) are
blocked for this client, what a compliant implementation actually requires, and
the order in which it should be built if that changes. It exists so the decision
is auditable rather than "not done".

## The gate: DAVE is mandatory for third-party voice

Since September 2024 Discord has been migrating voice, video and Go Live to
end-to-end encryption with the **DAVE** protocol, and since **1 March 2026** it
supports only E2EE media sessions. Discord's own support article says third-party
applications and bots that connect to voice must implement DAVE. The voice gateway
enforces this: close code `4017` is "E2EE/DAVE protocol required", and the current
voice documentation includes `max_dave_protocol_version` in the Identify payload
and DAVE transition opcodes 21–24.

DAVE is not an optional cipher swap. It needs:

- an **MLS** (RFC 9420) group per media session, with Discord's voice gateway as
  the delivery/authentication service;
- a DAVE-capable identify, epoch proposals/commits, and transition handling;
- frame-level encryption *inside* the transport-encrypted RTP payload (so Opus
  frames are encrypted before packetisation), plus per-sender key derivation.

Discord publishes the protocol whitepaper (`github.com/discord/dave-protocol`) and
an open-source C++ implementation, **libdave**, which is what Discord points
implementers at. A from-scratch Rust MLS + DAVE implementation is a project of its
own; binding libdave adds a C++ build dependency and a licence/review surface that
this MIT/Apache-2.0 client has not evaluated.

**Consequence:** any voice or Go Live implementation in Rustcord today would join
a channel, receive `4017`, and disconnect. Shipping capture UI, mute buttons and a
"Join" affordance on top of that would be the exact half-implementation the
project's rules forbid (`DECISIONS.md` D21). The existing voice *presence* feature
(who is connected, from `VOICE_STATE_UPDATE`) stays — it needs no voice connection.

## What a compliant path would take

1. **Signalling.** Main-gateway `op 4 Voice State Update` →
   `VOICE_STATE_UPDATE` + `VOICE_SERVER_UPDATE` → voice WebSocket
   (`wss://{endpoint}/?v=…`), Identify (`server_id`, `user_id`, `session_id`,
   token), Ready (SSRC, IP, port, modes), UDP IP discovery, Select Protocol,
   Session Description (secret key). Heartbeats and `Speaking` (op 5) follow.
2. **Transport.** RTP framing with one of the advertised AEAD modes
   (`aead_aes256_gcm_rtpsize` preferred, then
   `aead_xchacha20_poly1305_rtpsize`, then `xsalsa20_poly1305_lite_rtpsize`),
   sequence/timestamp/SSRC handling, jitter buffer, and opus 48 kHz stereo.
3. **DAVE.** MLS group join, key package publication, epoch transitions
   (opcodes 21–24), frame encrypt/decrypt, and handling members joining/leaving
   mid-call.
4. **Capture and playback (Linux).** PipeWire (or a portal) capture, Opus encode;
   PipeWire/Pulse playback, Opus decode, per-user volume, mute/deafen, and a
   device picker. Rust options: `cpal` (playback/capture via ALSA/Pulse) and
   `pipewire-rs`; Opus via `audiopus`/`opus` bindings.
5. **Go Live (item 6).** The same voice session plus H.264/VP8 video over the same
   RTP path, quality/bitrate negotiation, and viewer state. Everything in 1–4 is a
   prerequisite; there is no separate, simpler streaming API.

## Why not implement 1–3 now

- **No way to verify.** This development environment has no working bot session
  (`DECISIONS.md` D28), and voice requires the bot to be in a guild voice channel
  with at least one other member to negotiate DAVE. Code that cannot be run once
  would be untested protocol code — the highest-risk kind to guess at.
- **DAVE compliance is the long pole**, not UI or audio I/O. Without it the rest is
  dead weight.
- **Licence/build review** for libdave (C++, CMake, OpenSSL) has not been done for
  a project that currently builds with `cargo build --release` alone.

## Recommended order if this is picked up

1. Spike libdave behind an optional Cargo feature; prove one join/leave against a
   real voice channel with two accounts before writing UI.
2. Add signalling + transport (2 above) with synthetic-payload tests, mirroring the
   gateway tests already in `src/api`.
3. Add DAVE, then Opus and a jitter buffer, then capture/playback, then UI.
4. Go Live last: it reuses the verified media path.

Until step 1 succeeds, voice audio and screen sharing remain **blocked on DAVE**,
and the roadmap item should stay unchecked with this note as its reason.
