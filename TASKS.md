# TASKS.md

Legend: `[x]` done and verified, `[~]` implemented but unverified, `[ ]` not done.

## Verified working

- [x] Native egui window, guild rail, channel list, member list, message list,
  composer (`src/ui/sidebar.rs`, `src/ui/chat.rs`).
- [x] Gateway transport: TLS WebSocket, Hello/Identify, heartbeat, ACK tracking,
  RESUME with sequence + `resume_gateway_url`, INVALID_SESSION, exponential
  backoff 2 s → 60 s (`src/api/gateway.rs`). Tested with synthetic payloads.
- [x] REST send message (`src/api/discord.rs`), invoked from the composer in
  live mode.
- [x] Bot token in the OS keyring, never on disk (`src/auth.rs`).
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
- [x] 97 unit tests, clippy `-D warnings` clean, release build verified.
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
- [ ] Handle `MESSAGE_UPDATE`, `MESSAGE_DELETE`, `MESSAGE_REACTION_*`,
  `TYPING_START`.
- [ ] Rate-limit queue for REST (`X-RateLimit-*` headers, bucket per route).
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
