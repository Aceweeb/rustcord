# Rustcord — Progress / Handoff Notes

> **Partly superseded.** The project now maintains `PROJECT.md`,
> `ARCHITECTURE.md`, `TASKS.md`, `CURRENT_STATE.md`, `DECISIONS.md` and
> `HANDOFF.md`. Read `CURRENT_STATE.md` for verified numbers and `HANDOFF.md`
> for the next task; sections 1–3 below describe the earlier scaffold.
> The project also moved to `/mnt/storage/Default Project/rustcord`.

Last updated: 2026-11-12 (Europe/Berlin)

Latest increment: a colour per folder, assigned from a palette and editable from
the folder menu (D38); the server peek's channels are clickable (D37); a native message log of edits and deletions (D36); a
hover peek of a server's channels beside the rail (D35);
folder renaming from the folder menu (D34); keyboard-only navigation with an
in-place focus ring (D33);
guild folders and drag-free rail ordering persisted locally (D32);
a Ctrl+K quick switcher over servers and channels (D31); local
unread and mention badges on channels and guild icons (D30); guild member roster over the Members intent and REST (D29); load failures
are visible in the channel (D28 addendum); the guild rail is populated from READY
(D27). Reaction chips toggle (D26), on top of the right-click menu
(D25), message actions and voice presence (D24).
`HANDOFF.md` has the file-by-file detail; voice **audio** remains deliberately
unimplemented.

Project path: `/home/jason/Documents/Default Project/rustcord`

This file is the checkpoint for the next chat/session. Read this first, then
`README.md`.

---

## 0. Goal and original prompt

### Goal

Build a lightweight, modern Discord client rewrite in Rust that reproduces the
Discord desktop UI (guild rail, channel list, messages, voice/video), supports
Vencord/BetterDiscord-style themes and plugin hooks, stays under the resource
budget (< 150 MB idle, < 5% CPU on standby), builds cleanly on Arch Linux, and
ships as MIT/Apache-2.0 with README, CONTRIBUTING, and a runnable scaffold.
Pure `egui` was chosen over Tauri because Tauri pulls in WebView2/Chromium on
Windows, which the constraints forbid.

Later user additions:
- streaming / GoLive UI and indicators
- "translate" Vencord themes into a native theme layer
- Spotify now-playing integration
- email/password login (declined; see section 3)

### Original prompt (preserved for context)

<details>
<summary>Expand the original request</summary>

> You are a senior systems engineer and Rust expert tasked with building a
> lightweight, modern Discord client rewrite. The goal is to replicate the full
> Discord desktop UI (channels, server list, message viewer, voice/video
> integration) while prioritizing low system resource usage (target < 150MB RAM
> idle, < 5% CPU on standby). The application must be cross-platform (Windows,
> macOS, Linux) and built with Rust + Tauri (or pure egui/winit if preferred).
>
> **Core Requirements:**
>
> - **UI Faithfulness:** Replicate the Discord: dark mode, compact/expanded
>   channel sidebar, message bubbles, emoji reactions, image/video previews,
>   voice channel activity dots, and notification popovers. Use native OS
>   styling where possible.
> - **Vencord & Theme Integration:** Implement a theming system that supports
>   custom CSS-like injection (compatible with Vencord/BetterDiscord theme
>   formats). Provide a plugin hook API that allows users to inject React
>   components or HTML/CSS snippets into the UI (e.g., custom embedders,
>   mini-mods, theme toggles). The plugin system must load .js/.ts or compiled
>   WebAssembly plugins securely, with a whitelist/permissions model.
> - **Low Resource Usage:**
>   - Use Tauri's minimal footprint (no Electron) or optimized egui rendering.
>   - Implement lazy loading for message history (infinite scroll, virtualized
>     list).
>   - WebSocket-based messaging (no polling).
>   - Memory-efficient data structures (use serde for JSON, tokio for async,
>     once_cell for globals).
>   - Profile-guided optimization, compact string handling, and no-std where
>     feasible.
> - **Plugin Ecosystem:** Expose a Plugin trait with init(), on_message(),
>   on_react(), on_settings() hooks. Plugins can register custom commands, UI
>   overlays, or API modifications. Provide a default vencord-compat plugin
>   bundle that supports the most common Vencord theme injections and modifier
>   scripts.
> - **Authentication & API:** Use reqwest + serde for Discord API v10. Handle
>   OAuth2 login, gateway connection with sharding, intents (messages, guilds,
>   voice), and rate-limit handling. Include a local config manager (encrypted
>   storage via rpassword/keyring).
> - **Voice Support:** Basic voice endpoint wiring (may use siphash/codec stubs
>   or integrate high-score/rVoice abstractions). Focus on UI sync
>   (deaf/mute, disconnect, volume sliders).
>
> **Deliverables:**
>
> - Full Cargo.toml with dependencies optimized for size/release targets.
> - src/ directory structure: ui/, plugins/, api/, models/, utils/.
> - A working scaffold that launches a window with the Discord sidebar +
>   message area.
> - A "Hello, World!" plugin that adds a toggleable "Blyat" easter-egg banner
>   at the bottom of the channel list (configurable via settings.json).
> - Makefile or CMakeLists.txt for Arch Linux compilation with cargo release or
>   maturin flags for minimal binary size (--strip,
>   CARGO_PROFILE_RELEASEopt-level = "z").
> - Brief ARCHITECT'S_NOTE explaining trade-offs between Tauri vs. pure Rust
>   GUI, resource profiling results, and how the plugin system maintains Vencord
>   compatibility without loading unsafe native code.
>
> **Constraints:**
>
> - No Electron. Absolutely no webview2 or chromium embedding unless strictly
>   necessary for a fallback mode.
> - Must compile and run on Arch Linux with rustc 1.78+ and tauri 1.6+ (or
>   egui 0.24+).
> - Output must be MIT/Apache 2.0 licensed, with a CONTRIBUTING.md outline for
>   plugin contributors.
> - Include a README.md with "Getting Started", "Building for Arch", "Enabling
>   Vencord Themes", and "Safety/Permissions Model" sections.
> - Begin by generating the project skeleton, then iterate on the plugin API and
>   theming system. Ensure every code block is commented for a mid-senior Rust
>   developer to extend.
>
> **Quick Build/Arch Notes:**
>
> - `cargo build --release --target x86_64-unknown-linux-gnu` produces a
>   ~3–6MB binary (stripped).
> - Use `pacman -S rust cargo git` for Arch dependencies.
> - Tauri handles native OS integration (tray, file picking, SSL) via minimal
>   Rust/WASM, keeping RAM usage an order of magnitude below Electron.
> - Plugin system: plugins are loaded from a `./plugins/` directory as `.wasm`
>   files built with wasm-pack, with a `plugin.json` manifest declaring
>   permitted hooks (`read_messages`, `modify_embed`, `theme_injection`, etc.).
>   This avoids executing arbitrary JS and keeps the process sandbox-safe.
>
> **Streaming Support:** Native Discord GoLive/Streaming integration via the
> gateway (`voice_state` updates, `speaking` events, stream-key validation). UI
> elements for starting/stopping screen share, user list in voice channels, and
> streaming status display in the channel sidebar. Use Tauri's async IPC to
> bridge to native screen-capture APIs (macOS `CG`, Windows `Duplex`, Linux
> `pipewire/xrandr`).
>
> **Vencord "Translation" / Theme Compat Layer:**
> - A `vencord-compat` plugin that parses and applies the most common Vencord
>   theme properties (CSS variables, BetterDiscord injection points, `.theme`
>   file format).
> - Instead of loading raw JS, provide a `ThemeEngine` that maps Vencord CSS
>   selectors to the Rust/egui rendering pipeline.
> - Expose an `inject_theme(ssr: String)` hook that safely rewrites the
>   DOM-like egui state without executing arbitrary code.
> - Support the top 20 most-used Vencord modifiers (custom embedders, font
>   overrides, accent color swaps) via a theme-preset manifest system.
>
> **Spotify Integration:**
> - Official Spotify Web API bind (`spotify-cli`/`rusqlite` backend) for
>   `currently_playing`, `playback_progress`, and `user_playback_state`.
> - A `SpotifyPlugin` that implements the `Plugin` trait: `on_activity_update()`
>   posts the current track (title, album, album art) to the user's Discord
>   status via the Discord API `PUT /users/@me/settings`.
> - Click-to-play: clicking the status overlay opens the Spotify URI in the
>   system browser or launches the native Spotify client (via
>   `open`/`xdg-open`).
> - Rate-limit-aware polling (5-second intervals) with `reqwest` +
>   `serde_json`, fallback to `spctl` (Linux)/`osascript` (macOS) AppleScript if
>   the API gateway stalls.
>
> **Full-Stack Deliverables:**
> - `plugins/` folder with three pre-built plugins: (a) `vencord-compat`,
>   (b) `spotify-status`, (c) `streaming-indicator`.
> - `src/api/spotify.rs` and `src/api/discord.rs` modules with typed structs for
>   API payloads.
> - `settings.json` schema that toggles: `vencord_themes`, `spotify_status_sync`,
>   `streaming_notifications`, `resource_priority`.
> - Profiling notes: `valgrind --tool=massif`, `criterion.rs` benchmarks, and a
>   `Makefile` target `profile` that generates a `--release` binary with
>   `--timings-server` for CI.

</details>

---

## 1. Current status

| Check | Result |
| --- | --- |
| `cargo check` | passes |
| `cargo test` | 12 tests passing |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo build --release` | passes |
| Release binary | `target/release/rustcord`, ~14 MB stripped |
| Runtime | launches on Arch/Wayland; idle RSS ~134 MB; steady-state CPU ~0.0% |
| Git | not initialized in this directory |

The app is a working native egui client scaffold. It is **not** a complete
Discord replacement yet. Mock data is used until a bot token is configured and
a real guild arrives over the gateway.

---

## 2. What is implemented

### UI
- Native egui/eframe app, no Electron, no WebView2, no Chromium embedding.
- Guild rail, channel list, member list, message history, composer, settings
  window (`Ctrl+,`).
- Dark theme applied through `AppTheme::apply`.
- Message rendering filters by `Message::channel_id` so channels no longer
  share one history.

### Discord
- Typed REST client: `src/api/discord.rs`
  - gateway URL lookup
  - current user
  - send message
- Gateway transport: `src/api/gateway.rs`
  - TLS WebSocket (`tokio-tungstenite`, rustls native roots)
  - Hello / Identify / heartbeat loop
  - READY, GUILD_CREATE, MESSAGE_CREATE parsing
  - guild + channel sync (text, voice, category)
  - reconnect loop with exponential backoff (2s → 60s cap)
  - RESUME support via `session_id`, `resume_gateway_url`, and sequence
  - heartbeat ACK tracking; a missing ACK replaces the zombie socket
  - INVALID_SESSION discards the session and re-Identifies
  - dispatches `Reconnecting` / `Resumed` events to the UI instead of
    dropping the gateway handle
- Composer behavior:
  - demo mode: appends local mock messages
  - live mode: sends through the REST client when a text channel is selected
  - live voice channels cannot be used as message targets
- Bot token authentication:
  - `src/auth.rs`
  - stored in the OS Secret Service keyring under service `rustcord`,
    account `discord_bot_token`
  - never written to `settings.json`

### Plugins
- `Plugin` trait + concrete built-ins in `src/plugins/`:
  - `vencord-compat`
  - `spotify-status`
  - `streaming-indicator`
  - `blyat-banner`
- `load_wasm_plugin` is intentionally fail-closed. No sandbox is pretended.

### Themes
- `src/plugins/vencord_compat.rs` parses a safe subset of Vencord/BetterDiscord
  `.theme` files:
  - CSS custom properties only
  - backgrounds, text, accent, status colors
  - selectors / `@import` / arbitrary CSS ignored
- Sample theme: `themes/vencord-example.theme`.
- Theme path is configurable in `settings.json` or via the settings window.

### Settings
- `src/settings.rs`, persisted to `~/.config/rustcord/settings.json`.
- Local `settings.json` is a development fallback.
- Current keys:
  - `vencord_themes`
  - `spotify_status_sync`
  - `streaming_notifications`
  - `resource_priority`
  - `theme_file`
  - `show_blyat_banner`
- Credentials are **not** in this file.

---

## 3. Important decisions / constraints

- **No email/password login.** This was explicitly requested but not
  implemented. Reasons:
  - exposes raw account credentials to a third-party process
  - not the supported Discord third-party API model
  - violates Discord's terms
  - bot token / approved OAuth2 are the supported paths
- **Bot token + keyring** is the implemented auth path.
- **No Electron / WebView2 / Chromium.**
- **egui 0.37 API specifics** (important for the next agent):
  - `eframe::App` uses `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`
    (not `update(&mut self, ctx, frame)`).
  - Panels are `egui::Panel::left/right/top/bottom(...)`, not
    `SidePanel`/`TopBottomPanel`.
  - `CentralPanel::default().show(ui, ...)`.
  - `egui::Window::show(ctx, ...)` still takes a `Context`.
  - The app root is a `Ui`, and `ui.ctx()` gives the context.
- Release profile is size-oriented (`opt-level = "z"`, LTO, strip).
- Gateway runtime uses one Tokio worker thread to keep resource use low.
- The keyring dependency uses the `sync-secret-service` feature. Arch needs a
  Secret Service provider (e.g. `gnome-keyring`).

---

## 4. Verification commands

```bash
cd "/home/jason/Documents/Default Project/rustcord"

cargo fmt --all
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
./target/release/rustcord
```

Arch dependencies:

```bash
sudo pacman -S --needed base-devel rust cargo pkgconf \
  wayland wayland-protocols libxkbcommon \
  libx11 libxcursor libxi libxrandr libxinerama \
  mesa vulkan-icd-loader dbus gnome-keyring \
  valgrind
```

---

## 5. Known limitations / next steps

Priority order for the next session:

1. **Gateway resilience**
   - reconnect/backoff
   - RESUME support + sequence/session tracking
   - INVALID_SESSION handling
   - sharding (only if needed)
   - rate-limit queue for REST
2. **Real message history**
   - fetch channel history via REST (`GET /channels/{id}/messages`)
   - virtualized list for large histories
   - per-channel message cache
3. **Reactions / typing / edits / deletes**
   - `MESSAGE_UPDATE`, `MESSAGE_DELETE`, `MESSAGE_REACTION_ADD/REMOVE`
   - `TYPING_START`
4. **Attachments and embeds**
   - render image/video/embed payloads
   - upload path
5. **Spotify live polling**
   - OAuth2 PKCE or a manually supplied access token
   - poll `currently_playing` every ~5 s
   - update `SpotifyStatusPlugin` instead of the demo track
6. **Streaming / screen share** — **blocked on DAVE**, see
   `docs/VOICE_AND_STREAMING.md`; Go Live reuses the voice media path.
   - Linux capture via PipeWire/X11 (not started)
   - Discord GoLive gateway/voice state wiring (not started)
   - UI controls and viewer state (not started)
7. **Voice** — **blocked on DAVE** (mandatory since 1 March 2026); presence only.
   - Opus codec + UDP/RTP/DAVE or a library abstraction (not started)
   - mute/deafen/volume UI sync (not started)
8. **WASM plugin sandbox** — **done** (`DECISIONS.md` D46,
   `src/plugins/wasm.rs`): manifest + permissions, `wasmi`, fuel/memory/output
   limits, no host imports, fail-closed load and call, settings toggle, 15 tests.
9. **Secure OAuth2**
   - PKCE, local redirect, refresh token storage in keyring
10. **Packaging** — partially done
    - `.desktop` file and icon (done, verified WM class)
    - Arch `PKGBUILD` (done, `packaging/PKGBUILD`)
    - AppImage / Flatpak (open; needs a clean-VM build)

---

## 6. Recent change log

- Initial scaffold created: UI, mock data, settings, plugins, theme parser.
- Added measured resource notes to README.
- Added library target (`src/lib.rs`) so plugins can depend on `rustcord`.
- Added tests for theme parsing, color parsing, gateway payloads.
- Added Discord Gateway transport + unit tests.
- Added bot-token keyring storage (`src/auth.rs`).
- Added GUILD_CREATE parsing and live guild/channel sync.
- Added REST message sending in live mode.
- Added `Message::channel_id` and channel-filtered rendering.
- Removed `discord_token` from `settings.json` after moving credentials to the
  keyring.
- Added paginated channel history, a variable-height virtualised message list,
  inline attachments/embeds behind a byte-bounded image cache, and a first-run
  bot-token sign-in page.
- Added REST rate limiting (`src/api/ratelimit.rs`, 11 tests). The rules follow
  Discord's documented limits; the code is Rustcord's own, not Yapfast's
  (GPL-3.0 vs MIT/Apache). See `DECISIONS.md` D13–D16.
- Added the message lifecycle: `MESSAGE_UPDATE`, `MESSAGE_DELETE`,
  `MESSAGE_REACTION_ADD/REMOVE`, `MESSAGE_REACTION_REMOVE_ALL`,
  `MESSAGE_REACTION_REMOVE_EMOJI`, `TYPING_START`, plus pure list operations and
  a typing tracker in `models.rs`. 25 synthetic-payload tests, no token needed
  (`DECISIONS.md` D17). Test suite: 142.
- Added an optional OAuth2 `identify` login beside the bot token: PKCE S256,
  state validation before the code is read, loopback callback listener, keyring
  session separate from the bot token, restoration, logout and account
  switching. Identity only — it grants no messaging, and the UI says so. 18 tests
  (`DECISIONS.md` D18). Test suite: 160.
- Added gateway reliability (`src/api/backoff.rs`, D19): a sliding budget of ten
  identifies an hour that waits instead of reconnecting, jittered reconnect
  delays (50–100 %, never zero), and a backoff reset that only fires after a
  session has stayed up 60 s. 11 tests. Test suite: 169.
- Added local message search (`src/search.rs`, `src/ui/search.rs`, D20): Ctrl+F
  overlay, `from:`/`@` author filter, case-insensitive AND terms, newest-first,
  50 results, char-safe snippets, click to open the channel. No index, no
  per-frame scan, and the overlay states that Discord's server-side search needs
  a user token. 12 tests. Test suite: 180.
- Restyled the workspace to Discord's own measurements and colours (D23): new
  default **Discord** preset, 72/240/48/34 px rail, channel list, header/icons and
  rows, painted channel rows with derived hover/selected fills, 40 px message
  avatars, Discord type scale, 8 px window/menu rounding. All existing presets,
  CSS themes and features preserved. Test suite: 194.
- Made the local-only design enforceable and visible (D22, `src/privacy.rs`): a
  single exact-match host allowlist now guards every REST call, image fetch and
  the gateway socket, including resume URLs from payloads. Settings → Privacy
  lists the local stores and hosts; `scripts/check-local.sh` repeats the checks.
  Verified no configured-token run opens sockets or writes files. 7 tests. Test
  suite: 194.
- Added the bot install flow to the login page (D21): the invite URL is built
  from the configured client ID with only the permissions Rustcord uses, with
  open-in-browser and copy-link. 3 tests. Test suite: 187.
- Documented, with Discord's own sources, why there is no password/user-token
  login: self-bots are forbidden and can terminate an account, `messages.read`
  is RPC-only, and user-level DMs/friends need the approval-gated Social SDK.
- Added desktop integration: an SVG icon, `scripts/install-local.sh` and a
  corrected launcher entry. Installed to `~/.local/bin` + the hicolor icon theme
  and verified live (`hyprctl` shows `class: Rustcord`). Relative theme paths now
  resolve against the config and binary directories, not just the cwd, which a
  launcher does not share with the project. Test suite: 185.
- Added refresh-token handling to the OAuth2 identity session (D18 amendment):
  `refresh_session`, `merge_refresh` and `refresh_token_for`, run on a worker
  thread at start-up when the stored session has expired, with the result written
  back to the keyring. 3 tests, including the case where Discord omits a rotated
  refresh token. Test suite: 183.
- Wrote a design proposal for multiple bot accounts (D20 appendix) and did not
  start it: it changes keyring layout, session ownership and identify budgeting.
- Wrote `YAPFAST_PARITY.md`, an honest audit against Yapfast's public feature
  list: implemented, partial, missing-but-legitimate, refused-by-decision and
  needs-research, with the bot-token line drawn explicitly.

- Made the UI translucent and wallpaper-aware (D39). `src/wallpaper.rs` detects the
  desktop image on Hyprland (hyprpaper IPC, swww, config, or any image path in the
  Hyprland config tree), KDE, GNOME and XFCE, then downscales to 320 px, box-blurs
  it three times and uploads one texture; the app paints it under every panel on
  the background layer. The window itself is transparent (`with_transparent`) and
  themed surfaces use alpha 205 (`AppTheme::veiled`), so the real desktop shows
  through even when no image can be found. Settings → Appearance gained a glass
  toggle and a wallpaper path field (applied on focus loss, `~`/`$HOME` expanded).
  8 wallpaper tests plus one settings test; suite now 267.
- Replaced the icon with a blurple squircle and a stylised white face, drawn as
  original SVG (`assets/rustcord.svg`, 256 px PNG rendered from it and embedded as
  the window icon). `scripts/install-local.sh` installs the SVG into the hicolor
  theme.

- Started the Vencord-style extras (D40) with a new Settings → Extras section.
  `message_timestamps` names the day on every row (Today / Yesterday / date, year
  when needed) and keeps weekday, date and seconds in the tooltip; the formatting
  is a pure tested function. `character_counter` draws `N/2000` at the composer's
  right edge, accent near the limit and red past it, using the same constant the
  send guard enforces. 2 new pure tests; suite now 269.

- Integrated Discord Rich Presence (D41). `src/rpc.rs` speaks the official local
  IPC protocol: socket discovery for native, Flatpak, Snap and Vesktop clients,
  handshake with a public application id, `SET_ACTIVITY` frames, a worker thread
  with reconnect/replay and one-time logging, and pure framing/field helpers with 6
  tests. Settings → Extras gained the opt-in switch, application id field (falls
  back to the OAuth id) and a channel toggle; presence updates on channel changes
  and settings edits, and never blocks the UI. Suite now 276.

- Tinted folder members with the folder colour (D42). A server inside an expanded
  folder now blends its tile 55 % toward that folder's colour and carries a 1.5 px
  ring in it, while selection keeps the accent fill and white pill; closed folders
  blend their 2×2 preview cells too. Added `utils::mix` (alpha-preserving, clamped)
  with a test; suite now 277.

- Completed roadmap item 5 (D43): Spotify now-playing is live. `api/spotify_auth.rs`
  adds the PKCE public-client flow on its own loopback port with
  `user-read-currently-playing`, keyring session storage that records the issuing
  client id, and refresh; `spotify_sync.rs` polls every five seconds on a worker
  thread, refreshes with a one-minute slack, dedupes errors and reports
  Track/Idle/Error. Settings → Notifications gained the client-id field, Connect and
  Disconnect with a status line; the plugin's demo track is replaced by live data
  and cleared when nothing plays. Privacy allowlist, store list and README name the
  two Spotify hosts. 5 new tests; suite now 283.

- Sent typing indicators (D44). `DiscordClient::trigger_typing` posts to
  `/channels/{id}/typing`; the composer calls `notify_typing` on every text change
  and `models::typing_due` throttles to one call per eight seconds per channel
  (immediately on a channel switch), with empty drafts and non-text channels
  skipped and failures logged at debug. Inbound typing already surfaced above the
  composer. The pure throttle is tested at the eight-second boundary; suite now
  283.

- Made search session-wide with a channel filter (D45). `SearchQuery` gained
  `channel` parsed from `in:#name`, `channel:name` or a bare `#name`; `search()`
  takes a channel-id→name map so the filter and the result labels stay correct
  after renames; hits carry their channel name and the overlay leads each row with
  `#channel`, reports how many channels are cached, and prints the parsed filter.
  Two new tests; suite now 285.

- Finished roadmap item 8, the WASM sandbox (D46): `src/plugins/wasm.rs` loads
  `plugins/<name>/plugin.json` + `.wasm` through `wasmi` with no host imports,
  permission gating (`transform_messages`), fuel (5 M default / 20 M ceiling),
  16 MiB memory and 8 KiB I/O caps, path-traversal checks, fail-closed calls, and
  an opt-in Extras toggle; 15 tests. Added `packaging/PKGBUILD` for item 10 and
  `docs/VOICE_AND_STREAMING.md` explaining why items 6–7 are blocked on DAVE.
  Suite now 300.

- Animated the typing indicator and mirrored it in the channel header (D47):
  `models::typing_dots` is a pure 400 ms three-frame cycle, `summary` no longer
  embeds an ellipsis, `ui/chat.rs` computes the label once and draws it in both
  places, and repaints every 120 ms only while somebody is typing. One new test.

- Renew the typing notice while a draft keeps focus (D48): the composer calls the
  throttled `notify_typing` every frame when focused with text, and
  `typing_refresh_in` schedules exactly one repaint at the eight-second mark
  instead of polling. Assertions added to the existing typing test; total 301.

- Added `before:`/`after:` search bounds (D49): `YYYY-MM-DD` or
  `YYYY-MM-DDThh:mm[:ss]` interpreted as UTC, both exclusive; invalid dates stay
  literal terms; `describe()` and the overlay hint/footer name them. One new test;
  suite 302.

- Search results now highlight matches and jump to the message (D50):
  `search::highlight_ranges` (merged, case-insensitive, lowercase-length safe),
  a single `LayoutJob` per row, `jump_to_message` plus a bounded convergence loop
  over `RowMetrics::estimated_offset_of`, a 2.5 s tint, and cancellation on manual
  scroll. Two new tests; suite 304.

- Added a worked WASM plugin (`examples/wasm-uppercase`) plus `build_plugin`
  (WAT → wasm) and `check_plugin` (load + one transform through the real loader),
  so the sandbox can be exercised without a Discord session. Not yet dropped into
  the live config folder or seen in the UI.

- Avatars and Nitro-style profile cards (D51): custom/default CDN avatars through
  the existing image cache, display names, banners, accent colours and avatar
  decorations parsed from gateway and REST user objects, hover profile card.
  Additive fields on `User`; 1 new test; suite 307.

- Removed the seeded "Smooth Operator" demo track (D52). The now-playing strip now
  appears only after the Spotify poller reports an actually-playing track, and
  `clear_spotify` on idle/pause hides it. 1 new test; suite 308.

- Sidebar account avatar (D53): READY now forwards the signed-in user's avatar,
  display name and decoration; the account panel reuses `ui::chat::avatar` and its
  profile card. Gateway test extended; suite 308.

- Member-list avatars (D54): rows reuse `ui::chat::avatar` and request roster
  avatars through `ImageLoader`; existing page data already carried the hashes.
  No new test; suite unchanged at 308.

- Now-playing strip shows album art and a pause state (D55): paused tracks are kept
  with `is_playing`, progress freezes while paused, `i.scdn.co` added to the image
  and privacy allowlists. 3 new tests; suite 311.

- DMs from the member list (D56): `open_dm` POSTs `/users/@me/channels`, channels
  live under the synthetic `direct-messages` guild and reuse all text-channel
  paths; rail/header label them `@`. 1 new test; suite 312. Not exercised against
  Discord (D28); session-only list.

- DM list at startup (D57): `GET /users/@me/channels` loads DMs and group DMs into
  the synthetic rail entry once per connection, with empty state and merge with
  member-opened DMs. 1 new test; suite 313. Live fetch unverified (D28).

- Discord-style message blocks (D58): consecutive messages from one author within
  seven minutes share one avatar/name block with a gutter clock and no gap; replies
  break blocks. Member list split into ONLINE/OFFLINE sections. 1 new test; suite
  314. UI not visually inspected.

- Spotify as Discord Rich Presence (D59): while playing, the local RPC activity
  carries song/artist, elapsed start and end timestamps, and the album as large
  text; paused/idle falls back to server/channel. 1 new test; suite 315. Not
  exercised against a running Discord client/IPC socket.

- Server icons and member counts (D60): rail loads guild icons through the image
  cache; `GET /guilds/{id}?with_counts=true` supplies approximate total/online
  counts beside the roster. 2 new tests; suite 317. Live CDN/REST unverified.

- Presence dots on member avatars (D61): status-coloured dot with a panel ring,
  green/amber/red/grey, member list only; 1 new test; suite 318. Live presence data
  still depends on the privileged intent, so live rosters default to offline dots.

- Channel-scoped member panel (D62): only message authors in the open channel plus
  its voice members, merged with roster data, `IN CHANNEL — n`; Discord-style hover
  fills on message and member rows. Suite unchanged at 318; UI not visually checked.

- Resource bounds (D63): glibc arena cap, image cache default 32 MB, at most three
  cached guild rosters. Measured idle demo RSS ~246 MB -> ~229 MB mean; CPU
  unchanged at ~0.2-0.4%. Suite unchanged at 318.

- Immediate numbered defaults (D64): six Discord default PNGs embedded and uploaded
  at startup, `avatar_url` now custom-only, first frame never shows initials for a
  snowflake id. 2 new tests; suite 319. Appearance not visually checked.

- Channel categories (D65): `Channel.parent_id`/`position` parsed, collapsible
  category sections ordered by Discord position, uncategorised fallback groups,
  keyboard order matches. 1 new test; suite 320. Appearance not visually checked.

- Animated guild icons (D66): `a_` hashes request GIFs, worker keeps all frames
  with delays, texture cache animates them on egui's clock at ≤12 fps. 4 new
  tests; suite 324. Motion not visually checked.

- Periodic guild counts (D67): every real guild refreshes approximate total/active
  numbers at session start and every five minutes; counts survive roster eviction
  and empty responses no longer erase them. 1 new test; suite 325. Live REST
  cadence not exercised without a bot session.

- Message hover action bar (D68): react picker, one-click reply and a more menu
  that shares the context-menu contents; painted inside the row so row metrics
  do not move. 3 new tests; suite 328. Hover/popup behaviour not visually checked.

- Roster-seeded member panel (D69): silent guild members and roster-backed thread
  participants included, authors/voice merged, virtualised rows, only visible
  avatars requested. 1 new test; suite 329. Live roster/thread behaviour not
  exercised without a bot session.

- Discord header/chrome pass (D70): channel topics parsed and displayed, working
  search/member toggle, settings moved to user panel. 1 new test; suite 330.
  Release peak 256 MB, under the 300 MB ceiling. Rendering not visually checked.

- Animated avatars and banners (D71): `a_` avatar, profile-banner and guild-banner
  hashes use GIF URLs, the existing frame pipeline decodes them, all avatar
  surfaces and profile cards draw the current frame, and the guild hover peek
  gained a server banner. 3 new tests; suite 333. Release peak 256 MB, 44 MB of
  headroom. Motion and appearance not visually checked.

- Guild icon focus pause (D72): the rail keeps its own animation clock, advances
  it only while the window has focus, and stops scheduling frames in the
  background. 1 new test; suite 334. Release peak 264 MB, 36 MB of headroom.
  Focus transition not exercised without a desktop session.

- Immediate counts refresh (D73): focus regain and gateway READY/RESUMED force
  the next guild-count pass instead of waiting for the five-minute cadence; an
  in-flight pass is not duplicated. 2 new tests; suite 336. Release peak 242 MB,
  58 MB of headroom. Live focus/reconnect triggers not exercised.

- Server hover member counts (D74): the rail peek shows total and active
  approximate counts under the server name, falling back to the guild payload
  and loaded roster, and omitting fields that are not known. 1 new test; suite
  337. Release peak 261 MB, 39 MB of headroom. Peek not visually checked.

- Composer toolbar (D75): paperclip and native file picker with a removable
  attachment chip, multipart upload with reply/empty-caption support, and a
  bounded Unicode emoji picker that inserts into the draft. 3 new tests; suite
  340. Release peak 251 MB, 49 MB of headroom; binary ~14 MB. Picker and live
  upload not visually exercised.

- Role-grouped member panel (D76): roles parsed from the gateway and refreshed
  from `/guilds/{id}/roles`, member role ids from the roster, sections ordered by
  hoisted-role position with role-colour dots, and ONLINE/OFFLINE catch-alls for
  unhoisted members. 3 new tests; suite 343. Release 211 MB RSS / 164 MB PSS /
  250 MB peak, 50 MB of headroom. Live rendering not visually checked.

- Message dividers (D77): local-date separators and a NEW line at the first
  live-unread message, integrated into the variable-height virtual list and
  breaking author grouping. 4 new tests; suite 347. Release 221 MB RSS / 171 MB
  PSS / 236 MB peak, 64 MB of headroom. Dividers not visually checked.

- Caret-aware emoji insertion (D78): the picker reads the editor's persisted
  character selection, replaces it (or inserts at the collapsed caret), and
  stores the caret after the glyph instead of appending to the draft. 1 new test;
  suite 348. Release 220 MB RSS / 154 MB PSS / 239 MB peak, 61 MB of headroom.
  UI event path not visually checked.

- Bounded attachment queue (D79): the composer accepts multiple files, shows up
  to ten removable rows with a running total, and enforces 10 MiB per file plus
  20 MiB per batch in both the composer and the upload path. Multipart sends
  map `files[n]` to matching attachment ids and filenames. 2 new tests; suite
  350. Release 216 MB RSS / 154 MB PSS / 238 MB peak, 62 MB of headroom. Live
  multi-file upload not exercised.

---

## 7. Notes for the next agent

- Do not re-add email/password login.
- Do not serialize tokens into settings or logs.
- Preserve the egui 0.37 panel/App API shape when editing UI code.
- If you add gateway reconnect logic, add tests around payload handling before
  touching the socket loop.
- The gateway is only compiled/tested against synthetic payloads here; a real
  bot token was not used in this environment.
- If the keyring backend is unavailable, token save fails visibly; the token can
  still be used in memory for the current session.
- Keep the release profile size-conscious; adding large dependencies increases
  the ~11 MB binary.
