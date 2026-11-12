# DECISIONS.md

Format: decision — context — consequences. Append new entries; do not rewrite
history unless a decision is genuinely reversed.

## D1. egui/eframe instead of Tauri

**Context.** The brief allowed Tauri or a pure Rust GUI, with a hard < 500 MB
total and a 100–200 MB idle target. Tauri pulls WebKitGTK (or WebView2) with a
separate renderer process.

**Decision.** egui + glow (OpenGL) in a single process.

**Consequences.** Measured 195 MB RSS and 2 threads; there is no renderer child
process to account for. The cost is that Discord's CSS cannot be reused and
layout must be written by hand. Vencord's CSS is therefore translated to a
palette, not executed.

## D2. Bot tokens only; no user-account client

**Context.** The overall goal was a client for a real personal account. Discord's
Terms of Service prohibit automating user accounts (self-bots); user-account API
access is not available to third parties; Rich Presence over legacy IPC is now
partner-gated by the Discord Social SDK.

**Decision.** Support bot tokens (and, in principle, approved OAuth2) only.
Reject email/password login by design. Document the limitation rather than
build a policy-violating client.

**Consequences.** Rustcord is a client for bot accounts plus local demo data, not
a drop-in replacement for the official client. This is a scope limit, not a bug.
No token scraping, no injection, no claims of being ban-proof.

## D3. JSON settings instead of SQLite (documented deviation)

**Context.** The brief suggested SQLite for local settings. Settings here are a
single small document: booleans, a preset id, an accent, a chord map.

**Decision.** Keep `settings.json` with `#[serde(default)]` on every added field,
so older configs keep loading without a migration step.

**Consequences.** Zero extra C dependency, no bundled SQLite in the binary, easy
manual editing, and trivial diagnostics. Revisit if/when a message cache or
search index is added; those are genuinely relational and belong in SQLite.

## D4. Read `/proc` directly instead of adding a `sysinfo` dependency

**Context.** The Memory page must report measured, not estimated, figures.

**Decision.** Parse `VmRSS`, `VmHWM`, `Threads`, `VmAvailable`, `/proc/<pid>/stat`
ticks and `/proc/<pid>/fd` directly (`src/sysinfo.rs`), with parsers unit-tested
against fixed strings and one live test.

**Consequences.** No dependency, exact control over units (kb → bytes) and a
documented `USER_HZ = 100` assumption. Linux-only; other platforms would need a
different implementation behind the same functions.

## D5. Bounded message buffer driven by resource priority

**Decision.** `ResourcePriority` maps to a history cap (500 / 2 000 / 10 000) and
a repaint interval (1000 / 250 / 60 ms). The UI trims the buffer after every
insert.

**Consequences.** Peak RSS cannot grow without bound on busy channels, and the
priority setting does real work instead of being a label.

## D6. Theme layering: preset → accent → CSS file

**Decision.** The preset palette is the base, the accent override replaces one
slot, and the `.theme` file overrides individual CSS custom properties on top.
An unknown preset id falls back to the default rather than failing.

**Consequences.** A theme file that only sets `--brand-experiment` keeps the
preset's backgrounds; light presets stay light under a partial CSS override.
Layer order is covered by tests in `plugins/vencord_compat.rs`.

## D7. Gateway events over `mpsc`, not shared async state

**Decision.** The gateway task owns the socket; it pushes `GatewayEvent` values
into a channel that the UI drains with `try_recv` each frame.

**Consequences.** No locks in the render path, no `await` on the UI thread, and
reconnect/backoff logic stays entirely testable in isolation.

## D8. WASM plugin loading fails closed

**Decision.** `load_wasm_plugin` returns an error. Native plugins are registered
explicitly.

**Consequences.** No false claim of sandboxing. Adding a real loader requires a
manifest, a permission model, fuel/memory limits and a `wasmi`/`wasmtime`
backend — tracked in `TASKS.md`.

## D9. No telemetry, no background network

**Decision.** No analytics, crash reporting or update checks. Diagnostics never
leave the machine; the report is written only when the user clicks export.

**Consequences.** Privacy is verifiable by reading `src/api/`. Crash reports are
the user's to send manually.

## D10. Project moved to `/mnt/storage`

**Context.** The root filesystem filled (497 GB, 4.7 GB free), which broke both
Cargo builds and local command execution.

**Decision.** Keep the working copy at `/mnt/storage/Default Project/rustcord`.

**Consequences.** Build output lives on the roomy volume. `CARGO_HOME` remains on
`/` and is the next space risk (see `docs/TROUBLESHOOTING.md`).

## D11. Report PSS alongside RSS; pin the EGL vendor on NVIDIA

**Context.** Idle RSS was 190 MB against a historical 134 MB claim. Building the
pre-change commit in a worktree and measuring it identically gave the same
190 MB, so there was no regression. Per-library RSS showed 126 MB of shared GPU
driver mappings (`libnvidia-gpucomp`, `libnvidia-eglcore`, and Mesa's
`libLLVM`), against ~23 MB of anonymous application memory.

**Decision.** Two changes rather than architectural surgery:

1. `main.rs` sets `__EGL_VENDOR_LIBRARY_FILENAMES` to the NVIDIA vendor file when
   an NVIDIA device and vendor description exist and the user has not set it.
   libglvnd then never initialises Mesa, so `libLLVM` and gallium are not mapped.
2. Measurements report PSS next to RSS, because RSS counts shared library pages
   in full and overstates a GL application's real cost.

**Consequences.** 190 MB → 157 MB RSS and 64.6 MB → 54 MB PSS, with the same
2 threads and ~0.0% idle CPU. The override is skipped on AMD/Intel systems and
whenever the user set the variable, so no non-NVIDIA setup is affected. The
remaining RSS is the proprietary driver and is not reducible from app code.
