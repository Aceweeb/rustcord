# HANDOFF.md

Read `PROJECT.md` → `CURRENT_STATE.md` → `TASKS.md` → this file, then verify the
repository state before changing anything.

## Verified state at handoff

- Path: `/mnt/storage/Default Project/rustcord`
- Branch: `main`, remote `origin` = `https://github.com/Aceweeb/rustcord`
- `cargo test --all-targets`: 164 lib + 5 bin tests passing
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
- `src/ui/login.rs` — first-run bot-token sign-in page with a shape check, a
  demo-mode skip, and a Sign out path in Settings → Privacy (D15).
- `src/api/ratelimit.rs` — per-route/per-major-id buckets, `X-RateLimit-*`
  accounting, global 429s, one retry, 60 s ceiling (D16).
- Message lifecycle events (`MESSAGE_UPDATE`, `MESSAGE_DELETE`,
  `MESSAGE_REACTION_*`, `TYPING_START`) with pure list operations in
  `models.rs` and a typing tracker; 25 tests, all synthetic (D17).
- OAuth2 identity sign-in (`identify` + PKCE, keyring session, restoration,
  logout, account switching); 18 tests (D18).
- Gateway reliability (`src/api/backoff.rs`): ten identifies an hour, jittered
  delays, 60 s stability gate; 11 tests (D19).
- `YAPFAST_PARITY.md`: the feature audit against Yapfast, including what is
  refused by decision rather than missing.

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

## Installed for this user

`scripts/install-local.sh` put the release binary at `~/.local/bin/rustcord`, the
launcher entry at `~/.local/share/applications/rustcord.desktop` (absolute `Exec`,
`StartupWMClass=rustcord`) and the icon in the hicolor theme. **Re-run it after
any change the user should see in the launcher**, and commit after the usual
checks. The entry was verified with `desktop-file-validate`, a launch from
`$HOME`, and `hyprctl clients` reporting `class: Rustcord`.

## Then, in order

1. OAuth2 identity: verify the live exchange — sign-in **and** refresh — with a
   real application (client ID + `http://127.0.0.1:53134/callback` redirect).
   Ports 53134, the client-ID normalisation and a forced expiry are the things to
   test first. Both paths are unit-tested but have never spoken to Discord.
2. Local message search over loaded history (`YAPFAST_PARITY.md` §7). Discord's
   guild search endpoint needs a user token, so a server-side search is not an
   option: index what is in memory, keep it bounded, and say in the UI that it
   searches loaded history only.
3. Per-message context menu (edit, delete, copy, react, reply) so the lifecycle
   events above are not only inbound.
4. Packaging: icon asset + PKGBUILD.

## Rules for the next session

- Never push without running tests, clippy and a release build first.
- Never add user-token or password authentication. See `DECISIONS.md` D2 and D15.
  This includes OAuth2 flows that yield a user token for messaging: refusing these
  requests is deliberate, because the cost is the user's account.
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
- Two logins exist and must not be confused: the bot token (gateway + REST
  messaging) and the OAuth2 identity session (display only, scope `identify`).
  Keyring accounts are `discord_bot_token` and `discord_oauth_identity`; no
  client secret is stored because the flow is PKCE.
- Live events must never refetch history: they mutate `messages` and clear
  `RowMetrics` so heights are re-measured. Pagination state lives in `history`
  and is deliberately untouched.
- `Reaction.users` is capped at 64 ids and is empty for REST history; do not
  assume it lists everyone who reacted.
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
