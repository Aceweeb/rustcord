# CURRENT_STATE.md

Last verified: 2026-11-12, CachyOS, NVIDIA (proprietary 620.70.06), Wayland session.

## Build and test

| Check | Command | Result |
| --- | --- | --- |
| Tests | `cargo test --all-targets` | **169 passed (164 lib + 5 bin), 0 failed** |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --all` | clean |
| Release build | `cargo build --release` | success |

## Measured runtime figures (not estimates)

Release binary, 1280×800 window, default settings, theme file loaded.

| Metric | Before | After | How measured |
| --- | --- | --- | --- |
| RSS | 190–195 MB | **159 MB** | `/proc/<pid>/status` `VmRSS` |
| PSS | 64.6 MB | **57–61 MB** | `/proc/<pid>/smaps_rollup`; moves with other GL clients |
| Peak (`VmHWM`) | 196 MB | **160 MB** | kernel high-water mark |
| Idle CPU | 3–8% after ~10 s (repaint loop) | **0.0% (0.2 fps)** | ticks/s; frames at debug |
| Threads | 2 | 2 | `/proc/<pid>/status` |
| Sign-in screen | — | **104–141 MB / 45–58 MB PSS** | varies with driver page residency |

Reproduce with `scripts/measure-memory.sh 20`. The script prints RSS, PSS, peak,
CPU ticks and thread count per second.

## Why RSS was 190 MB (investigation outcome)

The old README claimed ~134 MB. It was **not** a code regression:

1. I built commit `227706e` (the pre-change baseline) in a separate worktree and
   measured it identically: **190.1 MB RSS / 64.7 MB PSS**. The current code
   measured **189.9 MB / 64.6 MB**. The two builds are indistinguishable, so the
   earlier 134 MB figure came from a different session/driver state, not from
   added features.
2. Per-library RSS showed the real cause — shared GPU driver mappings:

   | Mapping | RSS |
   | --- | --- |
   | `libnvidia-gpucomp.so` | 79,992 kB |
   | `libLLVM.so.23.1` (Mesa) | 25,316 kB |
   | `libnvidia-eglcore.so` | 21,140 kB |
   | application binary | ~8,000 kB |
   | `[anon]` + `[heap]` | ~13,000 kB |
   | fonts (wqy-zenhei, NotoColorEmoji, GoogleSansFlex) | ~8,000 kB |

   The app's own allocations were never more than ~23 MB anonymous.

3. **Fix:** `main.rs` now pins libglvnd to the NVIDIA EGL vendor
   (`__EGL_VENDOR_LIBRARY_FILENAMES`) when an NVIDIA device and vendor file are
   present and the user has not set the variable. Mesa is then never
   initialised, removing `libLLVM` and the gallium driver: **−33 MB RSS,
   −10 MB PSS**, and `libLLVM` is confirmed absent from `/proc/<pid>/maps`.

The remaining ~100 MB of RSS is the proprietary NVIDIA GL stack, which is shared
with other GL clients and is not reducible from application code. PSS is the fair
per-process figure; RSS overstates a GL app's cost because it counts shared
library pages in full.

## Repaint loop found and fixed

While measuring the new message list, a 20 s sample showed CPU jumping to 3–8%
of a core around t≈11 s and staying there, with PSS climbing. Cause: the app
re-applied the palette every frame, and `set_visuals` calls `request_repaint`, so
each frame scheduled the next one.

Fix: hash the palette and call `set_visuals` only when it changes
(`RustcordApp::apply_theme`, `DECISIONS.md` D12). The 20 s sample is now flat at
0.0% CPU, with a single scheduler tick at the 5 s telemetry wake-up.

A second CPU spike appeared after the image loader landed and turned out to be
measurement, not code: the binary was sampled in the seconds right after a
rebuild (driver and font warm-up), and later samples caught 5-second bursts of
~49 fps while the pointer sat over the window. `/proc/<pid>/schedstat` showed
0.02% of a core and the frame counter showed 0.2 fps at idle, so the loader is
innocent. See `DECISIONS.md` D14 — idle CPU is only meaningful next to a frame
count.

## Gateway reliability

`src/api/backoff.rs` (D19), wired into `src/api/gateway.rs`:

- At most **10 identifies an hour**, sliding; a spent budget waits **before**
  sending anything, and the wait is not counted as a failed attempt.
- A Resume costs no allowance.
- Reconnect delays are jittered to 50–100 % of the step, never zero.
- The backoff ladder resets only after a session has stayed up 60 s, so a
  handshake that immediately dies keeps backing off instead of hammering.
- Monotonic clock only; a system clock change cannot refill or empty the budget.

11 tests: the limit and its boundary, wait arithmetic, sliding expiry, reset,
 jitter bounds (including out-of-range rolls and a zero delay), the 60 s gate, and
the RNG staying in range. The socket loop itself is still only reachable with a
real token, so the wiring is compile-checked and review-checked, not live-tested.

## Discord identity login (OAuth2 `identify`)

A second, optional login beside the bot token, for showing who you are
(`DECISIONS.md` D18). It is **not** a messaging login and the UI says so.

- Flow: `state` + PKCE S256 → browser → loopback `127.0.0.1:53134/callback` →
  code exchange without a client secret → `GET /users/@me`.
- Displays: display name, `@username`, user id, and an initial-letter avatar. No
  avatar download is attempted, so no CDN request is made for it.
- Storage: keyring entry `discord_oauth_identity`, separate from the bot token;
  only the public client ID is ever in `settings.json`.
- Session restoration is automatic at start-up; an expired session stays visible
  and is flagged. Sign out and switch account are both available on the sign-in
  page and in Settings → Privacy.
- Works without a keyring? The bot token path already needed one; identity needs
  the same backend, and a keyring failure is reported rather than crashing.
- 18 tests: PKCE RFC vector, base64url, authorise URL shape, state mismatch,
  missing/empty code, denial, path mismatch, percent-decoding, token parsing,
  identity parsing, store round trip, expiry, restoration.

Not available: refresh tokens, other scopes, avatar images, and any messaging
with a personal account — Discord does not offer that to third-party clients.

## Message lifecycle

Edits, deletes, reactions and typing arrive over the gateway and mutate the same
list the virtual view renders (`DECISIONS.md` D17):

- `MESSAGE_UPDATE` — partial payloads never clear a field; unknown ids are
  ignored, so an out-of-order edit cannot add a row.
- `MESSAGE_DELETE` — matched by id and channel when given; replayed deletes are
  no-ops.
- `MESSAGE_REACTION_ADD/REMOVE` — chips track a capped user list so replays cannot
double-count; `MESSAGE_REACTION_REMOVE_ALL` and `_REMOVE_EMOJI` clear reactions
  through one path.
- `TYPING_START` — per-channel tracker, 8 s expiry, 32-typer cap, shown above the
  composer with a bounded 500 ms repaint while active. No timer, no thread.

Covered by 25 tests, all with synthetic payloads; no token required. Verified:
every event, unicode and custom emoji, missing fields, wrong channel, unknown id,
replayed/malformed payloads, multiple typers and expiry.

## REST rate limits

Every REST call now goes through `api/ratelimit.rs` (`DECISIONS.md` D16):
requests are keyed by route plus the channel/guild they target, `X-RateLimit-*`
is recorded per bucket, a zeroed bucket waits for its window, `429` prefers the
body's `retry_after`, global limits pause everything, and one retry is allowed.
A wait over 60 s is returned as an error instead of stalling the UI thread's
task.

Covered by 11 pure tests (routing, per-channel isolation, expiry, global and
local 429s, malformed headers, body parsing). **Not yet exercised against a real
`429`** — that needs live traffic with a valid token.

## Sign-in

The app opens on a sign-in page when the keyring holds no token (`ui/login.rs`,
`DECISIONS.md` D15). It accepts a **bot token only** — shape-checked before any
request, stored in the keyring, then the gateway starts. "Continue in demo mode"
skips it; **Settings → Privacy → Sign out** clears the keyring, session and
caches and brings the page back. An install that already has a token opens in
the workspace, so the page never interrupts an existing session.

## Attachments and images

Message attachments and embeds render inline through a byte-bounded image cache
(`src/images.rs`):

- Budget is `Settings::image_cache_mb` (default 48 MB, range 8–512 MB); eviction
  is least-recently-used and accounted at 4 bytes per pixel.
- Only rows the virtualised list drew are fetched, so scrolling is what triggers
  a download.
- URLs must be `https://` on Discord's CDN; downloads are capped at 12 MB and
  4096 px, and failures are not retried.
- Decoding happens on a worker thread spawned by the first request; placeholders
  use Discord's reported dimensions so rows do not jump when a texture lands.
- The Memory settings page shows live cache usage and a budget slider.

## Coverage

- Unit-tested: `/proc` parsing, CPU percentage math, history eviction, settings
  roundtrip + legacy migration, shortcut parsing/matching/conflicts, all 12 theme
  presets' WCAG contrast, CSS variable extraction, diagnostics report rendering,
  log ring eviction, gateway payload parsing, EGL vendor selection policy,
  history page parsing (order, reactions, malformed bodies) and row
  virtualisation (viewport ranges, clamping, 10k-row cost).
- Not tested automatically: the GUI itself (no UI test harness), live Discord
  connectivity (needs a real bot token), keyring storage (no Secret Service
  provider was assumed, and none was used).

## Known issues

1. Idle RSS above the historical 134 MB figure, but the excess is the GPU driver;
   PSS at 54 MB is lower than the old RSS-based claim implies.
2. Gateway has only been exercised against synthetic payloads.
3. `Settings` uses JSON rather than SQLite (see `DECISIONS.md` D3).
4. Voice/video and streaming capture remain unimplemented; the UI shows plugin
   state only.
5. WASM plugins fail closed by design.

## Environment quirks

- Project lives at `/mnt/storage/Default Project/rustcord` because `/` filled up
  (497 GB, 4.7 GB free). A full root filesystem breaks Cargo builds and command
  execution.
- `CARGO_HOME` is still on `/` and is the next space risk.
- Mesa is deliberately not initialised; if a future environment needs it,
  unset `__EGL_VENDOR_LIBRARY_FILENAMES` before launching.
