# TASKS.md

Legend: `[x]` done and verified, `[~]` implemented but unverified, `[ ]` not done.

## Verified working

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
