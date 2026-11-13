# TASKS.md

Legend: `[x]` done and verified, `[~]` implemented but unverified, `[ ]` not done.

## Verified working

- [~] Search highlighting and jump-to-message (`DECISIONS.md` D50):
  `highlight_ranges` + one `LayoutJob` per row; `jump_to_message` switches channel,
  converges the scroll in ≤6 frames using measured-height averages, tints the row
  for 2.5 s and cancels on manual scroll. 2 new tests; the scroll landing and tint
  have not been seen on screen (D28).
- [x] Date filters in search (`DECISIONS.md` D49): `before:`/`after:` with
  `YYYY-MM-DD` or `YYYY-MM-DDThh:mm[:ss]`, UTC, exclusive bounds, invalid values
  kept as text, echoed in the parsed-filter line. 1 new test.
- [~] Session-wide search with channel filters (`DECISIONS.md` D45): every cached
  message across channels, newest first, with `from:name`, `in:#channel` /
  `channel:name` / `#name` filters, channel names on rows, cached-channel count and
  a parsed-filter line; clicking a result opens its channel. Parsing and narrowing
  tested; the overlay has not been typed into in a signed-in window (D28).
- [x] Animated typing indicator in the composer **and** channel header
  (`DECISIONS.md` D47): pure `typing_dots` 400 ms cycle, one shared label, 120 ms
  repaint only while visible, expiry still timer-free. 1 new test; rendering not
  seen in a signed-in window.
- [x] Typing notice renewal while a draft keeps focus (`DECISIONS.md` D48): the
  focused composer re-arms the eight-second notice, with one scheduled wake-up
  from `typing_refresh_in` rather than polling; blank drafts send nothing.
  Assertions in the existing typing test; live traffic still unverified (D28).
- [~] Outgoing typing indicator (`DECISIONS.md` D44): `POST
  /channels/{id}/typing` on the first keystroke and then at most every eight
  seconds per channel, skipped for empty drafts, non-text channels and demo mode,
  failures at debug level; inbound typing still renders above the composer. Throttle
  tested; no live call was made (no working bot session, D28).
- [~] Spotify live now-playing (`DECISIONS.md` D43, roadmap item 5): PKCE
  public-client flow on `127.0.0.1:53135`, scope `user-read-currently-playing`,
  keyring session tagged with its client id, five-second worker-thread poll with
  refresh and error de-duplication, Connect/Disconnect and status in Settings →
  Notifications, demo track replaced/cleared. Token parsing, refresh merging, URL
  and the state check are tested; no Spotify app id was available here, so the
  browser round trip and a real poll are unverified.
- [~] Folder-coloured server tiles (`DECISIONS.md` D42): members of an expanded
  folder blend 55 % toward the folder colour and get a 1.5 px ring in it, selection
  keeps the accent fill and white pill, and a closed folder tints its 2×2 preview
  cells. `utils::mix` is tested; the drawn rail has not been seen signed in (D28).
- [~] Discord Rich Presence (`DECISIONS.md` D41), Settings → Extras: official
  local IPC protocol, socket discovery for native/Flatpak/Snap/Vesktop, handshake,
  `SET_ACTIVITY` with server + optional channel, worker thread with reconnect and
  replay, opt-in switch and application id (falls back to the OAuth id). 6 pure
  tests; the transport starts and retries live, but Discord was not running here,
  so no activity has appeared on a profile.
- [~] Vencord-style extras (`DECISIONS.md` D40), Settings → Extras: per-message
  timestamps with Today/Yesterday/date (exact weekday, date and seconds in the
  tooltip) and a live `N/2000` composer counter that warns at 1800 and turns red
  past the limit. 2 pure tests; neither control has been seen in a signed-in
  window (D28).
- [~] Translucent, wallpaper-aware UI (`DECISIONS.md` D39): transparent window,
  alpha-205 themed surfaces, blurred detected wallpaper painted under all panels,
  Hyprland/KDE/GNOME/XFCE detection incl. Lua Hyprland configs, Settings →
  Appearance glass toggle and wallpaper path (`~`/`$HOME` expanded), graceful
  desktop-through fallback when no image is found. 8 wallpaper tests + 1 settings
  test; the rendered glass has not been seen (sign-in blocked, D28).
- [x] Discord-style icon: blurple squircle plus a stylised white face, original
  SVG with a rendered 256 px PNG embedded as the window icon and installed by the
  launcher script.
- [x] Folder colours (`DECISIONS.md` D38): eight-colour palette assigned to new
  folders in turn, `Folder colour` submenu with swatches plus `No colour`, stored
  normalised as `#rrggbb`, invalid values refused, white rings for expanded and
  selected, theme surface as fallback. 2 model tests; swatches not seen live.
- [x] Clickable channel peek (`DECISIONS.md` D37): rows open the channel through
  `select_channel`; the flyout survives the pointer leaving the icon via a stored
  icon+flyout rect and a 14 px reach, on the foreground layer so clicks land. 3
  tests on the pure target decision; a live click not observed.
- [x] Native message log (`DECISIONS.md` D36): edits keep old and new text,
  deletions keep the vanished text, newest first, 500-entry cap, filter and clear,
  `Ctrl+Shift+L` panel, cleared on sign-out, session-only by design. 6 model tests;
  no live edit/delete observed and the panel not seen in a signed-in window.
- [~] Server channel peek on rail hover (`DECISIONS.md` D35): non-interactable
  tooltip area with text/voice lists, unread brightening, mention counts and caps,
  positioned from the hovered icon and clamped on screen. Compiles and tests pass;
  placement not seen in a signed-in window.
- [x] Folder renaming (`DECISIONS.md` D34): `Rename folder…` in the folder menu
  opens a small anchored dialog over a draft; Save/Enter persists, Esc/Cancel
  discards, blank names are refused with a reason. 2 model tests; dialog not typed
  into in a signed-in window.
- [x] Keyboard-only navigation (`DECISIONS.md` D33): pure `Navigator` with area
  switching, wrapping and clamping, rebindable `Ctrl+Shift+K` toggle, in-place
  focus rings on rail items/folders/channel rows, guards against palettes and
  focused text fields. 6 tests; rings not seen in a signed-in window.
- [x] Guild folders and drag-free rail ordering (`DECISIONS.md` D32): flat order
  plus folder membership, context-menu move/add/ungroup, 2×2 folder tiles that
  expand in place, persisted in `settings.json`. 10 tests over the layout model;
  menus not clicked in a live session.
- [x] Ctrl+K quick switcher (`DECISIONS.md` D31): pure entry building and
  filtering, cursor wrapping/clamping, rebindable `QuickSwitcher` action, Enter /
  click activation through `select_channel`. 6 tests. Overlay itself not seen
  running: it only renders after sign-in and the available credential is rejected.
- [~] Unread/mention badges (`DECISIONS.md` D30): pure `UnreadTracker`, brighter
  channel labels with a dot, red pills on channels and guild icons, marks read
  through `select_channel`. 5 tests. **Not observed live**: no `MESSAGE_CREATE`
  arrived during a 25 s probe on the user token available here.
- [~] Guild member roster (`DECISIONS.md` D29): `GUILD_MEMBERS` intent with a
  4014 fallback, paginated `GET /guilds/{id}/members` (1000 per page, 5000 cap),
  scrollable roster panel with a message-author fallback. Parsing and the 4014
  classification are tested. **Not verified live**: the only token here is a user
  token, which REST rejects (D28).
- [x] Token failures are explained (`DECISIONS.md` D28): 401 says a user-account
  token cannot be used and points at the bot flow, 403 says the bot lacks
  permission. Verified live: the keyring held a user token, `/users/@me` was 200
  raw and 401 with `Bot`, and every message fetch was 401.
- [x] Guild list populated from READY (`DECISIONS.md` D27). The live gateway
  sends full guild objects in READY and no `GUILD_CREATE`; both are now parsed,
  deduplicated by id, and the rail scrolls. Verified against the real token:
  23 guilds, 2938 channels.
- [x] Reaction chips toggle (`DECISIONS.md` D26): clicking your own chip sends
  `DELETE …/reactions/{emoji}/@me`, clicking anyone else's sends the matching
  `PUT`; both update optimistically and roll back the correct direction on
  failure. Shared `reaction_path` keeps encoding and `@me` identical.
- [x] Right-click message context menu (`DECISIONS.md` D25): quick reactions
  (optimistic, rolled back on failure), copy text, reply with a real
  `message_reference`, plus edit/delete on the bot's own messages. Inbound
  replies render from `referenced_message`. 9 tests across models, REST and
  gateway.
- [x] Message actions (`DECISIONS.md` D24): `⋯` menu on the bot's own messages,
  edit-in-composer (Enter saves, Esc cancels) via PATCH, delete with a
  confirmation via DELETE, client-side 1–2000-character rule, and results applied
  through `poll_action_results`. 4 tests for `valid_content`.
- [x] Voice-channel presence (`DECISIONS.md` D24): `VOICE_STATE_UPDATE` parsing,
  pure `VoiceTracker`, and members listed under each voice channel in the
  sidebar. 7 tests. Voice **audio** is not part of this entry.
- [x] Native egui window, guild rail, channel list, member list, message list,
  composer (`src/ui/sidebar.rs`, `src/ui/chat.rs`).
- [x] Local message search (`DECISIONS.md` D20, `src/search.rs`): Ctrl+F overlay,
  loaded history of the open channel, `from:`/`@` author filter, 50 results,
  char-safe snippets, click-to-open. 12 tests. Server-side search is not
  available with a bot token and the UI says so.
- [x] Discord-accurate UI pass (`DECISIONS.md` D23): new default **Discord**
  preset with Discord's exact colours, and Discord's metrics as constants —
  72 px rail, 240 px channel list, 48 px guild header/icons, 34 px channel rows,
  40 px avatars, rounded-square selected guild with the white pill, painted
  channel rows with hover/selected fills derived by `utils::lift`, and Discord's
  type scale. Existing presets, CSS themes and every feature preserved.
- [x] Local-only guarantees enforced (`DECISIONS.md` D22, `src/privacy.rs`): one
  exact-match host allowlist behind every REST call, image fetch and the gateway
  socket (including resume URLs); no database, no telemetry crate, no endpoint in
  the binary. Settings → Privacy lists the local stores and hosts;
  `scripts/check-local.sh` reproduces the static checks. 7 tests.
- [x] Desktop integration: `assets/rustcord.svg` icon, `scripts/install-local.sh`
  (build → `~/.local/bin/rustcord`, launcher entry with an absolute `Exec`, icon
  into the hicolor theme, cache refresh), verified in the running session
  (`hyprctl` reports `class: Rustcord`, matching `StartupWMClass`).
- [x] Relative `theme_file` paths resolve against the config dir and the binary's
  directory before the working directory, so a launcher start does not silently
  lose the theme. 2 tests.
- [x] Bot install flow on the login page (`DECISIONS.md` D21): builds the invite
  URL from the configured client ID with only the six permissions the client
  uses, plus copy-link and open-in-browser. 3 tests pin the permission bits.
- [ ] Voice **audio** and Go Live: **blocked on DAVE** — Discord has required
  E2EE (MLS + frame-level encryption) for third-party voice since 1 March 2026 and
  rejects clients without it (close code `4017`). Requirement chain, libdave
  option and build order are recorded in `docs/VOICE_AND_STREAMING.md`. Presence
  is done and unaffected; no half-working join UI is shipped.
- [ ] Research: Discord Social SDK (`sdk.social_layer`) — the only sanctioned
  route to user-level DMs/friends, and it needs Discord approval, separate terms
  and a game SDK (C++/C#/Unity). Also `rpc`/`messages.read`, which only work
  against a running official client. Both are gated; do not fake either.
- [ ] Multiple bot accounts (`DECISIONS.md`, proposal section): designed, not
  started; needs per-account keyring entries, per-account sessions and per-token
  identify budgets.
- [x] Gateway reliability (`DECISIONS.md` D19): identify budget of 10 an hour,
  jittered backoff (50–100 %), stability-gated reset after 60 s, monotonic clock.
  11 tests in `src/api/backoff.rs`.
- [~] `cargo test --all-targets`: **169 passing** (164 lib + 5 bin), clippy clean,
  release build successful. Audited against Yapfast in `YAPFAST_PARITY.md`.
- [x] Gateway transport: TLS WebSocket, Hello/Identify, heartbeat, ACK tracking,
  RESUME with sequence + `resume_gateway_url`, INVALID_SESSION, exponential
  backoff 2 s → 60 s (`src/api/gateway.rs`). Tested with synthetic payloads.
- [x] REST send message (`src/api/discord.rs`), invoked from the composer in
  live mode.
- [x] Bot token in the OS keyring, never on disk (`src/auth.rs`).
- [x] Optional OAuth2 `identify` login with PKCE, state validation, loopback
  callback, keyring session, restoration, logout and account switching
  (`src/api/oauth.rs`, `src/ui/login.rs`, `DECISIONS.md` D18). Identity only:
  grants no messaging.
- [x] Identity refresh tokens: an expired session refreshes itself off the UI
  thread and is written back to the keyring; a rotated token replaces the old one
  and an omitted one is preserved.
- [~] OAuth2 live exchange (including refresh): implemented and unit-tested, but
  not exercised against Discord — that needs a real application client ID and the
  registered loopback redirect.
- [x] 12 theme presets across Dark/Light/Purple/AMOLED, accent override, live
  switching, contrast tests (`src/ui/theme.rs`).
- [x] CSS custom-property theme files, single-line files included
  (`src/plugins/vencord_compat.rs`).
- [x] Nine-section settings window, persisted (`src/ui/settings.rs`,
  `src/settings.rs`).
- [x] Shortcut model with rebinding and conflict detection (`src/shortcuts.rs`).
- [x] `/proc` telemetry + bounded history + budget alert (`src/sysinfo.rs`,
  `src/monitor.rs`).
- [x] Log capture ring + diagnostics checks/report export (`src/logbuf.rs`,
  `src/diagnostics.rs`).
- [x] Sign-in page for bot tokens (`src/ui/login.rs`): first-run only, token
  shape check, keyring storage, demo-mode skip, and a Sign out path.
- [x] 102 unit tests, clippy `-D warnings` clean, release build verified.
- [x] Measured: 159 MB RSS, 57 MB PSS, 2 threads, 0.0% idle CPU at 0.2 fps
  (release, documented in `CURRENT_STATE.md`).

## Next, in priority order

- [x] Investigate the 190–195 MB idle RSS. Outcome: no code regression (the
  pre-change build measured identically); ~126 MB was shared GPU driver
  mappings. Fixed by pinning libglvnd to the NVIDIA EGL vendor, which stops
  Mesa/`libLLVM` from loading: **190 → 157 MB RSS, 64.6 → 54 MB PSS**. Recorded
  in `CURRENT_STATE.md` and `DECISIONS.md` D11.
- [ ] Optional further trim: `system_fonts` pulls ~8 MB of fonts
  (wqy-zenhei, NotoColorEmoji, GoogleSansFlex). Dropping the feature would save
  that at the cost of falling back to bundled typography.
- [x] Real channel history with `before` pagination (`fetch_messages`,
  `parse_history`) and a variable-height virtualised list
  (`ui/virtual_list.rs`, `ScrollArea::show_viewport`). Parsing and layout are
  unit-tested; the network path still needs a real bot token to confirm.
- [ ] Verify the live paging path end-to-end with a bot token: first page,
  "Load older messages", duplicate-free prepend, and 429 behaviour.
- [x] Message lifecycle (`DECISIONS.md` D17): `MESSAGE_UPDATE`,
  `MESSAGE_DELETE`, `MESSAGE_REACTION_ADD/REMOVE`, `MESSAGE_REACTION_REMOVE_ALL`,
  `MESSAGE_REACTION_REMOVE_EMOJI`, `TYPING_START`. Partial payloads preserve
  fields, replayed events cannot double-count, unknown ids are ignored, and the
  typing tracker expires without a timer. 25 synthetic-payload tests.
- [x] REST rate limiting (`src/api/ratelimit.rs`, `DECISIONS.md` D16): per-route
  buckets keyed by major id, `X-RateLimit-*` accounting, global 429 handling,
  `retry_after` from the body, one retry, and a 60 s ceiling that errors instead
  of hanging. 11 pure tests; a real 429 is still unverified.
- [x] Attachments and embeds render inline through a byte-bounded LRU image
  cache (`src/images.rs`): 48 MB default budget, CDN allowlist, 12 MB / 4096 px
  download caps, LRU eviction, decode worker spawned on first request. Only rows
  the virtualised list drew are fetched. Cache tests cover budget, LRU order,
  oversized eviction, URL rejection and PNG decode accounting.
- [ ] See the image path on screen with a real message containing an attachment;
  no live Discord traffic has been exercised yet.
- [ ] Live gateway integration test with a real bot token (not possible in the
  development sandbox; requires the user's token).
- [x] Spotify now-playing in Discord Rich Presence (`DECISIONS.md` D59): playing
  tracks replace the server activity with song/artist, progress and end timestamps;
  paused/idle falls back to channel presence. 1 new test; live IPC unverified.
- [x] Discord-style UI rhythm (`DECISIONS.md` D58): grouped message blocks for
  same-author messages within seven minutes, gutter clock, ONLINE/OFFLINE member
  sections, `@` DM rail glyph. 1 new test; appearance not visually checked.
- [x] DM list loaded at startup (`DECISIONS.md` D57): `fetch_dm_channels` reads
  DMs and group DMs once per connection, synthetic rail entry always present, empty
  state, merges member-opened DMs. 1 new test; live fetch unverified (D28).
- [x] Open a DM from the member list (`DECISIONS.md` D56): `DM` button, REST
  open-or-create, synthetic Direct Messages guild, `@` labels, member fetch skip.
  1 new test; live Discord call unverified (D28), list is session-only.
- [x] Member-list avatars (`DECISIONS.md` D54): roster rows use the shared avatar
  helper and profile card; avatars share the image budget. Visual check pending
  (D28).
- [x] Signed-in avatar and profile card in the sidebar (`DECISIONS.md` D53):
  READY forwards avatar/global name/decoration, app stores the READY user id, panel
  reuses the message-row avatar helper. Visual check pending (D28).
- [x] Album art and pause indicator in the now-playing strip (`DECISIONS.md` D55):
  paused tracks stay visible with `is_playing`, frozen progress, album-art thumbnail
  and `⏸` marker; 3 new tests. Visual check pending (D28).
- [x] Now-playing strip only for real playback (`DECISIONS.md` D52): removed the
  demo track seeded at plugin init; 1 test. Visual check pending (D28).
- [x] Profile pictures and Nitro cosmetics (`DECISIONS.md` D51): custom avatars and
  Discord default avatars via `ImageLoader`, `global_name`/`banner`/`accent_color`/
  `avatar_decoration` on `User`, hover profile card. 1 new test; not yet seen on
  screen (D28), and banners only render when the payload includes them.
- [x] Example WASM plugin (`examples/wasm-uppercase`): ABI-documented `.wat`, a
  prebuilt 272-byte `.wasm`, `build_plugin` and `check_plugin` examples. Verified
  headlessly: "Hello from Rustcord — this is a plugin test." becomes uppercase.
  Not yet loaded from the live config folder or seen in the running UI.
- [x] WASM plugin sandbox (`DECISIONS.md` D46): `wasmi` interpreter, manifest with
  `deny_unknown_fields`, permission gate (`transform_messages`), no host imports,
  no `start` function, path-traversal checks, fuel (5 M default / 20 M ceiling),
  16 MiB memory and 8 KiB I/O caps, bounds-checked pointers, UTF-8 validation,
  fail-closed calls, opt-in Extras toggle, silent skip of broken folders. 15 tests.
- [~] WASM plugins on screen: load a real third-party module and see its transform
  in a sent message; only WAT fixtures and a temp-folder load test exist here.
- [x] Arch `PKGBUILD` (`packaging/PKGBUILD`): `--frozen` release build, binary,
  desktop entry, SVG/PNG icons and licence. Syntax-checked; not run through
  `makepkg`/`namcap` in this environment.
- [ ] AppImage and Flatpak: need a clean-VM build and runtime bundling.
- [ ] Packaging: PKGBUILD, AppImage, icon asset (`.desktop` entry exists in
  `packaging/`).
- [ ] Voice/Go Live: blocked on DAVE; see `docs/VOICE_AND_STREAMING.md`.
