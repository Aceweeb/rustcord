# TASKS.md

Legend: `[x]` done and verified, `[~]` implemented but unverified, `[ ]` not done.

## Verified working

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
- [ ] Voice **audio**: voice-server WebSocket handshake, UDP/RTP, Opus encode and
  decode, jitter buffer, and DAVE/E2EE. Presence is done; audio is a separate
  project and has no UI controls until it works.
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
- [ ] WASM plugin sandbox (`wasmi` + manifest + permissions), currently
  fail-closed.
- [ ] Packaging: PKGBUILD, AppImage, icon asset (`.desktop` entry exists in
  `packaging/`).
- [ ] Voice: explicitly out of scope until a compliant signalling path is
  chosen; UI affordances only.
