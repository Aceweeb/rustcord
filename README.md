# Rustcord

A lightweight, native Discord-shaped client scaffold written in Rust + `egui`.
No Electron, no Chromium, no WebView2. The UI is drawn by `egui`/`glow`; the
binary is a single native executable.

> New session? Read `PROGRESS.md` first — it is the durable checkpoint.

This is a **scaffold**, not a drop-in Discord replacement. It gives you a
runnable app, a clean module layout, a typed plugin system, a Vencord-style
theme parser, Spotify "now playing" presentation state, streaming indicators,
and typed Discord/Spotify REST clients. A real Discord Gateway transport is
included (connect, Hello, Identify, heartbeat, READY/MESSAGE_CREATE parsing).
Voice, screen capture, OAuth, and the WASM loader are intentionally left as
explicit next steps rather than half-implemented stubs that pretend to work.

## What works today

- Native egui UI: guild rail, channel list, member list, message history, composer.
- Nine-section settings window: Appearance, Themes, Performance, Memory,
  Notifications, Shortcuts, Privacy, Diagnostics, About (`Ctrl+,`).
- 12 built-in theme presets across Dark, Light, Purple and AMOLED, plus custom
  accent colours, painted previews, instant switching and persisted selection.
- Live memory and CPU figures sampled from `/proc`, with a budget alert.
- Dark theme with live palette application.
- Vencord/BetterDiscord-style `.theme` parsing for CSS custom properties:
  `--background-primary`, `--background-secondary`, `--background-tertiary`,
  `--text-normal`, `--text-muted`, `--brand-experiment`, `--status-online`,
  `--status-danger`, etc.
- Plugin manager with a `Plugin` trait and four built-ins:
  - `vencord-compat` — theme parser / palette adapter
  - `spotify-status` — now-playing state + progress bar
  - `streaming-indicator` — LIVE badge and viewer count
  - `blyat-banner` — toggleable example banner, configured in `settings.json`
- Settings window (`Ctrl+,` or the gear button) persisted to
  `~/.config/rustcord/settings.json`. A local `settings.json` is used as a
  fallback for development.
- Typed REST clients for Discord API v10 (`src/api/discord.rs`) and Spotify
  (`src/api/spotify.rs`).
- Discord Gateway transport (`src/api/gateway.rs`): TLS WebSocket, Hello,
  Identify, heartbeat loop, READY / GUILD_CREATE / MESSAGE_CREATE parsing.
- Live guild and channel sync from `GUILD_CREATE`; the composer switches from
  local demo messages to the Discord REST API once live channels exist.
- Real channel history from `GET /channels/{id}/messages`, paged backwards with
  Discord's `before` cursor, rendered through a variable-height virtualised list
  so only visible rows cost anything.
- Inline attachments and embeds with a byte-bounded LRU image cache
  (`image_cache_mb`, default 48 MB). Only visible rows are downloaded, only
  Discord's CDN hosts are contacted, and the cache drops least-recently-used
  textures to stay inside the budget.
- Bot-token authentication stored in the OS keyring (`src/auth.rs`). Tokens are
  never written to `settings.json`.
- Optional **Discord identity** login (OAuth2 `identify`, PKCE + state, loopback
  redirect) showing your username, avatar initial and user id. It is a separate,
  read-only login stored under its own keyring entry: it grants no messaging, and
  Rustcord still never asks for a password, a user token or a cookie.
- Sign-in page on first run (`src/ui/login.rs`): token shape is checked before a
  request is spent, "Continue in demo mode" skips it, and Settings → Privacy →
  **Sign out** clears the keyring and returns to it.
- Release profile tuned for size (`opt-level = "z"`, LTO, strip).

Measured on the development machine (Arch, Wayland, release build):

| Metric | Result |
| --- | --- |
| Binary size | ~11 MB stripped |
| Idle RSS (release, 1280×800) | 159 MB (`VmRSS`) |
| Idle PSS (fair per-process figure) | 57–61 MB (`smaps_rollup`) |
| Peak RSS | 160 MB (`VmHWM`) |
| Steady-state CPU | 0.0%, 0.2 fps while idle |
| Threads (no gateway token, no images) | 2 |

The sign-in screen measures lower (100–141 MB RSS / 45–58 MB PSS) because no
channel surface is built until there is a session.

RSS for a GL application counts shared library pages in full, and on this machine
~100 MB of it is the proprietary NVIDIA driver stack (`libnvidia-gpucomp`,
`libnvidia-eglcore`). Rustcord also pins libglvnd to the NVIDIA EGL vendor, which
avoids mapping Mesa's `libLLVM` and gallium driver entirely—worth ~33 MB. The
application's own allocations are ~23 MB anonymous. Reproduce every figure with
`scripts/measure-memory.sh 20`.

## What is deliberately not implemented

- Gateway resume/reconnect/backoff, sharding, and a rate-limit queue.
- Voice/video transport, Opus codec, screen capture/streaming.
- Attachment upload, sending reactions, typing indicators, and message editing.
  (Incoming attachments and embeds *are* rendered; only uploading is missing.)
- OAuth2 login. Email/password login is intentionally not implemented: it is
  not part of Discord's supported third-party API model, it exposes account
  credentials to this process, and it violates Discord's terms. The same applies
  to OAuth2 flows that produce a user token for messaging: that is a self-bot by
  another name. Use a bot token (stored in the keyring) or an approved OAuth2 app
  instead.
- Real Spotify polling loop; the plugin currently uses a demo track.
- WASM plugin execution. `load_wasm_plugin` fails closed on purpose; see
  `src/plugins/mod.rs` for the intended permission model.

## Build on Arch Linux

```bash
sudo pacman -S --needed base-devel rust cargo pkgconf \
  wayland wayland-protocols libxkbcommon \
  libx11 libxcursor libxi libxrandr libxinerama \
  mesa vulkan-icd-loader dbus gnome-keyring

make check
make run
make build      # target/x86_64-unknown-linux-gnu/release/rustcord
```

If you only need the default host target:

```bash
cargo run --release
```

The `Makefile` also has `make deps` and `make profile` (Valgrind Massif).

## Configuration

Copy or edit `settings.json`:

```json
{
  "vencord_themes": true,
  "spotify_status_sync": true,
  "streaming_notifications": true,
  "resource_priority": "balanced",
  "theme_file": "themes/vencord-example.theme",
  "show_blyat_banner": true
}
```

`theme_file` can be absolute or relative to the working directory. If it is
`null`, Rustcord looks for `~/.config/rustcord/theme.theme`.

### Discord connection

On first run Rustcord opens on a sign-in page; paste a **bot token** there. The
token is stored in the Secret Service keyring under `rustcord` /
`discord_bot_token` and the gateway starts immediately, with progress in the
status bar. The field also lives in Settings → Privacy, next to `Connect gateway`
and `Sign out`, which drops the keyring entry and returns to the sign-in page.
Tokens are never serialized into `settings.json`.

On Arch you need a running Secret Service provider (for example
`gnome-keyring` or a compatible KWallet setup). If the keyring backend is not
available, saving the token fails with an error and the token remains in memory
for the current session only.

Email/password login is not supported by design. Discord's supported
third-party path is bot tokens or an approved OAuth2 application; automating
email/password login would handle raw account credentials and violates the
service terms.

A sample theme lives at `themes/vencord-example.theme`. Only CSS custom
properties are interpreted; selectors, `@import`, and arbitrary CSS are ignored
by design. That is what keeps the parser safe and small.

## Project layout

```
src/
├── main.rs              # eframe bootstrap
├── app.rs               # application state, settings window, mock data
├── auth.rs              # bot token storage via the OS keyring
├── models.rs            # User, Guild, Channel, Message, Activity, ...
├── settings.rs          # JSON config load/save (no credentials)
├── images.rs            # bounded LRU of decoded textures, CDN allowlist, worker
├── api/
│   ├── discord.rs       # typed REST client for Discord v10
│   ├── gateway.rs       # WebSocket gateway, heartbeat, dispatch parsing
│   └── spotify.rs       # currently-playing client
├── plugins/
│   ├── mod.rs           # Plugin trait, PluginManager, WASM hook
│   ├── vencord_compat.rs
│   ├── spotify_status.rs
│   ├── streaming_indicator.rs
│   └── blyat.rs
├── ui/
│   ├── theme.rs         # palette + egui Visuals application
│   ├── sidebar.rs       # guild rail, channel list, user panel
│   ├── virtual_list.rs  # variable-height row virtualisation
│   ├── settings.rs      # nine-section settings window
│   └── chat.rs          # top bar, members, messages, composer
└── utils/mod.rs         # color parsing, initials, duration formatting
```

## Plugin authoring

Implement `Plugin` and register it with `PluginManager`:

```rust
use rustcord::plugins::{Plugin, PluginContext};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &str { "my-plugin" }
    fn init(&mut self, _ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        Ok(())
    }
    fn on_message(&mut self, message: &mut rustcord::models::Message) {
        message.content.push_str(" ✨");
    }
}
```

The manager calls `init`, `on_message`, `on_react`, `on_settings`,
`on_activity_update`, and `on_streaming_update` on every registered plugin.
The built-ins are held concretely so the UI can query typed state without
downcasting.

WASM plugins are the intended extension path, but the loader is a fail-closed
placeholder. A production loader must validate a manifest, enforce a permission
list, and run the module in a sandbox (`wasmi`/`wasmtime`) before exposing it as
a `Plugin`.

## Performance notes

- `egui` avoids a browser engine entirely; the renderer is OpenGL via `glow`.
- The release profile uses `opt-level = "z"`, LTO, one codegen unit, and strips
  symbols. Expect a much smaller binary than an Electron client; actual RAM/CPU
  numbers depend on fonts, window size, and how much history you load.
- Message history is virtualised (`ui/virtual_list.rs`) with measured row
  heights, because egui's uniform-height row API would clip multi-line messages
  and image rows.
- The `resource_priority` setting is a policy hook, not a scheduler yet. A
  production implementation can use it to cap repaint rates, history fetch
  depth, and image decode concurrency.

## Legal / account safety

Discord's Terms of Service restrict third-party clients and self-bots. A
user-token client can get an account terminated. Use the official bot API, an
approved OAuth2 integration, or your own test server. The typed `DiscordClient`
accepts whatever auth scheme you pass in (`Bot <token>` is the intended one for
bots). Do not ship a client that logs in with user tokens.

## Architect's note

**Tauri vs. pure egui.** Tauri gives you a real webview and therefore the
easiest path to pixel-perfect Discord CSS, but on Windows it pulls in WebView2
(Chromium), which contradicts the "no Chromium" constraint and inflates the
runtime. Pure `egui` keeps the binary native and small, but you reimplement
layout and styling yourself. This scaffold chooses `egui` so the resource and
platform constraints are honest.

**Theme compatibility.** Vencord themes are CSS, and replicating CSS in a
non-web renderer is not a 1:1 mapping. The parser intentionally supports the
high-value custom properties (backgrounds, text, accent, status colors) and
ignores selectors. That covers most "recolor" themes without shipping a CSS
engine. The `ThemeEngine`/`inject_theme` boundary from the brief maps to
`VencordCompatPlugin::reload` and `AppTheme::apply`.

**Plugins.** Native trait objects are fast and type-safe, but they are not a
sandbox. The WASM slot is where untrusted third-party code belongs; it must stay
fail-closed until a real permission model and interpreter are wired in.

**Streaming/Spotify.** Both are presented as plugin-owned state so the UI does
not care where the data came from. The REST clients are separate, which makes it
straightforward to add a token refresh loop and a gateway task later without
rewriting the UI.
