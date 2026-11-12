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

## D12. Apply the theme only when the palette changes

**Context.** `RustcordApp::ui` re-applied the palette every frame so that theme
switches appear instantly. `egui::Context::set_visuals` calls `request_repaint`,
so the app scheduled another frame from inside every frame. Measured effect: it
looked idle at 0% CPU until the window was mapped, then held 3–8% of a core
while doing nothing, with PSS creeping up as frames allocated and freed.

**Decision.** Hash the palette (FNV-1a over the eight colours, polarity and name)
and call `set_visuals` only when the hash changes. The theme still updates within
one frame of a change, because a change is itself what triggers the next frame.

**Consequences.** 20-second idle sample now shows 0.0% CPU with one scheduler tick
at the 5 s sampling wake-up. The rule generalises: any per-frame call that
requests a repaint must be gated, or it becomes a spin loop.

## D13. Images are cached by bytes, not by count, and only for visible rows

**Context.** Attachments and embeds are the one part of Discord that can blow a
memory budget without bound: a channel of 2560×1440 screenshots decodes to
~14.7 MB each. A count-based cache ("keep 100 images") says nothing about RAM.

**Decision.**

- Decoded textures are accounted at `width × height × 4` and held in an LRU with
  a byte budget (default 48 MB, `Settings::image_cache_mb`, clamped 8–512 MB).
  Exceeding the budget evicts the least-recently-used texture, which drops the
  GPU texture too.
- Only rows the virtualised list actually drew request images, so scrolling past
  a large screenshot never downloads it.
- Downloads are restricted to `https://` on Discord's CDN hosts. A crafted embed
  cannot make the client fetch arbitrary URLs, and the allowlist is a pure
  function with tests rather than a comment.
- Downloads are capped at 12 MB and 4096 px on either axis; failures are
  remembered per URL so a broken embed cannot cause a retry storm.
- Decoding runs on a worker thread created on the first image request, so a
  text-only session keeps its original two threads and never pays for reqwest's
  internal runtime.

**Consequences.** Memory during image-heavy scrolling is bounded by the setting,
not by the channel. Idle footprint rose by ~1 MB (RSS 158 → 159 MB) and threads
stay at 2 until an image is actually needed.

## D14. Detect repaint loops with a frame counter, not CPU samples

**Context.** A single CPU sample cannot distinguish "the app is busy" from "the
window is under the cursor and the compositor is asking for frames". A freshly
built binary also spends its first seconds in driver and font warm-up. Twice
during this project a CPU spike looked like a regression and wasn't one.

**Decision.** `tick_telemetry` logs frames rendered since the previous tick at
debug level. Idle is ~1 frame per 5 s telemetry tick (0.2 fps); a repaint loop is
hundreds of frames per tick (a 5 s burst measured 244 frames, 48.8 fps, while the
pointer was over the window).

**Consequences.** `RUST_LOG=debug` answers "is something spinning?" in seconds
and without a profiler. A nonzero `cpu%` column is only meaningful after the
startup seconds, and must be read alongside the frame count.

## D15. The login page signs in a bot token, never an account

**Context.** A login page conventionally means email/password. Discord prohibits
third-party user-account clients, and OAuth2 that yields a user token for
messaging lands in the same place: automating a user account is what the terms
forbid, regardless of which app performs it. D2 already ruled the path out.

**Decision.** `ui/login.rs` is a credential page for a **bot token**:

- It opens on first run, when the keyring is empty. An install that already has a
  token goes straight to the workspace, so it never interrupts a session.
- Submitting checks the token *shape* (`<id>.<timestamp>.<signature>`, three
  non-empty parts, 50+ characters) before spending a request. This is a usability
  check, not authentication: a truncated paste or an application id becomes an
  immediate message instead of a 401.
- A valid token goes to the operating-system keyring, then the gateway starts. If
  the keyring is unavailable the session continues in memory only.
- "Continue in demo mode" skips sign-in.
- Settings → Privacy → **Sign out** clears the keyring entry, gateway, history
  and image cache, then returns to this page.

**Consequences.** There is one credential path in the app, and the page cannot be
repurposed to harvest an account password. Requests for account login are
declined by policy, not by accident (`HANDOFF.md`).

## D16. Rate limits are tracked per route and major id, client-side

**Context.** Discord counts requests in buckets and may answer `429` with a
`retry_after`. A client that ignores this either hammers the API or serialises
every request behind one global gate, which makes switching channels feel dead.
The reference behaviour is documented by Discord and written up by Yapfast; the
implementation here is Rustcord's own (see the licensing note at the end).

**Decision.** `src/api/ratelimit.rs`:

- A request's bucket key is its method, its path with ids templated
  (`GET /channels/:id/messages`), and the **major id** it carries. Two channels
  therefore count separately, as Discord counts them.
- `X-RateLimit-Limit` / `-Remaining` / `-Reset-After` are recorded per bucket; a
  bucket at zero waits until its window resets.
- `429` handling prefers the body's `retry_after` over `Retry-After`, treats
  `global: true` as account-wide, and retries **once**. A 429 with no timing
  information waits one second rather than hot-looping.
- A wait longer than 60 s is surfaced as an error to the caller instead of being
  slept on, so a punished account shows a message instead of appearing hung.
- Buckets are keyed per route rather than shared through `X-RateLimit-Bucket`.
  That can only be more conservative than Discord's accounting, and it keeps the
  model small enough to test purely.

**Consequences.** The arithmetic is pure (`now` is passed in), so 11 tests cover
routing, per-channel isolation, expiry, global 429s, missing headers and body
parsing without sleeping or touching the network. Behaviour against a real 429
is still unverified.

**Licensing note.** Yapfast is GPL-3.0 and Rustcord is MIT OR Apache-2.0, so no
Yapfast code was read into this file. Its published documentation of Discord's
documented limits was used as a checklist; the implementation and the tests are
Rustcord's.

## D17. Live events mutate the existing list; nothing reloads history

**Context.** Edits, deletes, reactions and typing arrive over the gateway as
partial payloads, can be replayed after a resume, and can refer to messages this
client never loaded. Handling them by refetching a channel would throw away the
virtual list's measured heights and the pagination cursor.

**Decision.**

- `MESSAGE_UPDATE` applies only the fields it carries. A missing `content` or an
  empty `embeds` list leaves what the message holds, so an unreadable partial
  payload cannot blank a message. Unknown ids are ignored: an edit must never
  create a row, which is what out-of-order delivery would otherwise do.
- `MESSAGE_DELETE` matches on id, and on channel when the event names one, then
  removes the row. A replayed delete is a no-op rather than an error.
- Reaction chips keep the count Discord reports plus a **capped list of user ids**
  (64 per emoji). The list is what stops a replayed `MESSAGE_REACTION_ADD` from
double-counting, and a duplicated remove from decrementing twice. Reactions
  loaded from REST history carry no user list, so those count down from the count
  alone. A chip disappears at zero.
- `MESSAGE_REACTION_REMOVE_ALL` and `MESSAGE_REACTION_REMOVE_EMOJI` share one
  event with an optional emoji, so there is one code path for "clear reactions".
- Typing is a pure `TypingTracker`: per channel, 8 s lifetime refreshed by each
  `TYPING_START`, capped at 32 typers. There is no timer and no thread — the
  composer asks for a 500 ms repaint only *while* somebody is typing, and the
  tracker expires entries when asked.
- Any change that can alter a row's height (edit, delete, reaction chip) clears
  `RowMetrics`, so the virtual list re-measures only the rows on screen. History
  fetching and pagination state are untouched.

**Consequences.** Live events cost O(rows held) at worst in a scan and never
touch the network. 25 tests cover the parsing, the transitions, replay and
out-of-order cases, and expiry — all without a token.

## D18. OAuth2 `identify` is a second, separate login — never a messaging one

**Context.** D2 and D15 refuse to log in as a person, because a user token makes
Rustcord a self-bot and gets accounts terminated. That is why the only login is
a bot token. Users still expect a "Sign in with Discord" button, and there is a
legitimate, documented use for OAuth2: showing *who* is signed in.

**Decision.**

- Add an **identity-only** OAuth2 login with `scope=identify`, and nothing else.
  The scope is a constant asserted in a test so it cannot be widened by accident.
- PKCE (`S256`) with **no client secret**: the verifier never leaves the process,
  only its SHA-256 leaves in the authorise URL. No secret is stored anywhere.
- `state` is 32 random bytes and is checked **before** the code is looked at, so a
  forged or stray redirect cannot inject a code. `access_denied` is surfaced as a
  cancellation, not an error.
- The redirect is a loopback listener on `127.0.0.1:53134/callback`, polled on its
  own named thread with a 180 s deadline and a cancel flag — no blocking accept.
  The browser page it serves is static HTML with no scripts or remote assets.
- The session (tokens + id/username/avatar) is JSON in the OS keyring under
  `rustcord` / `discord_oauth_identity`, **separate from** `discord_bot_token`.
  It never enters `settings.json`; diagnostics never print it; the only thing
  written to settings is the public client ID (digits only, or dropped).
- `SessionStore` is a trait with a keyring implementation and an in-memory one, so
  restoration, expiry and logout are testable without a keyring or a network.
- Restoration keeps an **expired** session visible but flagged, because forgetting
  which account it was is worse than showing a stale name.
- Logout clears only the identity; signing out of the bot also clears the identity,
  since the app is going back to its first-run state anyway. "Switch account" is
  clear-then-authorise: one account at a time, no session juggling.
- Rustcord still never asks for a password, a user token or a cookie. There is no
  scraping path and no code that reads a browser profile.

**Consequences.** Messaging is still bot-only, and the UI says so next to the
identity card. The flow needs the user's own Discord application client ID and a
registered `http://127.0.0.1:53134/callback` redirect; without them the button is
disabled and explains what is missing.

The end-to-end exchange cannot be verified here without a real application.

**Amendment — refresh tokens.** An expired session now recovers by itself when it
still has a refresh token and a client ID: one `grant_type=refresh_token` request
carries no secret (PKCE public client), then `/users/@me` re-reads the identity so
a renamed account or a new avatar is picked up. Discord rotates the refresh token
only sometimes, so a response without one keeps the stored value — dropping it
would sign the user out at the next expiry with no error to point at. The
refreshed session is written back to the same keyring entry. A failed refresh
leaves the old session visible and flagged rather than deleting it, because the
cause is usually transient and the account should stay recognisable. Tokens are
still never written to disk, so **no plaintext JSON token file exists by design**.

## D19. The gateway spends at most ten identifies an hour

**Context.** Every fresh websocket handshake costs an IDENTIFY. A client that
reconnects — revoked token, no network, a captive portal, a wrong clock — can
burn Discord's documented 1000-a-day ceiling in minutes and then be locked out
of the gateway entirely, which looks exactly like a broken app. The old ladder
was a bare `2, 4, 8 … 60` seconds with no limit and no jitter, so a fleet on the
same outage retried in lockstep.

**Decision.** `src/api/backoff.rs`:

- A sliding-window budget of **10 identifies an hour**. When it is spent, the
  cycle returns `CycleOutcome::BudgetWait` **before** sending anything, closes the
  socket, and the loop sleeps that long with the shutdown channel still selectable.
- A budget wait is **not** a failed attempt: the backoff ladder keeps its place.
- A Resume costs no allowance, because Discord does not count it as an identify.
- Each delay is scaled by a random factor in `[0.5, 1.0)`; a failed RNG lands at
  0.5 rather than at zero, and a delay can never be zero.
- The ladder resets only after a session has **stayed up for 60 s**. A handshake
  that succeeds and dies immediately keeps backing off, which is the crash-loop
  case the old code handled wrongly.
- Time is monotonic (`Instant`), never the wall clock, so a clock jump cannot
  refill or empty the budget.

**Consequences.** A permanently broken token now costs one identify every six
minutes instead of a hot reconnect loop, and the status bar says why it is
waiting. The cost is slower recovery after a long outage: at worst the app waits
out the window rather than reconnecting at once, which is the intended trade.

## D20. Search covers loaded history, and says so

**Context.** Discord's `GET /guilds/{id}/messages/search` belongs to the user
API. A bot token is refused there, and Rustcord does not hold a user token by
design (D2, D15). So "message search" has exactly two honest shapes: scan what
the client already holds, or build a local index over it.

**Decision.**

- `src/search.rs` scans the **loaded history of the open channel** newest-first
  and stops at 50 results. No index, no extra allocation beyond the hits, no
  network call, nothing that grows with message count.
- Query syntax is small on purpose: whitespace tokens are ANDed,
  case-insensitively; `from:name` or `@name` narrows to an author; a bare `from:`
  is an unfinished filter rather than a literal term, so typing it does not
  blank the results with a false match.
- Results are **snapshots** (`Hit`), not references: live events can remove a
  message between the search and the click, and a row that vanished must not be
  indexed into.
- Snippets are one line, windowed around the first match, and sliced on character
  boundaries derived from the lowercased copy, so emoji and CJK cannot panic or
  split a glyph.
- Refresh happens only when the overlay is open and something marked it stale
  (new page, live edit/delete/create). There is no per-frame scan.
- The overlay states that it searches loaded messages only, and why. The limit is
  a property of the token type, not an unfinished feature.

**Consequences.** Searching does not find messages that were never fetched, and
the copy says so instead of implying a server-side search. A real index remains
possible later, but it would have to stay inside the memory budget and be
invalidated by the same events that mark the overlay stale today.

## D23. The default look is Discord's, measured rather than approximated

**Context.** "Make it look like Discord" is easy to do badly: rounded corners and
blurple on an otherwise arbitrary layout reads as a knock-off. Discord's density
comes from a specific set of numbers and a three-step surface scale.

**Decision.**

- A new **Discord** preset is the default: `#313338` chat, `#2b2d31` sidebars,
  `#1e1f22` rail, `#5865f2` blurple, `#dbdee1` text, `#949ba4` muted, `#f23f43`
  danger, `#23a55a` online. The other twelve presets are untouched, so anyone who
  picked one keeps it.
- Discord's **metrics**, as named constants rather than magic numbers:
  `RAIL_WIDTH` 72, `CHANNEL_LIST_WIDTH` 240, `GUILD_ICON` 48,
  `CHANNEL_ROW_HEIGHT` 34, member list 240, channel header 48, message avatar 40.
- Guild icons are circles that become 16 px rounded squares when selected, with
  the white pill beside them, and channels are painted rows (inset 8 px, 4 px
  radius) instead of widget labels, because widget padding cannot hit 34 px
  exactly.
- Hover and selected fills are **derived** from the surface with
  `utils::lift`, not extra palette fields: adding two colours to every preset and
to the CSS theme mapping would be a wider change for no gain.
- Type scale follows Discord: 15 px names and body, 11–12 px timestamps and
  category headers, 16 px channel header.

**Consequences.** A CSS theme file still wins over the preset (D3-era behaviour),
so a user's `theme_file` keeps overriding these colours — the metrics and
structure apply regardless, which is why the layout change is worth more than the
palette change. Contrast tests cover the new preset like the others.

## D22. Local-only: no database, no telemetry, one allowlist

**Context.** The client is meant to run entirely on the user's machine and talk
directly to Discord. That has always been true in practice — no server component,
no bundled database, history in memory — but it was a property of the code rather
than something enforced or checkable, and "it doesn't collect anything" is worth
nothing without evidence.

**Decision.**

- `src/privacy.rs` holds the **single host allowlist**: `discord.com`,
  `gateway.discord.gg`, `cdn.discordapp.com`, `media.discordapp.net`,
  `images-ext-1/2.discordapp.net`. Exact match only, `https`/`wss` only, ports and
  userinfo stripped before comparing, so `discord.com.evil.test`,
  `evil.discord.com` and `https://discord.com@evil.test` are all refused.
- It is **enforced**, not documented: every REST call in `api::discord` and the
  gateway socket (including the `resume_gateway_url` from a payload, which is
  attacker-influenced) pass through `host_allowed` and fail closed.
- No database: message history, decoded textures and the diagnostics log live in
  memory, bounded by the resource profile and the image budget. The only files are
  `~/.config/rustcord/settings.json` (preferences only) and two keyring entries.
- No telemetry: no analytics crate is linked, no endpoint appears in the binary,
  and nothing is sent anywhere except Discord.
- `api::spotify` is the one non-Discord client in the tree and **nothing
  constructs it** (the Spotify status plugin builds its activity locally). If it is
  ever wired up, its host must be added to the allowlist and disclosed in the
  settings window first.

**How it is checked.** `src/privacy.rs` tests (7): allowlist behaviour, look-alikes,
cleartext schemes, that the OAuth endpoints are on the list, that the image CDN
list agrees, that the store list names no database or telemetry, and that the
summary states both guarantees. `scripts/check-local.sh` repeats the static half
for a human: telemetry and database crates in `Cargo.lock`, hosts in the source,
the local state list, and a scan of the release binary for analytics endpoints.

**Evidence from a run.** With no token configured the app opens no sockets at all
(`ss -tunp` shows none), and a six-second run creates neither the config nor the
data directory — the settings file appears only when something is changed.

**Consequences.** A future feature that wants a non-Discord host has to change
this module deliberately, which is the point: a stray analytics dependency or a
new service would fail the tests instead of shipping quietly.

## D21. "Log in as my Discord account" — what exists, what does not, and why

**Context.** The request keeps coming back in the form "just make Discord login
happen, find any way". It is worth answering with the actual state of Discord's
platform rather than an opinion.

**What Discord documents (checked 2026-11-12):**

- *Self-bots are forbidden.* Discord's own help article on automated user
  accounts says automating a normal user account outside the OAuth2/bot API "is
  forbidden, and can result in an account termination if found". The OAuth2 topic
  repeats it.
- *`messages.read` is not a client scope.* It exists "for local rpc server api
  access", i.e. talking to the official desktop client over its local RPC socket,
  and is otherwise restricted to channels the app itself created.
- *User-level DMs and friends need the Social SDK.* `dm_channels.read` and
  `relationships.read` require Discord approval, and the Social SDK's
  `sdk.social_layer` scope (which enables limited messaging) is approval-gated,
  governed by separate terms, and shipped as a game SDK (C++/C#/Unity), not as an
  API a Rust desktop client can simply call.
- *Bots are the supported path.* Bot tokens authenticate as an application
  account, connect to the gateway and use the REST API, with permissions granted
  when the bot is invited to a server.

**Decision.** Rustcord's login page offers every legitimate step and no
illegitimate one:

1. **Create an application** and copy its bot token (linked instructions).
2. **Invite the bot to a server** — new in this session: the page builds
   `…/oauth2/authorize?client_id=…&scope=bot applications.commands&permissions=…`
   from the same client ID, asking only for the six permissions the client uses
   (view channels, read history, send messages, embed links, attach files, add
   reactions = 117824). A token whose bot is in no server can read nothing, and
   that failure is indistinguishable from a broken app.
3. **Paste the token**, which is stored in the OS keyring.
4. Optionally **connect your identity** with OAuth2 `identify`, so the app can
   show your name and avatar.

Rustcord will not add a password form, will not read browser cookies or local
storage, will not copy tokens out of the official client's files, and will not
speak as a person on the gateway. Those are the "any way" shortcuts, and each of
them is a ban risk for the user and a terms violation for the project.

**Consequences.** If Discord ever opens user-level messaging to approved
third-party clients, the honest route is the Social SDK with its approval
process, and that is recorded in `TASKS.md` as research with its prerequisites
rather than something to hack around.

## Proposal (not implemented): multiple bot accounts

The request to connect several bot accounts at once conflicts with the current
single-session design: one keyring account (`discord_bot_token`), one
`GatewayHandle`, one `DiscordClient`, and `app.messages` assuming one channel of
one token. It is not a small change, and a half-done version would be worse than
none, so it is proposed rather than started:

1. `BotAccount { id, label, token_ref }` with one keyring entry per account
   (`discord_bot_token:<id>`), and a migration that imports the existing entry as
   the first account.
2. A `BotSessions` map: account id → gateway handle, client and per-account guild
   list, so the sidebar can show each bot's guilds under its own heading.
3. `app.messages` keyed by `(account, channel)`, or an explicit "active account"
   with the existing single-channel buffer and a switch that clears it — the
   second is far cheaper and does not touch the history machinery.
4. Identify budgets must be **per token**, not global: ten an hour across accounts
   would still be correct, but each bot's allowance is separate in Discord's
   accounting, so a shared budget would starve the second bot.

D15 is not contradicted by this (no user tokens appear anywhere); D19's budget
shape is what needs care. Until then, switching bots means sign out and sign in,
which is honest and works.

## D24. Message edit/delete are real; voice is presence only

"Add message and VC support" splits into two very different problems, and only
one of them can be done honestly with a bot token.

**Messaging.** Sending and reading already worked, and inbound
`MESSAGE_UPDATE`/`MESSAGE_DELETE` were already applied. The two gaps were
outbound **edit** (`PATCH /channels/{id}/messages/{id}`) and **delete**
(`DELETE …`, answered 204). Both are bot-token endpoints, so both are
implemented:

1. The message menu (`⋯`) appears only on the bot's own messages, because the
   invite requests no Manage Messages permission and Discord would refuse the
   rest. `RustcordApp::can_modify` is the single place that decides.
2. Editing reuses the composer in edit mode, so shortcuts, spellcheck and the
   theme all apply unchanged; Enter saves, Esc cancels, and the returned message
   replaces the row (so the `(edited)` marker is Discord's, not invented).
3. Deleting asks first. A menu click is one pixel away from a scroll gesture and
   there is no undo.
4. Content is validated client-side with the same rule Discord enforces: 1–2000
   **characters** after trimming, not bytes. `valid_content` is pure and tested.
5. Failures (403 on someone else's message, 404 after a race) land on the status
   line verbatim instead of vanishing.

**Voice.** Discord's voice *channel state* is ordinary gateway data:
`VOICE_STATE_UPDATE` says who joined or moved (a `channel_id`), and who left
(`null`). That is parsed into `GatewayEvent::VoiceState` and folded into a pure
`VoiceTracker` (one channel per user, 50-per-channel cap, leave on `null`), so the
sidebar lists the people in each voice channel exactly as Discord does. Demo mode
shows no one, because there is no demo gateway.

What is **not** implemented is voice **audio**: a WebSocket handshake to a voice
server, UDP/RTP, the Opus codec, jitter buffering, and DAVE/E2EE for the
end-to-end encryption Discord now requires. That is a codec and realtime-audio
project on the scale of the rest of this client, and half of it (mute buttons
that mute nothing, a "Join" that joins nothing) would be exactly the pretending
this project refuses. There are therefore no join/mute/deafen controls; the
sidebar shows presence, and the README says audio is absent.

**Consequences.** Editing and deleting work with the same permission set as
sending. Voice **presence** needs no extra intent (`GUILDS` is already
requested). Voice audio stays on the "not implemented" list until there is a
worker for Opus and DAVE, and `TASKS.md` records the parts.

## D25. The message context menu is the one action surface; replies are real references

D24 added edit and delete behind a small `⋯` menu inside the row. That was the
wrong place for them: Discord's own action surface is a right-click anywhere on
the message, it holds five actions rather than two, and hovering into a 16 px
button is a worse target than the row you are already reading.

So the row itself is now a click-sensing container whose `context_menu`
(`src/ui/chat.rs`) offers, in Discord's order:

1. **Quick reactions** — 👍 ❤️ 😂 🎉, one click each.
2. **Copy text** — the local clipboard, disabled when the message has no text.
3. **Reply**.
4. **Edit** and **Delete**, only for messages `can_modify` allows (the bot's own).

Every entry except Copy is a REST call, and all three carry the same lesson:
optimism needs a rollback.

- **React** optimistically adds the reaction to the row (so the chip appears
  before the round trip) and `add_own_reaction` refuses a second click, because
  `PUT …/reactions/{emoji}/@me` is idempotent server-side. If the request fails,
  `MessageAction::ReactFailed` removes the optimistic chip again and reports the
  error — a reaction that silently disappears on reload is worse than one that
  was never drawn.
- **Reply** does not fake a quote. The composer shows `Replying to @name` with
  the target's snippet, Esc or `✕` cancels, and sending includes
  `message_reference { message_id, channel_id, fail_if_not_exists: false }` —
  the same shape Discord's clients send, so a deleted target still posts. The
  reference is filtered to the channel the target lives in, since a cross-channel
  reference is rejected by the API.
- **Edit** and **Delete** are unchanged from D24.

Inbound replies are parsed from `referenced_message` (guild gateway payloads and
REST history alike) into `ReplyRef`, which is `Option` on `Message` and defaults
to `None`, so old payloads and stored shapes stay readable. A payload that only
carries `message_reference` (deleted target) shows no preview rather than an
invented author.

**Consequences.** `Message` grew a reply reference, which pushed `GatewayEvent`
past clippy's variant-size threshold; `GatewayEvent::Message` is now
`Box<Message>`, so the event channel still moves a pointer. Copy is the only
purely local action, and it is the only one that cannot fail.

**Voice is untouched by this**: no audio, per D24.
