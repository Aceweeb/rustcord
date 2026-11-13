# CURRENT_STATE.md

Last verified: 2026-11-13, CachyOS, NVIDIA (proprietary 620.70.06), Wayland session.

## Build and test

| Check | Command | Result |
| --- | --- | --- |
| Tests | `cargo test --all-targets` | **343 passed (338 lib + 5 bin), 0 failed** |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --all` | clean |
| Release build | `cargo build --release` | success |

## Measured runtime figures (not estimates)

Release binary, 1280×800 window, default settings, theme file loaded.

| Metric | Latest | How measured |
| --- | --- | --- |
| RSS | **211 MB** | `/proc/<pid>/status` `VmRSS`, 20 s sample |
| PSS | **164 MB** | `/proc/<pid>/smaps_rollup`; moves with other GL clients |
| Peak (`VmHWM`) | **250 MB** | kernel high-water mark |
| CPU | **0.0% without animation; nonzero while animated surfaces are visible** | ticks/s; 12.5 fps redraws; guild icons stop when unfocused |
| Threads | **7–11** | `/proc/<pid>/status` |
| Sign-in screen | lower than a channel session | no channel surface is built |

The 300 MB ceiling holds with about 50 MB of headroom. The earlier 159 MB figure
predates animated guild icons, the roster-backed member panel, and periodic count
refreshes; re-measure after any surface that adds textures or caches.

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

## Look and feel

Default preset **Discord** (`#313338` chat, `#2b2d31` sidebars, `#1e1f22` rail,
`#5865f2` blurple, D23), with Discord's measurements as constants in
`src/ui/sidebar.rs`: 72 px rail, 240 px channel list, 48 px guild header,
48 px guild icons, 34 px channel rows, 240 px member list, 40 px message avatars.

Guild icons are circles that become rounded squares when active, with the white
selection pill; channel rows are painted (8 px inset, 4 px radius, hover and
selected fills derived from the surface) so they hit 34 px exactly. Type scale is
Discord's: 15 px names/body, 11–12 px meta, 16 px channel header.

The other twelve presets and CSS theme files still work and still win when
chosen; the layout improvements apply to all of them.

## Local-only guarantees

`src/privacy.rs` (D22) holds the one exact-match allowlist and it is enforced at
every network entry point: `api::discord`'s `request`, the gateway socket
(including a `resume_gateway_url` taken from a payload), and image downloads
(which keep their own CDN list, asserted to agree). Cleartext and look-alike
hosts are refused.

No database: messages, textures and the log ring are memory-only, bounded by the
resource profile. Files: `settings.json` (preferences) and two keyring entries.
No telemetry: no analytics crate in `Cargo.lock`, no analytics endpoint in the
release binary.

Observed evidence: with no token configured the process holds **no sockets**, and
a six-second run creates **neither** the config nor the data directory. `Settings
→ Privacy` lists every store and host; `scripts/check-local.sh` reproduces the
static checks.

## Login page

Four blocks, in the order a new user needs them (`src/ui/login.rs`):

1. **Bot token** — shape-checked, keyring-stored, demo mode available.
2. **Add the bot to a server** — the invite URL for the same application, with
   exactly the six permissions the client uses (117824), copyable and openable.
   A bot in no server can read nothing, so this is the step that makes a token
   useful.
3. **Discord identity** — optional OAuth2 `identify` + PKCE, its own keyring
   entry, refresh on expiry. Read-only by design.
4. **Help** — where to create the application.

There is deliberately no password field and no user-token path: see
`DECISIONS.md` D21 for Discord's documented position (self-bots are forbidden and
can end an account; `messages.read` is RPC-only; user DMs/friends need the
approval-gated Social SDK).

## Desktop install

`scripts/install-local.sh` builds and installs for the current user:
`~/.local/bin/rustcord`, `~/.local/share/applications/rustcord.desktop` (absolute
`Exec`, `StartupWMClass=rustcord`) and `~/.local/share/icons/hicolor/scalable/apps/rustcord.svg`.
Re-running it updates all three, which is the update path for a launcher install.

Verified in this session: `desktop-file-validate` passes, the entry starts the
binary from `$HOME` (where a launcher starts it), the window reports
`class: Rustcord` / `title: Rustcord` in `hyprctl clients`, the process stays up,
and the SVG renders (rsvg-convert, 64×64 RGBA).

## Message search (local)

Ctrl+F opens an overlay that searches the **loaded history of the open channel**
(D20). `src/search.rs` is pure: terms ANDed case-insensitively, `from:name` or
`@name` filters to an author, newest-first, 50 results maximum. Snippets are
single-line and char-safe around multi-byte content. Results are snapshots, so a
message deleted between search and click cannot be indexed into; clicking a
result opens its channel.

The overlay says it searches loaded messages only: Discord's guild-search
endpoint requires a user token, which this client refuses to hold. There is no
index and no per-frame scan — results refresh only while the overlay is open and
something marked it stale.

12 tests cover parsing, the author filter, AND semantics, ordering and the
limit clamp, empty queries, snippet windowing, newline flattening, multi-byte
content and the jump payload.

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
- An expired session with a refresh token recovers on its own: one refresh
  exchange on a worker thread, identity re-read from `/users/@me`, the result
  written back to the keyring. A rotated refresh token is stored; a response
  without one keeps the old token rather than signing the user out.
- Works without a keyring? The bot token path already needed one; identity needs
  the same backend, and a keyring failure is reported rather than crashing.
- 21 tests: refresh merging and the no-refresh-token path, PKCE RFC vector, base64url, authorise URL shape, state mismatch,
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

## Member panel roles

The member panel follows Discord's hoisted-role layout. Guild roles arrive in
`GUILD_CREATE`/READY and are refreshed by `GET /guilds/{id}/roles` alongside the
roster fetch; every member records its role ids from `GET /guilds/{id}/members`.
The highest-position hoisted role becomes the member's section, role sections
are ordered by descending position, and a role-colour dot precedes the header.
Members without a hoisted role fall back to ONLINE/OFFLINE by presence.

REST members carry no live presence, so they arrive offline and would otherwise
never appear in a role section; role sections therefore include them and their
status dot still shows the absence. Non-hoisted roles are not sections, matching
Discord. `GUILD_ROLE_CREATE/UPDATE/DELETE` are not handled yet, so a role change
appears after the next roster load or reconnect.

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

The composer can send one file at a time through the paperclip's native
`rfd` dialog (XDG Desktop Portal on this desktop). Files are checked to be
regular and at most 10 MiB once on selection and again on upload, shown as a
removable chip, and sent as `payload_json` + `files[0]` multipart with the
same caption/reply path as text. The emoji button opens a scrollable grid of
the built-in Unicode set and appends the chosen glyph to the draft; custom
guild emoji are not listed yet.

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
6. The composer's native file dialog, emoji popup and live multipart upload
   have not been exercised end-to-end here; the multipart form needs a real bot
   session to prove Discord accepts it.
7. Role sections are refreshed with the roster, not on `GUILD_ROLE_*` gateway
   events; role changes wait for a reconnect or a fresh roster load.

## Environment quirks

- Project lives at `/mnt/storage/Default Project/rustcord` because `/` filled up
  (497 GB, 4.7 GB free). A full root filesystem breaks Cargo builds and command
  execution.
- `CARGO_HOME` is still on `/` and is the next space risk.
- Mesa is deliberately not initialised; if a future environment needs it,
  unset `__EGL_VENDOR_LIBRARY_FILENAMES` before launching.
