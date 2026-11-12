# HANDOFF.md

Read `PROJECT.md` → `CURRENT_STATE.md` → `TASKS.md` → this file, then verify the
repository state before changing anything.

## Verified state at handoff

- Path: `/mnt/storage/Default Project/rustcord`
- Branch: `main`, remote `origin` = `https://github.com/Aceweeb/rustcord`
- `cargo test --all-targets`: 92 lib + 5 bin tests passing
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build --release`: success
- Runtime: 159 MB RSS, 57–61 MB PSS, 2 threads, 0.0% idle CPU at 0.2 fps

## What changed most recently

Channel history is real and paginated, the list is virtualised, and attachments
and embeds render through a bounded image cache:

- `src/api/discord.rs` — `fetch_messages(channel_id, before, limit)` walks
  Discord's `before` cursor; `parse_history` is pure and unit-tested (ordering,
  reactions, channel-id isolation, attachments/embeds, malformed bodies).
- `src/app.rs` — `ensure_history`, `load_older`, `poll_history`, per-channel
  `ChannelHistory`. Requests run on the tokio worker and return over `mpsc`.
- `src/ui/virtual_list.rs` — variable-height row virtualisation with measured
  heights, clamping and a 10k-row performance test.
- `src/ui/chat.rs` — `show_viewport` draws only visible rows; attachments and
  embeds render with correctly-sized placeholders until their texture lands.
- `src/images.rs` — byte-budgeted LRU texture cache, Discord CDN allowlist,
  download/decode caps, background decode worker created on first request.
- `src/settings.rs` — `image_cache_mb` (default 48, clamped 8–512).

## Immediate next task

**Exercise the live path with a real bot token.** Everything up to the network
boundary is tested, but no request has ever been made against Discord, so
neither history paging nor image rendering has been seen working:

1. Put a bot token in the keyring via **Settings → Privacy → Discord connection**.
2. Confirm a guild appears, a text channel loads 100 messages, and
   "Load older messages" prepends the previous page without duplicates.
3. Post (or find) a message with an image attachment and an embed; watch the
   cache figures in **Settings → Memory** as you scroll past it repeatedly.
4. Expect `429` under rapid paging — that is the next task, not a bug.

## Then, in order

1. REST rate-limit handling: parse `X-RateLimit-*` and `retry_after`, queue
   requests per route bucket (`src/api/discord.rs`).
2. `MESSAGE_UPDATE` / `MESSAGE_DELETE` / `MESSAGE_REACTION_*` / `TYPING_START`,
   each with a synthetic payload test before touching the socket loop.
3. Packaging: icon asset + PKGBUILD.

## Rules for the next session

- Never push without running tests, clippy and a release build first.
- Never add user-token or password authentication. See `DECISIONS.md` D2.
- Never serialise tokens or write them to logs/diagnostics.
- Every new dependency must justify its RSS and binary cost. `image` (decode
  only, png/jpeg/gif/webp) and `reqwest`'s `blocking` feature are the most
  recent, both justified by attachment rendering.
- Commit and push after each verified milestone.

## Gotchas

- `/` is nearly full; `CARGO_HOME` lives there. ENOSPC build failures are about
  the registry, not the project volume.
- `egui` 0.37: `App::ui(&mut self, ui, frame)` and `egui::Panel::left/…`.
- `ScrollArea::show_rows` assumes uniform heights — the message list uses
  `show_viewport` plus `RowMetrics`. Do not "simplify" it back.
- `app.metrics` is `std::mem::take`n inside `chat::show`; image URLs are
  collected in a local inside the same closure and requested afterwards,
  because `ImageLoader::prepare` needs `&mut app`.
- History cursors are derived from the oldest message held, not stored, so
  trimming history cannot desynchronise pagination.
- **Do not call `ctx.set_visuals` unconditionally each frame.** It requests a
  repaint, so it turns the app into a spin loop. `apply_theme` gates on a
  palette hash (D12).
- **A CPU sample alone is not evidence of a regression.** Read frame counts:
  `RUST_LOG=debug` logs frames per telemetry tick; idle is 0.2 fps. Samples
  taken straight after `cargo build --release` include driver/font warm-up, and
  pointer activity over the window produces legitimate 40–50 fps bursts (D14).
- Mesa is deliberately not initialised on NVIDIA; see `DECISIONS.md` D11.

## Verification commands

```bash
cd "/mnt/storage/Default Project/rustcord"
cargo fmt --all --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
sleep 5 && scripts/measure-memory.sh 20
RUST_LOG=debug ./target/release/rustcord   # check "ui frames" lines
git status -sb && git log --oneline -3
git ls-remote --heads origin refs/heads/main
```
