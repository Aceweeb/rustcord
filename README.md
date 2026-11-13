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
Message editing and deletion are implemented, and voice-channel **presence** (who
is in which channel) comes from the gateway. Sandboxed WebAssembly plugins load
from the config folder behind an opt-in toggle. Voice **audio** and screen capture
are intentionally left as explicit next steps rather than half-implemented stubs:
Discord requires DAVE end-to-end encryption for all media sessions since March
2026, and `docs/VOICE_AND_STREAMING.md` records what a compliant path involves.

## What works today

- Native egui UI: guild rail, channel list, member list, message history, composer.
- Right-click message menu: quick reactions, copy text, reply, edit and delete
  (the last two on the bot's own messages), all wired to the Discord REST API.
- Reaction chips are clickable: yours toggle off with a `DELETE`, anyone's chip
  adds yours with a `PUT`, and a failed request rolls the chip back.
- Guild member roster from the privileged Server Members intent, paginated over
  REST and capped at 5000; without the intent the panel falls back to message
  authors and says which toggle to enable.
- Unread and mention badges on channels and guild icons, tracked locally (bots
  have no server read state) and capped at 99+.
- Ctrl+K quick switcher over servers and channels, with prefix-first ranking,
  arrow/Enter navigation and a rebindable shortcut.
- Guild folders and drag-free ordering: right-click a server to move it, group it
  into a folder or ungroup it, rename a folder and give it a colour from its own
  menu. That colour also tints the folder's servers and its 2×2 preview, so an
  expanded group reads as one block. Stored in `settings.json`, since a bot cannot
  read a user's Discord folder settings.
- Keyboard-only navigation (`Ctrl+Shift+K` by default): arrow through the rail and
  channel list with an in-place focus ring, Enter opens, Escape leaves.
- Hover a server icon for a peek at its text and voice channels, with unread and
  mention counts; clicking a channel in the peek opens it directly.
- Typing indicators both ways: `TYPING_START` events appear above the composer
  ("alice and bob are typing…") and in the channel header, with the trailing dots
  animating on a 400 ms cycle; writing a draft tells the channel you are typing,
  and a focused draft renews that notice every eight seconds for as long as you
  keep it focused.
- A native message log (`Ctrl+Shift+L`) of edits and deletions seen this session,
  filterable, capped at 500 entries and never written to disk. Vencord's plugins
  patch Discord's own client and cannot load here; this is the same feature built
  natively.
- Vencord-style extras, built natively instead of loaded as plugins
  (Settings → Extras): timestamps on every message (`Today at 21:04`,
  `Yesterday at …`, then `3 Jan at …`, with the exact seconds in the tooltip) and
  a live character counter in the composer that turns accent-coloured near
  Discord's 2000-character limit and red once the message is too long to send.
- Ten-section settings window: Appearance, Extras, Themes, Performance, Memory,
  Notifications, Shortcuts, Privacy, Diagnostics, About (`Ctrl+,`).
- 12 built-in theme presets across Dark, Light, Purple and AMOLED, plus custom
  accent colours, painted previews, instant switching and persisted selection.
- Live memory and CPU figures sampled from `/proc`, with a budget alert.
- Dark theme with live palette application.
- Discord Rich Presence (Settings → Extras): publishes "playing Rustcord" with the
  server and, optionally, the open channel to the Discord client running on this
  machine, over its local IPC socket. No token is involved — it needs a public
  application id, and a missing socket is retried quietly in the background.
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
  (`src/api/spotify.rs`), plus a live now-playing poller (`src/spotify_sync.rs`)
  behind an opt-in Spotify PKCE connection.
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
- Direct messages and group DMs, loaded at startup into a "Direct Messages" rail
  entry; open new ones from a member row, with history, sending, unread and typing.
- Real server icons in the guild rail, with the server's approximate member total
  and online count in the member panel.
- A member panel scoped to the open channel: its message authors and current voice
  members, with presence dots and Discord-style row hover.
- Real profile pictures with a hover profile card (display name, accent colour,
  banner and avatar decoration when Discord sends them), loaded through the
  bounded image cache.
- Local full-text search (Ctrl+F) across every message cached this session, from
  every channel opened so far, with `from:name`, `in:#channel` (or `#channel`) and
  `before:` / `after:` date filters (exclusive, UTC). Results show the channel, author, time and a snippet, and clicking one
  opens that channel. Discord's server-side search needs a user token, so the
  overlay searches what is already in memory and says so.
- Reconnects stay inside Discord's identify allowance (ten an hour, D19) with
  jittered backoff, so a broken token degrades into a slow retry instead of a
  lockout, and a crash loop cannot reset the ladder every two seconds.
- Optional **Discord identity** login (OAuth2 `identify`, PKCE + state, loopback
  redirect) showing your username, avatar initial and user id. It is a separate,
  read-only login stored under its own keyring entry: it grants no messaging, and
  Rustcord still never asks for a password, a user token or a cookie. An expired
  identity session refreshes itself when it can, and is otherwise flagged.
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
- Voice **audio** (joining a call): voice-server WebSocket handshake, UDP/RTP,
  Opus, jitter buffering and DAVE/E2EE. The sidebar does list who is in each
  voice channel (`DECISIONS.md` D24); there are deliberately no mute/join
  buttons, because they would control nothing.
- Video/screen capture/streaming.
- Attachment upload and sending reactions. (Incoming attachments and embeds
  *are* rendered, and editing/deleting your own messages works.)
- OAuth2 login. Email/password login is intentionally not implemented: it is
  not part of Discord's supported third-party API model, it exposes account
  credentials to this process, and it violates Discord's terms. The same applies
  to OAuth2 flows that produce a user token for messaging: that is a self-bot by
  another name. Use a bot token (stored in the keyring) or an approved OAuth2 app
  instead.
- Voice audio, video and Go Live. Presence is shown, but joining a voice channel
  requires DAVE (MLS-based E2EE), which Discord now enforces for third-party
  clients; see `docs/VOICE_AND_STREAMING.md` for the gate and the build order.

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

## Install it in the application launcher

One command builds, installs and registers Rustcord as a desktop app:

```bash
scripts/install-local.sh
```

It installs, for the current user only (no root):

| File | Purpose |
| --- | --- |
| `~/.local/bin/rustcord` | the release binary |
| `~/.local/share/applications/rustcord.desktop` | launcher entry, with an absolute `Exec` |
| `~/.local/share/icons/hicolor/scalable/apps/rustcord.svg` | the icon |

Then search for **Rustcord** in the launcher (Super, or `hyprlauncher` / `rofi` /
`wofi` depending on the session). **Re-run the script after pulling or building**
to update the installed app: it replaces the binary, icon and entry in place, so
the launcher always starts the current build.

The entry uses an absolute path on purpose — a launcher started by the session
does not necessarily have `~/.local/bin` on `PATH`, and a bare `rustcord` would
fail silently. `StartupWMClass=rustcord` matches the window class (`hyprctl
clients` shows `class: Rustcord`), so the running window groups with the icon
rather than appearing as a second, unnamed app.

Overridable for packaging or a different layout: `BIN_DIR=… APPS_DIR=…
ICON_DIR=… scripts/install-local.sh`.

## Look and feel

The default theme is Discord's own palette (`#313338` chat, `#2b2d31` sidebars,
`#1e1f22` rail, `#5865f2` blurple) and the layout uses Discord's measurements:
a 72 px server rail with 48 px icons that round off when selected, a 240 px
channel list with 34 px rows, a 48 px channel header, a 240 px member list and
40 px message avatars. Twelve other presets and CSS theme files are still there
in Settings → Appearance if you want something else.

The window is translucent by default: themed surfaces carry alpha 205 over a
transparent window, so the desktop wallpaper behind the app reads through the
glass. Rustcord also looks up the wallpaper image itself — Hyprland (hyprpaper,
swww, or any image path in the Hyprland config tree), KDE, GNOME and XFCE — blurs
a 320 px copy and paints it beneath the panels, which keeps the frosted look on
compositors that do not blur windows. If no image is found the window stays
transparent, so the real desktop shows through. Settings → Appearance has the
`Translucent glass` switch and a wallpaper path field (absolute, `~/…` or
`$HOME/…`); leave the field empty to auto-detect.

On Hyprland the desktop behind the window is only blurred by the compositor if you
ask for it; one line in your Hyprland config does that:

```
windowrule = blur, class:^(rustcord)$
```

The icon is a blurple squircle with a stylised white face, drawn as original SVG
(`assets/rustcord.svg`) with a 256 px PNG rendered from it and embedded as the
window icon, so the launcher, taskbar and titlebar all show the same mark.

## Where your data lives

Rustcord is a local program talking to Discord, plus Spotify if you choose to
connect it for now-playing. There is no server of ours, no account to create, no
database file and no analytics.

| What | Where |
| --- | --- |
| Preferences | `~/.config/rustcord/settings.json` — no credentials, no messages |
| Bot token | OS keyring, `rustcord` / `discord_bot_token` |
| Identity session | OS keyring, `rustcord` / `discord_oauth_identity` |
| Spotify session | OS keyring, `rustcord` / `spotify_session` — only if connected |
| Messages | memory only, capped, gone when the window closes |
| Images | RAM texture cache, bounded by `image_cache_mb` |
| Diagnostics | memory ring buffer, exported only when you ask |

Network traffic goes to Discord and nowhere else unless you connect Spotify, and
that is enforced rather than promised: every REST call, the gateway socket
(including a `resume_gateway_url` from a payload), every image download and both
Spotify endpoints pass an exact-match allowlist of `discord.com`,
`gateway.discord.gg`, `cdn.discordapp.com`, `media.discordapp.net`, the two
`images-ext-*.discordapp.net` hosts, `accounts.spotify.com` and `api.spotify.com`.
Anything else is refused.

Settings → Privacy shows this same list, with the live history cap and image
budget. To verify it yourself:

```bash
scripts/check-local.sh      # crates, source hosts, binary scan
```

With no token configured the app opens no sockets at all, and a run that changes
nothing writes nothing — the settings file only appears when you change a setting.

## Configuration

Copy or edit `settings.json`:

```json
{
  "vencord_themes": true,
  "spotify_status_sync": true,
  "streaming_notifications": true,
  "resource_priority": "balanced",
  "theme_file": "themes/vencord-example.theme",
  "show_blyat_banner": true,
  "translucent": true,
  "wallpaper": null,
  "message_timestamps": true,
  "character_counter": true,
  "discord_rpc": false,
  "rpc_client_id": null,
  "rpc_show_channel": true,
  "spotify_client_id": null
}
```

`translucent` turns the glass on or off, and `wallpaper` pins the backdrop image
for the blurred layer; `null` means "detect it" (Hyprland, KDE, GNOME, XFCE).
`message_timestamps` and `character_counter` are the Extras toggles, and
`discord_rpc` publishes Rich Presence with `rpc_client_id` (falling back to
`oauth_client_id`), optionally naming the open channel when `rpc_show_channel` is
true. `spotify_client_id` is the public application id the now-playing connection
was made with; the tokens themselves stay in the keyring.

### Spotify now-playing

The `spotify-status` plugin follows what you are actually playing; the strip stays
hidden until a track is playing. Create an application at the Spotify developer
dashboard, add
`http://127.0.0.1:53135/callback` as a redirect URI, then paste the client id into
Settings → Notifications → Spotify application id and press **Connect Spotify**. The
PKCE flow asks only for `user-read-currently-playing`, keeps the tokens in the OS
keyring, and polls every five seconds. The strip shows the album art and a pause
marker when Spotify reports a paused track, and clears when nothing is loaded or
Spotify closes; **Disconnect Spotify** forgets the grant. No client secret exists in this
program.

### Rich Presence

Create a Discord application (the same one as the bot is fine), copy its
**Application ID** from the General Information page, and paste it into
Settings → Extras → Application id with the presence switch on. Rustcord then talks
to the Discord desktop client over its local socket; the activity shows the server
you are in and, unless you turn it off, the channel you are reading. Nothing is sent
to a server of ours, and no credential leaves the app. The Discord client has to be
running — with it closed, the app logs that it found no IPC socket and keeps
checking in the background.

`theme_file` can be absolute or relative to the working directory. If it is
`null`, Rustcord looks for `~/.config/rustcord/theme.theme`.

### Discord connection

On first run Rustcord opens on a sign-in page: create an application, use **Add
the bot to a server** to invite it with only the permissions Rustcord uses, then
paste the **bot token**. A bot that is in no server has nothing to read, which is
the most common reason a correct token appears to do nothing.

The token is stored in the Secret Service keyring under `rustcord` /
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
├── auth.rs              # bot token + identity session in the OS keyring
├── search.rs            # local message search over loaded history
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

### Sandboxed WebAssembly plugins

Enable **Settings → Extras → Sandboxed WebAssembly plugins**, then drop a folder
into `~/.config/rustcord/plugins/` (restart to load it):

```text
~/.config/rustcord/plugins/uppercase/
├── plugin.json
└── plugin.wasm
```

```json
{
  "name": "Uppercase",
  "version": "1.0.0",
  "module": "plugin.wasm",
  "permissions": ["transform_messages"],
  "fuel": 500000
}
```

The module runs under `wasmi` with no host imports at all, a fuel budget per call,
a 16 MiB linear-memory ceiling and 8 KiB caps on input and output. It must export
`memory`, `alloc(len) -> ptr`, `run(ptr, len) -> ptr` and `result_len() -> i32`;
`run` receives UTF-8 message text and returns UTF-8 replacement text. A plugin
without `transform_messages`, with an unknown permission or manifest key, with a
`start` function, with imports, or whose path escapes its folder is refused at
load. Any trap, out-of-fuel, bad pointer or invalid UTF-8 leaves the message
exactly as typed. The loader lives in `src/plugins/wasm.rs`.

A worked example is in `examples/wasm-uppercase`: a 272-byte module that
ASCII-uppercases message text, with the ABI documented in the `.wat`. Rebuild and
try it without a Discord session:

```bash
cargo run --example build_plugin -- examples/wasm-uppercase/plugin.wat \
    examples/wasm-uppercase/plugin.wasm
cargo run --example check_plugin -- examples/wasm-uppercase
```

`check_plugin` loads the folder through the same `WasmPlugin::load` and `Plugin`
hook the app uses and prints the transformed message.

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
