# Rustcord — Progress / Handoff Notes

> **Partly superseded.** The project now maintains `PROJECT.md`,
> `ARCHITECTURE.md`, `TASKS.md`, `CURRENT_STATE.md`, `DECISIONS.md` and
> `HANDOFF.md`. Read `CURRENT_STATE.md` for verified numbers and `HANDOFF.md`
> for the next task; sections 1–3 below describe the earlier scaffold.
> The project also moved to `/mnt/storage/Default Project/rustcord`.

Last updated: 2026-11-12 (Europe/Berlin)

Latest increment: message edit/delete and voice-channel presence (`DECISIONS.md`
D24). `HANDOFF.md` has the file-by-file detail; voice **audio** remains
deliberately unimplemented.

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
| Release binary | `target/release/rustcord`, ~11 MB stripped |
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
6. **Streaming / screen share**
   - Linux capture via PipeWire/X11
   - Discord GoLive gateway/voice state wiring
   - UI controls and viewer state
7. **Voice**
   - Opus codec + UDP/RTP/DAVE or a library abstraction
   - mute/deafen/volume UI sync
8. **WASM plugin sandbox**
   - manifest + permissions
   - `wasmi`/`wasmtime`
   - fuel/memory limits
   - fail closed on load errors
9. **Secure OAuth2**
   - PKCE, local redirect, refresh token storage in keyring
10. **Packaging**
    - `.desktop` file
    - AppImage / Flatpak / Arch PKGBUILD
    - icon and WM class

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
