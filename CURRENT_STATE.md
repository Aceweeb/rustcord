# CURRENT_STATE.md

Last verified: 2026-11-12, on CachyOS (Arch-based), kernel session with Wayland
+ X11 available.

## Build and test

| Check | Command | Result |
| --- | --- | --- |
| Tests | `cargo test --all-targets` | **61 passed, 0 failed** |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --all --check` | clean |
| Release build | `cargo build --release` | success, 2m02s cold |
| Binary | `target/release/rustcord` | see size below |

## Measured runtime figures (not estimates)

Release binary, 1280×800 window, default settings, theme file loaded
(`themes/vencord-example.theme` → "Arch Afterglow"):

| Metric | Value | How measured |
| --- | --- | --- |
| RSS after 8 s | 195 MB (195,312 kB) | `/proc/<pid>/status` `VmRSS` |
| Peak RSS | 196 MB (`VmHWM`) | kernel high-water mark |
| RSS after 14 s | 195 MB — unchanged | second sample |
| Threads | 2 | `/proc/<pid>/status` |
| CPU while idle | 0 ticks over 6 s (~0.0%) | `/proc/<pid>/stat` utime+stime |
| RSS via `scripts/measure-memory.sh 5` | 190 MB steady, 191 MB peak, 0.0% CPU | repeatable script |
| GPU/WebView children | none | single process by design |

Notes:
- Two runs give 190 MB and 195 MB depending on when the sample starts; treat
  "190–195 MB" as the current idle figure.
- The documented figure in the old `README.md` was ~134 MB. The current build
  measures 190–195 MB. The delta is not yet explained; the leading hypothesis is a
  larger font atlas from emoji glyphs added in the settings navigation. This is
  tracked as the top item in `TASKS.md`, and the measurement is reproducible via
  `scripts/measure-memory.sh`.
- 195 MB sits inside the 100–200 MB idle target and far below the 500 MB ceiling.

## Coverage

- Unit-tested: `/proc` parsing, CPU percentage math, history eviction, settings
  roundtrip + legacy migration, shortcut parsing/matching/conflicts, all 12 theme
  presets' WCAG contrast, CSS variable extraction, diagnostics report rendering,
  log ring eviction, gateway payload parsing.
- Not tested automatically: the GUI itself (no UI test harness), live Discord
  connectivity (needs a real bot token), keyring storage on this machine (no
  Secret Service provider was assumed).

## Known issues

1. Idle RSS above the previous 134 MB baseline; cause unconfirmed (font atlas
   suspected).
2. Gateway has only been exercised against synthetic payloads.
3. `Settings` uses JSON rather than SQLite (see `DECISIONS.md`).
4. Voice/video and streaming capture remain unimplemented; the UI shows the
   plugin's state only.
5. WASM plugins fail closed by design.

## Environment quirks worth remembering

- The project moved from `~/Documents/Default Project/rustcord` to
  `/mnt/storage/Default Project/rustcord` because the root filesystem filled up
  (497 GB, 4.7 GB free). `/` being full also broke command execution tooling.
- `CARGO_HOME` still lives on `/`; a full root disk will break builds first.
