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
- [x] 61 unit tests, clippy `-D warnings` clean, release build verified.
- [x] Measured: 195 MB RSS, 2 threads, ~0.0% idle CPU (release, documented in
  `CURRENT_STATE.md`).

## Next, in priority order

- [ ] Reduce idle RSS toward the 100–200 MB band's lower half. Hypothesis:
  emoji glyphs in the settings nav and sidebar pull a large font atlas into
  memory. Test by replacing emoji with ASCII glyphs and re-measuring with
  `scripts/measure-memory.sh`.
- [ ] Fetch real channel history (`GET /channels/{id}/messages`) with
  pagination, and virtualise the message list before loading thousands of rows.
- [ ] Handle `MESSAGE_UPDATE`, `MESSAGE_DELETE`, `MESSAGE_REACTION_*`,
  `TYPING_START`.
- [ ] Rate-limit queue for REST (`X-RateLimit-*` headers, bucket per route).
- [ ] Attachments and embeds: render images lazily with an LRU cache and a hard
  memory ceiling.
- [ ] Live gateway integration test with a real bot token (not possible in the
  development sandbox; requires the user's token).
- [ ] WASM plugin sandbox (`wasmi` + manifest + permissions), currently
  fail-closed.
- [ ] Packaging: PKGBUILD, AppImage, icon asset (`.desktop` entry exists in
  `packaging/`).
- [ ] Voice: explicitly out of scope until a compliant signalling path is
  chosen; UI affordances only.
