# HANDOFF.md

Read `PROJECT.md` → `CURRENT_STATE.md` → `TASKS.md` → this file, then verify the
repository state before changing anything.

## Verified state at handoff

- Path: `/mnt/storage/Default Project/rustcord`
- Branch: `main`, remote `origin` = `https://github.com/Aceweeb/rustcord`
- `cargo test --all-targets`: 75 lib + 5 bin tests passing
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build --release`: success
- Runtime: 158 MB RSS, 56 MB PSS, 2 threads, 0.0% idle CPU over 20 s

## What changed most recently

Channel history is now real and paginated, and the message list is virtualised:

- `src/api/discord.rs` — `fetch_messages(channel_id, before, limit)` walks
  Discord's `before` cursor; `parse_history` is pure and unit-tested
  (reversal to chronological order, reaction mapping, channel-id override,
  malformed bodies).
- `src/app.rs` — `ensure_history`, `load_older`, `start_history_fetch`,
  `poll_history`, per-channel `ChannelHistory` state. Requests run on the tokio
  worker and return over an `mpsc` channel; the UI thread never awaits.
- `src/ui/virtual_list.rs` — variable-height row virtualisation with measured
  heights, clamping and a 10k-row performance test.
- `src/ui/chat.rs` — `ScrollArea::show_viewport` draws only visible rows, with a
  "Load older messages" control and an end-of-channel marker.

## Immediate next task

**Exercise the live path with a real bot token.** Everything up to the network
boundary is tested, but no request has actually been made against Discord:

1. Put a bot token in the keyring via **Settings → Privacy → Discord connection**
   (or `DISCORD_TOKEN` is *not* read — use the UI).
2. Confirm a guild appears, a text channel loads 100 messages, and
   "Load older messages" prepends the previous page without duplicates.
3. Watch for rate limiting: rapid paging will eventually return `429`. The client
   currently surfaces the error text; the next task is a proper bucket queue.

## Then, in order

1. REST rate-limit handling: parse `X-RateLimit-*` and `retry_after`, queue sends
   and history requests per route bucket (`src/api/discord.rs`).
2. `MESSAGE_UPDATE` / `MESSAGE_DELETE` / `MESSAGE_REACTION_*` / `TYPING_START`,
   each with a synthetic payload test before touching the socket loop.
3. Attachments and embeds with a bounded image cache.
4. Packaging: icon asset + PKGBUILD.

## Rules for the next session

- Never push without running tests, clippy and a release build first.
- Never add user-token or password authentication. See `DECISIONS.md` D2.
- Never serialise tokens or write them to logs/diagnostics.
- Every new dependency must justify its RSS and binary cost.
- Commit and push after each verified milestone.

## Gotchas

- `/` is nearly full; `CARGO_HOME` lives there. Build failures with ENOSPC are
  about the registry, not the project volume.
- `egui` 0.37: `App::ui(&mut self, ui, frame)` and `egui::Panel::left/…`.
- `ScrollArea::show_rows` assumes uniform heights — that is why the message list
  uses `show_viewport` plus `RowMetrics`. Do not "simplify" it back.
- `app.metrics` is `std::mem::take`n inside `chat::show` so the closure can
  borrow `app` immutably; it is restored after the scroll area. Keep that shape
  or the borrow checker will reject the closure.
- History cursors are derived from the oldest message held, not stored, so
  trimming history cannot desynchronise pagination.
- Mesa is deliberately not initialised on NVIDIA; see `DECISIONS.md` D11.

## Verification commands

```bash
cd "/mnt/storage/Default Project/rustcord"
cargo fmt --all --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
scripts/measure-memory.sh 20
git status -sb && git log --oneline -3
git ls-remote --heads origin refs/heads/main
```
