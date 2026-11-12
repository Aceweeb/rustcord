# CURRENT_STATE.md

Last verified: 2026-11-12, CachyOS, NVIDIA (proprietary 620.70.06), Wayland session.

## Build and test

| Check | Command | Result |
| --- | --- | --- |
| Tests | `cargo test --all-targets` | **66 passed, 0 failed** |
| Lints | `cargo clippy --all-targets -- -D warnings` | clean |
| Formatting | `cargo fmt --all` | clean |
| Release build | `cargo build --release` | success |

## Measured runtime figures (not estimates)

Release binary, 1280×800 window, default settings, theme file loaded.

| Metric | Before | After | How measured |
| --- | --- | --- | --- |
| RSS | 190–195 MB | **157 MB** | `/proc/<pid>/status` `VmRSS` |
| PSS | 64.6 MB | **54 MB** | `/proc/<pid>/smaps_rollup` |
| Peak (`VmHWM`) | 196 MB | **157 MB** | kernel high-water mark |
| Idle CPU | ~0.0% | ~0.0% | ticks over 6 s |
| Threads | 2 | 2 | `/proc/<pid>/status` |

Reproduce with `scripts/measure-memory.sh 20`. The script prints RSS, PSS, peak,
CPU ticks and thread count per second.

## Why RSS was 190 MB (investigation outcome)

The old README claimed ~134 MB. It was **not** a code regression:

1. I built commit `227706e` (the pre-change baseline) in a separate worktree and
   measured it identically: **190.1 MB RSS / 64.7 MB PSS**. The current code
   measured **189.9 MB / 64.6 MB**. The two builds are indistinguishable, so the
   earlier 134 MB figure came from a different session/driver state, not from
   added features.
2. Per-library RSS showed the real cause — shared GPU driver mappings:

   | Mapping | RSS |
   | --- | --- |
   | `libnvidia-gpucomp.so` | 79,992 kB |
   | `libLLVM.so.23.1` (Mesa) | 25,316 kB |
   | `libnvidia-eglcore.so` | 21,140 kB |
   | application binary | ~8,000 kB |
   | `[anon]` + `[heap]` | ~13,000 kB |
   | fonts (wqy-zenhei, NotoColorEmoji, GoogleSansFlex) | ~8,000 kB |

   The app's own allocations were never more than ~23 MB anonymous.

3. **Fix:** `main.rs` now pins libglvnd to the NVIDIA EGL vendor
   (`__EGL_VENDOR_LIBRARY_FILENAMES`) when an NVIDIA device and vendor file are
   present and the user has not set the variable. Mesa is then never
   initialised, removing `libLLVM` and the gallium driver: **−33 MB RSS,
   −10 MB PSS**, and `libLLVM` is confirmed absent from `/proc/<pid>/maps`.

The remaining ~100 MB of RSS is the proprietary NVIDIA GL stack, which is shared
with other GL clients and is not reducible from application code. PSS (54 MB) is
the fair per-process figure; RSS overstates a GL app's cost because it counts
shared library pages in full.

## Coverage

- Unit-tested: `/proc` parsing, CPU percentage math, history eviction, settings
  roundtrip + legacy migration, shortcut parsing/matching/conflicts, all 12 theme
  presets' WCAG contrast, CSS variable extraction, diagnostics report rendering,
  log ring eviction, gateway payload parsing, EGL vendor selection policy.
- Not tested automatically: the GUI itself (no UI test harness), live Discord
  connectivity (needs a real bot token), keyring storage (no Secret Service
  provider was assumed, and none was used).

## Known issues

1. Idle RSS above the historical 134 MB figure, but the excess is the GPU driver;
   PSS at 54 MB is lower than the old RSS-based claim implies.
2. Gateway has only been exercised against synthetic payloads.
3. `Settings` uses JSON rather than SQLite (see `DECISIONS.md` D3).
4. Voice/video and streaming capture remain unimplemented; the UI shows plugin
   state only.
5. WASM plugins fail closed by design.

## Environment quirks

- Project lives at `/mnt/storage/Default Project/rustcord` because `/` filled up
  (497 GB, 4.7 GB free). A full root filesystem breaks Cargo builds and command
  execution.
- `CARGO_HOME` is still on `/` and is the next space risk.
- Mesa is deliberately not initialised; if a future environment needs it,
  unset `__EGL_VENDOR_LIBRARY_FILENAMES` before launching.
