# HANDOFF.md

Read `PROJECT.md` → `CURRENT_STATE.md` → `TASKS.md` → this file, then verify the
repository state before changing anything.

## Verified state at handoff

- Path: `/mnt/storage/Default Project/rustcord`
- Branch: `main`, remote `origin` = `https://github.com/Aceweeb/rustcord`
- `cargo test --all-targets`: 66 passing
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build --release`: success
- Runtime: 157 MB RSS, 54 MB PSS, 157 MB peak, 2 threads, ~0.0% idle CPU
- RSS investigation is closed: see `CURRENT_STATE.md` and `DECISIONS.md` D11

## Immediate next task (highest value)

1. Real channel history: `GET /channels/{id}/messages` with pagination, plus a
   virtualised list. Guard with the existing `history_cap`.
2. `MESSAGE_UPDATE` / `MESSAGE_DELETE` / `MESSAGE_REACTION_*` / `TYPING_START`
   in `src/api/gateway.rs`, each with a synthetic payload test before touching
   the socket loop.
3. REST rate-limit bucket handling in `src/api/discord.rs`.
4. Packaging: icon asset + PKGBUILD; `.desktop` entry already exists.

## Rules for the next session

- Never push without running the three checks above first.
- Never add user-token or password authentication. See `DECISIONS.md` D2.
- Never serialise tokens or write them to logs/diagnostics.
- Keep the release profile size-conscious; new dependencies should justify their
  RSS and binary cost.
- Commit and push after each verified milestone (the user asked for this
  explicitly).

## Gotchas

- `/` is nearly full. Cargo's registry lives there; a build failure with
  "No space left" is about `CARGO_HOME`, not the project volume.
- `eframe`/`egui` 0.37: `App::ui(&mut self, ui, frame)` and `egui::Panel::left/…`
  are the correct shapes. Do not "modernise" them to `update`/`SidePanel`.
- The window is created in `main.rs`; settings are applied per frame in
  `RustcordApp::ui`, so a theme change needs no restart.
- Emoji in labels is fine functionally but affects the font atlas (see above).
- Keyring requires a Secret Service provider; absence degrades to session-only
  token memory and shows as `WARN` in Diagnostics.

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
