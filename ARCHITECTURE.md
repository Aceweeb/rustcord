# ARCHITECTURE.md

## One process, four layers

```
main.rs                     installs log capture, loads Settings, starts eframe
  └─ app.rs                 RustcordApp: state, gateway polling, shortcuts,
     │                      telemetry tick, alert banners
     ├─ ui/                 egui rendering only (no network, no /proc)
     │   ├─ sidebar.rs      guild rail + channel list + user panel
     │   ├─ chat.rs         header, members, virtualised message list, composer
     │   │                  attachments/embeds, status bar
     │   ├─ virtual_list.rs variable-height row virtualisation (measured rows)
     │   ├─ settings.rs     nine-section settings window
     │   └─ theme.rs        presets + egui Visuals application
     ├─ plugins/            Plugin trait + four built-ins
     │   ├─ vencord_compat  CSS custom-property reader -> AppTheme
     │   ├─ spotify_status  activity state (demo track)
     │   ├─ streaming_indicator
     │   └─ blyat            example banner
     ├─ api/
     │   ├─ gateway.rs      tokio WebSocket: Hello/Identify/heartbeat,
     │   │                  RESUME, backoff, dispatch parsing
     │   ├─ discord.rs      typed REST v10 (history pages, send, current user)
     │   └─ spotify.rs      typed currently-playing client
     ├─ images.rs           byte-bounded LRU of decoded textures + CDN allowlist
     │                      + background decode worker
     ├─ settings.rs         Settings document + JSON persistence + migration
     ├─ shortcuts.rs        KeyChord parse/format + ShortcutAction table
     ├─ sysinfo.rs          /proc parsing: RSS, VmHWM, threads, fds, CPU ticks
     ├─ monitor.rs          throttled sampling + bounded history ring
     ├─ logbuf.rs           log::Log capture into a bounded ring
     ├─ diagnostics.rs      environment probes + report export
     ├─ auth.rs             bot token + OAuth2 identity session in the OS keyring
     ├─ oauth.rs            OAuth2 `identify`: PKCE, state, loopback callback,
     │                      session store trait (keyring / in-memory)
     └─ models.rs           User, Guild, Channel, Message, Activity, ...
```

## Data flow

1. **Startup**: `Settings::load_or_default` → `PluginManager::init_all` (theme
   resolution) → `RustcordApp::new` (demo data, optional gateway start).
2. **Frame**: `RustcordApp::ui` applies the theme if its palette changed, ticks
   the Spotify plugin, drains gateway events, send results, history pages and
   finished image decodes, dispatches shortcuts, samples `/proc` (throttled),
   then draws sidebar → chat → settings window.
3. **Images**: `chat::show` collects the URLs of rows it actually drew, and
   `ImageLoader::prepare` queues whatever is neither cached nor already in
   flight. The UI only uploads finished RGBA buffers; nothing blocks.
4. **Gateway events** are plain values over an `mpsc` channel; the UI thread
   never awaits. `poll_gateway` is non-blocking (`try_recv`).
5. **Live events**: `MESSAGE_UPDATE`, `MESSAGE_DELETE`, `MESSAGE_REACTION_*` and
   `TYPING_START` mutate the same list the view renders. Edits and deletes never
   refetch history, so pagination and measured row heights stay coherent; typing
   lives in a small expiring tracker (`models::TypingTracker`) with no timer.
6. **Sending**: in demo mode a message is appended locally; in live mode the
   REST call runs on the tokio runtime and returns via `mpsc`.
7. **Persistence**: settings are saved on change; message history is memory-only
   and clamped to `ResourcePriority::history_cap()`.

## Threads

- UI/render thread (the eframe loop).
- One tokio worker thread (`rustcord-gateway`), created only when a token
  exists. Measured: 2 threads total, no token.
- One `rustcord-images` thread, created on the first image request, which also
  starts reqwest's internal runtime thread. Both stay absent until an image is
  actually on screen, so a text-only session is still 2 threads.
- `smithay-clipboard`'s thread, owned by eframe.
- No other timers. `/proc` sampling is the single periodic wake-up, at 1 Hz
  while the settings window is open and 0.2 Hz otherwise, via
  `ctx.request_repaint_after`.

## Theme layering (in priority order)

1. Built-in preset: `ui::theme::PRESETS` (12 palettes, 4 categories).
2. Accent override: `Settings::accent_override` (`#rrggbb`).
3. `.theme` file: CSS custom properties only, parsed by `vencord_compat`.
   Selectors, `@import`, `url()` and arbitrary CSS are ignored.

`AppTheme::apply` maps the result onto `egui::Visuals`. Light presets switch the
base `Visuals` to `light()`, so widget polish stays correct.

## Performance model

- No WebView, so no renderer subprocess; RSS is one process.
- Immediate-mode rendering: only visible widgets are laid out each frame.
- Idle cost is bounded by `request_repaint_after`; there is no polling loop.
  Idle measures ~0.2 fps (one frame per telemetry tick); `RUST_LOG=debug` logs
  the frame count per tick so a repaint loop is visible without a profiler.
- The message list draws only rows intersecting the viewport, with measured row
  heights (`ui/virtual_list.rs`), so cost scales with the window, not the
  channel.
- Image memory is bounded by `Settings::image_cache_mb` and accounted per
  decoded pixel; eviction is LRU and frees the GPU texture.
- Message buffer is bounded; theme presets are `Copy` palettes in `.rodata`.
- The one measured regression risk is font atlas growth (emoji glyphs); see
  `TASKS.md`.

## Security boundaries

- No credential handling beyond passing a bot token to the keyring.
- No token is serialised into settings, logs or diagnostics. Diagnostics reports
  contain settings JSON (no secrets) plus `/proc` figures and log lines.
- No telemetry, no update checks, no background network activity. The only
  network traffic is a gateway/ REST call the user starts.
- WASM plugin loading fails closed (`plugins::load_wasm_plugin`).
