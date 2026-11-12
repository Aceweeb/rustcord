# PROJECT.md

## What this is

**Rustcord** — a lightweight, native Discord client written in Rust + `egui`.
No Electron, no WebView, no Chromium. Single process, OpenGL via `glow`.

- Repository: `/mnt/storage/Default Project/rustcord`
- Remote: `https://github.com/Aceweeb/rustcord` (branch `main`)
- License: MIT OR Apache-2.0
- Toolchain: Rust stable (1.99 verified), CachyOS/Arch Linux

## What it is not

Rustcord **cannot** log in as your personal Discord account, and does not try.
Discord's Terms of Service and Developer Terms prohibit automating user accounts
("self-bots"), user-account API access is not available to third-party apps, and
Rich Presence over the legacy local IPC is now partner-gated via the Discord
Social SDK. Consequences, by design:

- Only **bot tokens** (Discord developer portal) or approved OAuth2 are used.
- No password/token scraping, no injection into the official client, no
  automation of account actions, no claim of being ban-proof.
- The UI renders bot-visible guilds/channels/messages, plus local demo data when
  no token is configured.

Anything that would require a user-token client is out of scope, not "unfinished".

## Goals

| Goal | Budget | Status |
| --- | --- | --- |
| Idle RSS | 100–200 MB | 195 MB measured (release, 1280×800) |
| Hard ceiling | < 500 MB | satisfied |
| Idle CPU | < 5% | ~0.0% measured over 6 s |
| Local navigation | < 100 ms | in-process state change, no I/O |
| Startup | minimal work | settings load + one theme read |

## How to build and run

```bash
cd "/mnt/storage/Default Project/rustcord"
cargo test --all-targets        # 61 tests
cargo clippy --all-targets -- -D warnings
cargo build --release
./target/release/rustcord
```

Config lives in `~/.config/rustcord/settings.json`; diagnostics exports in
`~/.local/share/rustcord/diagnostics/`. The bot token lives in the OS keyring.

## Where to look first

- `CURRENT_STATE.md` — verified numbers and what is untested.
- `ARCHITECTURE.md` — module map and data flow.
- `TASKS.md` — done / next.
- `HANDOFF.md` — exact next actions for a fresh session.
- `DECISIONS.md` — why egui, why JSON settings, why /proc sampling.
- `docs/TROUBLESHOOTING.md` — user-facing fixes.
