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

## D26. Reaction chips toggle: PUT to add, DELETE to take ours back

D25 rendered reactions as plain labels, so the only way to react was the context
menu — which can only add, never remove. Discord's chips toggle, and the API for
the other direction already exists: `DELETE` on the same `…/reactions/{emoji}/@me`
path that `PUT` adds through. Both methods now share `reaction_path`, so the
encoding and the `@me` suffix cannot drift apart, and an empty emoji is refused
there rather than turning into a request for the whole reaction list.

Behaviour:

- Clicking a chip we are not part of adds our reaction (`PUT`).
- Clicking a chip we **are** part of removes it (`DELETE`). When other people
  reacted too, the chip stays with a count one lower and `me = false`; when we
  were the only one, it disappears.
- Both directions are optimistic and both have a rollback: `ReactFailed` carries
  `removed: bool`, so a failed `DELETE` re-adds the reaction and a failed `PUT`
  removes it. Without that flag the rollback would undo the wrong direction.
- `remove_own_reaction` never decrements below zero and only touches chips where
  `me` is true, so a failed removal cannot eat someone else's count.

The chips are now real buttons inside the row's click-sensing container from
D25, which raised the obvious risk: does the container swallow clicks meant for
its children? That was checked with a temporary headless `egui` harness
(`Context::run_ui` with synthetic pointer events); the child button received the
click and the container did not, so chips are safe as ordinary widgets. The probe
was deleted after it answered the question — it tested `egui`'s hit-testing, not
this project.

**Consequences.** A chip now means "click to toggle", as in Discord. The context
menu's quick reactions remain the add path for messages with no chips yet
(Discord hides the chip row until someone reacts, and so does this client).

## D27. Guilds come from READY as well as from GUILD_CREATE

**Symptom.** The status bar read "Ready as … · 23 guild(s)" while the guild rail
was empty.

**Investigation, on the wire rather than by reading code.** A probe connected to
`wss://gateway.discord.gg/?v=10&encoding=json` with the same token and intents the
client uses, identified, and printed event names for 12 seconds: `READY`,
`CHANNEL_UPDATE`, `CHANNEL_UNREAD_UPDATE` — and **zero `GUILD_CREATE`**, with
`1` (GUILDS) as the only intent as well as with the full default set. The READY
payload itself contained 23 guild objects, `unavailable: 0`, each with its
`channels` and `roles` arrays. So this gateway session delivers guild data
*inside READY* and never sends the per-guild events the client waited for.

The client had been reading `d.guilds` only to count it
(`GuildEvent::Ready { guild_count }`) and discarding the payload, which is why the
count was right and the rail was empty.

**Fix.** READY now also feeds each complete entry through the same `parse_guild`
that `GUILD_CREATE` uses and emits a `GuildCreate` per guild, so downstream code
has one code path and one shape. Entries with `unavailable: true` are skipped,
because they carry no name or channels. `GUILD_CREATE` handling stays, since a
resume or a large-guild session really can deliver guilds that way; when both
paths name the same guild, `RustcordApp` updates the existing entry instead of
pushing a duplicate (and skips channels whose id it already has).

**Second defect found on the way.** The rail had no scroll area. 23 icons at
56 px do not fit an 800 px window, so even with data the bottom guilds were
unreachable; `guild_rail` now wraps its column in a `ScrollArea`.

**Verified** with the real token through the client's own `GatewayClient`: READY
reports 23 guilds and 23 `GuildCreate` events follow, carrying 2938 channels in
total. The first version cloned READY's `guilds` array before iterating it, which
cost 110 MB of transient RSS for this account (270 MB against the 250 MB target);
borrowing the array instead measured **156 MB** in the same 12-second run. The probe was deleted afterwards; the regression test is
`ready_carries_whole_guilds`, built from the observed payload shape.

## D28. The stored credential is a user token; REST stays bot-only, and the 401 says so

**Report.** "Works, but I don't see messages and members." The guild rail was
populated and the channel list rendered, yet every channel was empty and the
member panel showed nothing.

**Investigation.** A probe read the token from the keyring and sent
`GET /users/@me` with three header shapes:

| Header | Result |
| --- | --- |
| `Authorization: Bot <token>` | **401 Unauthorized** |
| `Authorization: <token>` (raw) | **200 OK**, `"username":"rpent0055"` |
| `Authorization: Bearer <token>` | 401 |

That is the signature of a **user-account token**, not a bot token: bots
authenticate with `Bot …` and are rejected without it, user tokens are the
reverse. A follow-up probe confirmed the consequence — every message fetch
returned `HTTP 401 Unauthorized` for all sampled channels, so `app.messages`
stayed empty, and since the member panel is derived from message authors it was
empty too. The gateway had accepted the session (which is why guilds appeared),
but the REST API will not serve a user token through the bot scheme.

**Decision: do not support it.** D2, D15 and D21 already forbid user-account
tokens and self-bot behaviour: automating a user token is a ban risk for the
account, a terms violation, and the reason those decisions exist. Making
messages appear would mean sending the token raw — the self-bot path this
project refuses. The correct fix is a bot token whose bot is in the guild, which
is the documented install flow.

**What changed instead.** A 401 no longer surfaces as an opaque "HTTP 401" in a
mostly-empty window. `error_for_status` maps it to a sentence that names the
cause ("a user-account token cannot be used here — install a bot token instead,
see D2"), 403 to the missing-permission case, and everything else keeps the old
text. The member panel now says when there is nothing to list and why: members
come from message authors, and a full member list needs the privileged Members
intent, which this client does not request.

**Limits.** With a valid bot token, message history loads and the panel fills
with the authors in that channel. It is not the full guild roster; that needs
`GUILD_MEMBERS`, which is a privileged intent — a deliberate non-goal for now,
recorded in `TASKS.md`.

### D28 addendum: the failure was invisible

The 401 explanation never reached the screen. `chat::show` returned early for an
empty channel — before the row that renders `ChannelHistory::error` — so a
channel that failed to load drew exactly the same "No messages in this channel
yet." as a quiet one. That is why the app looked half-working rather than
broken: guilds came from the gateway, and the REST error that explained the rest
was skipped by an early return.

`history_controls` now runs before the empty check and returns the error, so an
empty channel shows the 401 sentence (wrapped, on its own line) instead of the
reassuring message. Errors for non-empty channels are unchanged but no longer
clipped into a single row.

This does not make a user token work, and is not meant to: it makes the refusal
legible where the user is looking.

## D29. Guild roster via the privileged Members intent, with a fallback

The member panel used to be derived from message authors, so a quiet channel had
an empty panel and there was no way to see who is actually in the guild.

**Gateway.** `DEFAULT_INTENTS` now includes `GUILD_MEMBERS` (1 << 1). That intent
is privileged: if "Server Members Intent" is not enabled for the application,
Discord closes the socket with **4014 (Disallowed intent(s))** and the whole
gateway dies — guilds, channels and messages with it. So a 4014 is not treated as
a network failure: `run_gateway` keeps a mutable intent mask, drops the member
bit, emits `GatewayEvent::MemberIntentMissing`, invalidates the session, waits one
second and identifies again. Without the bit the client behaves exactly as
before; the roster is simply unavailable, and the panel says which toggle to
enable. `is_disallowed_intents` is unit-tested against 4014, 4004, 1000 and a
missing code.

**REST.** `DiscordClient::fetch_members` walks
`GET /guilds/{id}/members?limit=1000&after=<last user id>` until a short page,
capping at 5000 members. Pagination depends on Discord's id ordering, so
`parse_members` deliberately does **not** sort — the last id of the raw page is
the cursor — and deduplicates while dropping entries without a `user` object.
Display sorting happens in the UI, case-insensitively by username.

**UI.** The panel shows the roster when it has one, falls back to the distinct
authors of loaded messages when it does not, and otherwise shows why: the
Developer Portal toggle, "loading members…", or "connect a bot token". It scrolls,
because a 5000-member guild does not fit 240 px.

**Verification limits.** With the token in this machine's keyring the gateway
accepted the member intent and delivered READY plus 23 guilds, so the fallback
path could not be exercised live; it is covered by the close-code test. The REST
listing itself returned 401 for the same user-token reason as D28, so no live
roster was ever fetched. Both halves need a real bot token with the intent
enabled to be proven end to end, and that is recorded in `TASKS.md` rather than
claimed as tested.

## D30. Unread and mention badges are local, because bots have no read state

Discord's unread model is server-side read state; bots do not have one. There is
no endpoint that says "3 unread, 1 mention" for a bot, and no `CHANNEL_UNREAD_UPDATE`
that means anything for one. Faking a count from `last_message_id` would be wrong
the moment the app restarts, so the counters are explicitly **local**: what this
process has seen since each channel was last opened.

**Tracking.** `UnreadTracker` (pure, in `models.rs`) keeps `{ messages, mentions }`
per channel id:

- `observe(channel, is_mention, is_open, is_own)` ignores the channel currently on
  screen (opening a channel is reading it), ignores our own messages, and counts
  everything else. Counts saturate rather than overflow.
- Mentions are `<@id>`, `<@!id>` for the bot's own id, plus `@everyone`/`@here`,
  by `mentions_user` — deliberately simple text matching, not a parser.
- `mark_read` drops the entry; every path that opens a channel now goes through
  `RustcordApp::select_channel`, so a click, keyboard cycling, search jump, guild
  switch and the first channel from READY all clear it.
- Badges cap at `99+` (`UNREAD_BADGE_CAP`), and `guild_mentions`/`guild_unread`
  aggregate a guild's channels for the rail.

**Drawing.** A channel with unread messages gets Discord's brighter label and a
small dot at the left edge; mentions add the red pill with the count at the right.
A guild icon shows the summed mention pill at its bottom-right, or a white dot
when there are unread messages but no mentions. No pill is drawn for the channel
you are looking at, because it was marked read.

**Honest limits.** Nothing accrues while the app is not running, so the first
launch after a week shows a clean slate — that is the correct behaviour for a
client with no read state, and better than inventing counts from message ids.
Mentions are only detectable when `MESSAGE_CONTENT` is enabled for the bot
(without it Discord sends empty content, so the pill never appears). Live
verification was not possible: the only credential here is a user token, and a
25-second gateway probe saw no `MESSAGE_CREATE` at all, so the badges are tested
through `UnreadTracker` and not observed on screen.

## D31. Ctrl+K is a local switcher, not Discord's

Discord's quick switcher is a client feature, not an endpoint, so there is nothing
to call: it jumps between the guilds and channels the client already holds. That is
exactly what this one does, which also makes it cheap — a few thousand rows scored
per frame, no index, no network, no memory held after the overlay closes.

**Model** (`switcher.rs`, pure and tested): `build_entries` emits servers first and
then each server's text and voice channels in the order received (categories are
not jump targets, since you cannot read one). `filter` requires every
whitespace-separated query token to appear in `"#name guild"`, ranked prefix match,
then substring, then the rest, breaking ties alphabetically; an empty query lists
the first 12, which is what a switcher shows before you type. `SwitcherState`
owns open/query/cursor, wraps the cursor at both ends, and clamps it whenever the
result list shrinks under a narrowing query.

**Binding.** `QuickSwitcher` is a normal `ShortcutAction` (default `Ctrl+K`), so it
appears in the shortcuts page and can be rebound. Opening it closes the message
search and vice versa — one palette at a time. The duplicate-shortcut test used
`Ctrl+K` as its arbitrary clash; it now uses the unbound `Ctrl+J`, so the test
still checks conflict reporting rather than colliding with a real default.

**Activation.** A channel row opens it and selects its server; a server row opens
that server's first text channel. Both go through `select_channel`, so a jump also
clears the channel's unread badge (D30). Enter takes the highlighted row, a click
takes the row clicked, `Esc` or `Ctrl+K` again closes.

**Not verified on screen.** The overlay only appears after sign-in, and the only
credential here is a user token whose REST calls are rejected (D28), so I checked
that the binary launches and stays up, plus the filtering/cursor tests — not the
overlay drawn over a live session.

## D32. Guild folders and ordering live in `settings.json`

Server folders are part of a user's own Discord settings; a bot token cannot read
or write them, and `PATCH /users/@me/settings` is a user-API call this client
deliberately does not make (D2/D15). So the rail layout is local, persisted with
the rest of the settings as `guild_rail: { order, folders }`.

**Why a flat order plus membership, not a nested tree.** A tree needs a rule for
every guild that joins or leaves; a flat list of guild ids and a list of folders
holding guild ids degrades instead. `items()` drops ids that no longer exist,
appends guilds it has never seen in received order, and emits a folder at the
position of its first member, once. A stale folder with no remaining members is
kept in the config (so rejoining restores it) but never drawn.

**Drag-free ordering.** Dragging is not the only honest way to move a server, and
it is the part of a rail that is easiest to get wrong with a virtualised list, so
the controls are explicit: right-click a server for `Move up` / `Move down`, `Add
to folder ▸`, `New folder with this server`, and `Remove from folder` when it is
already grouped. A folder tile's own menu offers `Move up` / `Move down` /
`Ungroup folder`. Moving a member moves its whole folder block, which is what
Discord does; moving the block rewrites the flat order from the swapped item list
so the two representations never disagree.

**Drawing.** A folder is a rounded square holding up to four member initials in a
2×2 grid (Discord's own treatment), accent-filled while open or when it contains
the selected server. Clicking toggles expansion; members render as normal icons
with a 6 px indent, keeping their unread badges (D30). Expansion is session-only,
so folders open collapsed on the next launch. Every layout change saves through
`settings_changed()`, which normalises and rebuilds the theme as usual.

**Limits stated plainly.** This layout is per machine and per config file; it does
not sync to Discord and is not visible to anyone else. Folder names default to the
server's name and there is no rename UI yet — a follow-up, not a hidden feature.
The rail and its menus were tested through `GuildRail` (10 tests) and by launching
the release binary; the menus themselves have not been clicked in a live session.

## D33. Keyboard navigation is a mode with an in-place focus ring

The quick switcher (D31) needs typing. Keyboard-only browsing does not: Discord's
own "keyboard navigation" moves a focus through the rail and channel list and
opens with Enter, with no overlay at all. That is a different feature, so it is a
separate mode (`nav.rs`) rather than another use of the switcher.

**The model is pure.** `Navigator` owns only `active`, `area` (rail or channels)
and the two cursors. `handle(key, rail_len, channel_len)` reports `Moved`,
`SwitchArea`, `Activate`, `Exit` or `Ignored`, and the app decides what
"activate" means: a folder toggles its expansion, a server selects itself and
moves the focus into its channels, a channel opens through `select_channel` (so
it is marked read, D30). Cursors wrap and are clamped against the live list sizes
every key, so a guild joining, a folder being ungrouped or a channel list
shrinking can never leave the focus pointing past the end.

**Keys.** Up/Down move within the current list; Right enters the channel list (and
is refused when the server has no channels); Left returns to the rail; Enter opens;
Escape leaves. The mode is toggled by `KeyboardNavigation`, a normal rebindable
shortcut, default `Ctrl+Shift+K`, and it starts on whatever is already selected.

**Guards.** The mode yields to anything that owns the keyboard: while settings,
search or the switcher are open it does nothing, and it stands down entirely when
any widget holds keyboard focus, which is the case while the composer is being
typed in — otherwise Enter would send a message *and* activate the focus. Opening
a palette, clicking a server or clicking a channel turns the mode off, because a
pointer has taken over.

**Drawing.** The focus is a ring drawn in place: white around a rail icon or folder
tile (readable on both the idle and accent fills), accent-coloured around a channel
row. The rail focus is computed by rail *item* index, so a collapsed folder is one
stop and its expanded members are none — matching what is on screen.

**Not verified live.** Toggling, arrows and activation are covered by `Navigator`
tests (6) and the code compiles/clippy/tests clean, and the release binary starts;
the rings themselves have not been seen in a signed-in window, for the same reason
as the switcher (D31): the only credential here is a user token REST rejects.

## D34. Folder names are edited in a small dialog, not inline

A folder is created named after the server that started it (D32), which is a
sensible default but not always the name you want. Renaming therefore has to be
possible without dragging, and the rail is 72 px wide: an inline text field there
would be unreadable and would fight the click-to-expand tile. So `Rename folder…`
in the folder's context menu opens a small anchored dialog instead.

**Draft, not live edits.** `FolderRename` holds the folder id, the draft and a
one-shot focus flag; nothing is written to `settings.json` until Save (or Enter).
Cancel and Escape drop the draft. This matters because `settings_changed()`
rebuilds the theme and writes the file — doing that per keystroke would be both
wasteful and, if the name were later abandoned, wrong.

**Validation is honest.** `GuildRail::rename_folder` trims the name and refuses a
blank one rather than substituting a default, so the dialog can keep the field
open, disable Save and say "A folder needs a name." rather than silently turning a
deliberate blank into "Folder". Renaming an unknown folder id returns false and
changes nothing.

**Interaction with the other modes.** Arrow-key navigation (D33) stands down while
this dialog is open, and the dialog's own field takes keyboard focus, so Enter
saves the name instead of activating a rail item behind it. The folder tile's
tooltip and the item list pick the new name up on the next frame because both read
from `settings.guild_rail`.

**Not verified live.** Covered by model tests (rename, trimming, blank refusal,
unknown id, draft initialisation) plus clean fmt/clippy/tests and a launch of the
release binary; the dialog has not been typed into in a signed-in window, for the
same credential reason as D31 and D33.

## D35. The rail peek is a non-interactable tooltip area

Discord shows a server's channels when you hover its icon, so you can look without
switching. The rail is a 72 px panel and the peek must spill over the channel list,
so it cannot be a widget inside the panel: `hover_peek` draws an `Area` in
`Order::Tooltip`, positioned from the hovered icon's own rect. The icon reports
hover back through `guild_row`'s return value rather than a shared field, which
keeps the row helper free of yet another `&mut` parameter.

**Content.** Server name, then `TEXT CHANNELS` and `VOICE CHANNELS` with `none` when
a group is empty, unread names brightened and mention counts appended (D30), and
`and N more` when a server has more than 8 text or 5 voice channels — the same cap
that keeps the panel from running off screen. Position is clamped to the window
bottom so a hover near the floor does not push the peek out of view.

**Deliberately inert.** The area is `interactable(false)`: the point is to look, and
a click or a context-menu right-click must still reach the icon underneath. The peek
is suppressed while keyboard navigation, search, the switcher, settings or the
rename dialog are up, because each of those is already the thing on screen.

**Cost.** It clones the guild name and the two channel name lists for one frame only
while hovering, and reads unread counters without cloning them. Nothing is cached,
so a channel created or renamed elsewhere is current the next time you hover.

**Not verified visually.** Compiles, clippy/tests clean (245 tests unchanged) and the
release binary starts, but the peek's placement and legibility have not been seen in
a signed-in window — the same credential blocker as D31/D33/D34. It is only drawn
when a server icon is hovered, which requires a live session.

## D36. Vencord plugins cannot load here; the message log is native

Vencord is a client mod for Discord's *own* client: its plugins are TypeScript
modules that patch Discord's web bundle at runtime (and it is GPL-3.0). Rustcord is
a from-scratch Rust client that talks to the API with a bot token, so there is no
bundle to patch and no plugin ABI to satisfy. Running a Vencord plugin would mean
embedding a JavaScript runtime and Discord's client code, which is neither possible
nor appropriate here; copying Vencord plugin source would also pull GPL code into
this project. So no Vencord plugin is loaded or vendored — what is implemented is
the *feature* those plugins provide, written natively.

**What MessageLogger does and what this does.** Vencord's MessageLogger keeps the
content of messages that are edited or deleted, because the client already received
it. Rustcord already receives `MESSAGE_UPDATE` and `MESSAGE_DELETE` on the gateway
and already holds the previous message, so `message_log.rs` snapshots the old
content before applying the update or removing the deleted message and appends an
entry: kind, message id, channel, author, old text, new text (edits only), time.

**Session only, deliberately.** Entries live in memory, newest first, capped at
`MAX_ENTRIES` (500) with the oldest evicted. Nothing is written to disk: persisting
other people's deleted messages is a privacy decision this client does not make on
the user's behalf, and it would add a new file to defend. That means the log is
empty again after a restart, and it only ever contains messages this session
actually saw — a message deleted before launch, or in a channel never opened,
leaves nothing to record and the UI says so rather than implying it was never
deleted.

**Surface.** `Ctrl+Shift+L` (`MessageLog`, rebindable) opens the panel; it lists
newest first with the channel name resolved from the loaded channel list, an author
and timestamp line, the old text, and the replacement text after an arrow for
edits. A filter box matches author, channel id and both contents; Clear empties the
log; Escape or Close hides it. Signing out clears it. The mode yields the keyboard
to search, the switcher, settings, the rail rename dialog and keyboard navigation,
exactly like the other panels.

**Verified.** Model tests cover edits keeping both versions, unchanged updates
being ignored, deletions keeping content, newest-first order, the cap evicting the
oldest, filtering and clearing, and the log is cleared on sign-out in code. The
panel itself has not been looked at in a signed-in window, and no live edit or
delete has been observed because Discord rejects this environment's credential
(D28); the wiring is compile-, clippy- and test-verified only.

## D37. The peek's rows are clickable, which means it must survive losing hover

D35 drew the peek as an inert tooltip precisely so it could not steal input. Making
its channels openable changes that: the rows need clicks, and, more subtly, the
peek has to stay alive while the pointer travels from the server icon onto the
flyout. Hovering the icon ends the moment the pointer leaves it, so a purely
hover-driven flyout would vanish in the gap — the click could never land.

**Two rects, not one flag.** `Peek` records the server, its icon rect and the
flyout's own rect for the frame it was drawn. `peek_target` decides what to show:
an icon under the pointer always wins (hovering another server switches the peek
immediately), otherwise the previous flyout is kept only while the pointer is
inside its rect expanded by `PEEK_REACH` (14 px, enough to cover the gap). Drop
outside that and the state is cleared, so nothing lingers after the pointer leaves.

**Layer and inputs.** The area moved from `Order::Tooltip` to `Order::Foreground`
so its rows can receive clicks; tooltip-order areas are not meant to be
interactable. Rows are labels with `Sense::click()` and a pointing-hand cursor,
with "Open this channel" on hover. Clicking one selects the parent server, opens
the channel through `select_channel` (so it is read, D30), and stops keyboard
navigation. The peek covers the part of the channel list it overlaps, which is
expected: it is a deliberate flyout, and leaving it closes it.

**Guards unchanged.** Search, the switcher, settings, the rename dialog, keyboard
navigation and the message log all still suppress the peek and clear its state.

**Tested without a pointer.** `peek_target` is pure and covered by three tests: a
hovered icon wins, the flyout stays open (with its original icon anchor) while the
pointer is on it including the gap, and leaving closes it. Actually clicking a row
in a live window has not been observed — this environment's credential is rejected
by Discord (D28), so the panel is not reachable signed in.

## D38. Folder colours are palette-first, stored as `#rrggbb`, drawn as identity

Grouped servers all look alike in a 72 px rail — four initials in a dark rounded
square. A colour per folder makes the groups readable at a glance, so it is stored
with the rest of the local rail layout (`GuildFolder::color`, D32/D34) and drawn on
the tile itself.

**Palette-first, not a colour picker.** `FOLDER_COLORS` is eight Discord-ish
colours; `new_folder` assigns the next one by folder count, so two folders created
back to back are different colours without the user touching anything, and the
ninth wraps rather than running out. The folder menu's `Folder colour` submenu
offers those eight swatches (labelled with a coloured dot, not just the name) plus
`No colour` for the theme's own surface. A full picker would add a dialog and a
validation surface for a cosmetic choice; the palette covers "tell these apart"
with one click.

**Stored form and validation.** The value is `#rrggbb` or absent, normalised through
`parse_css_color`/`color_to_hex` on write, so `#5865F2` and `#5865f2` cannot become
two different-looking entries in `settings.json`. An unparsable value is refused
rather than defaulted — a hand-edited typo leaves the folder unchanged, and a value
that somehow reaches the drawing code without parsing falls back to the theme
surface instead of painting something arbitrary.

**Expansion and selection stay legible.** The fill is always the folder's colour, so
its identity never changes as you click; expansion adds a 1.5 px white ring and a
folder holding the selected server gets a 2 px ring. Initials stay white on every
palette entry, which is why the palette stops at mid-dark colours.

**Not verified visually.** Colour assignment, normalisation, clearing, invalid
input and the serde roundtrip are covered by tests (2 new), and the binary starts,
but the swatches have not been seen in a signed-in window — same credential blocker
as D34/D37.

## D39. Translucency is real window alpha plus an in-app blurred wallpaper

The window is created with `with_transparent(true)` and `clear_color` returns
`[0, 0, 0, 0]` while glass is on, so whatever the compositor has behind the window
shows through: on Wayland that is the actual desktop wallpaper, live, at no
copying cost. Panels use the active theme's colours with alpha 205 (`GLASS_ALPHA`,
`AppTheme::veiled`), which leaves text contrast intact while the wallpaper reads
through. Turning `translucent` off restores opaque panels and an opaque clear
colour, so the two modes are one switch rather than two code paths.

Relying on the compositor alone would leave the app's own look unblurred on
compositors that do not blur windows, so the app also paints a blurred copy of the
detected wallpaper on the background layer (below every panel, above the clear
colour). `src/wallpaper.rs` finds the image, downscales it to 320 px wide, applies
three box-blur passes, and uploads one texture named `desktop-wallpaper`;
`cover_uv` keeps its aspect ratio while covering the window. Detection order is
the explicit `wallpaper` setting, then Hyprland (`hyprctl hyprpaper listactive`,
`swww query`, `hyprpaper.conf`, `exec-*`/`output` lines in `hyprland.conf`, and a
recursive scan of `~/.config/hypr`'s small text files for any token that looks
like an image path), KDE, GNOME and XFCE. Every parser is pure and tested; a
failure to find or decode an image is logged and simply leaves the compositor
transparency in place instead of failing startup.

No compositor-specific blur is requested: changing a Hyprland rule at runtime
would mutate the user's session from inside a chat client, so the README instead
documents the one-line `windowrule` a user can add if they want the desktop behind
the window blurred as well.

**Not verified visually.** The wallpaper parsers, `cover_uv`, blur and
`Settings::normalize` are covered by tests (the suite is 262 lib + 5 bin) and the
binary starts without a panic, but the translucent window, the painted backdrop and
the new icon have not been seen in a signed-in window — D28 still blocks reaching
one.

## D40. Vencord's conveniences are reimplemented, starting with timestamps and a counter

Vencord and BetterDiscord work by patching Discord's own bundle, which Rustcord
never loads (D36). Their small conveniences are therefore not "supported" here;
the ones worth having are rebuilt against this client's own state, and the ones
that need Discord's user-account API are left out. The first two live in a new
Settings → Extras section so the next ones have an obvious home.

**Timestamps on every message.** Discord itself prints a time only on the first
message of a group, and Rustcord has no grouping — every row already carried
`HH:MM`. The useful addition is the date, so `message_timestamps` (default on)
switches rows to `Today at 21:04`, `Yesterday at 21:04`, `3 Jan at 21:04` and
`31 Dec 2025 at 21:04`, and every stamp carries the full weekday, date and seconds
in its hover tooltip. The formatting is a pure function over the message time and a
supplied "now" in the local offset, so the Today/Yesterday/year boundaries are
tested without waiting for midnight or depending on the machine's timezone. Turning
the setting off returns the terse clock.

**Character counter.** `character_counter` (default on) draws `N/2000` at the
composer's right edge, reserving 62 px so long drafts cannot slide under it. The
limit is read from `DiscordClient::MAX_CONTENT_CHARS`, the same constant
`valid_content` enforces, so the counter and the send refusal cannot disagree.
Muted below 1800, accent-coloured from 1800, danger red above 2000, with a tooltip
that explains the limit; the send path still refuses over-long content rather than
truncating it.

**Not verified visually.** Both format branches and the day boundaries are covered
by tests, and the app builds and starts, but the row stamp and the counter have not
been seen in a signed-in window — the only credential here is a user token Discord
rejects (D28).

## D41. Rich Presence uses Discord's local IPC socket, not another token

Discord publishes presence through the Rich Presence IPC protocol: an application
connects to the socket of the Discord client already running on the machine
(`$XDG_RUNTIME_DIR/discord-ipc-0`, or inside the Flatpak/Snap runtime directory),
handshakes with a public application id, and sends `SET_ACTIVITY`. No token, no
HTTP, no gateway — this is the same channel games and music players use, and it is
why adding it does not touch D2/D15: nothing here authenticates as a person.

**Why not read presence instead.** Reading friends' activities needs the gateway's
`GUILD_PRESENCES` intent, which Discord grants to bots in fewer than 100 guilds and
which is not what "integrate RPC" means here. Rustcord *publishes* its own activity;
it does not read anybody else's.

**Opt-in and narrow.** `discord_rpc` defaults to off because the activity is visible
to other people. `rpc_client_id` is a snowflake validated like `oauth_client_id`
(digits or nothing), with the OAuth application id as a fallback so one application
can serve identity login and presence. `rpc_show_channel` decides whether the state
line names the channel; off leaves just the server. Nothing about messages, tokens
or the keyring is sent — details are the guild name, state is `#channel`, and the
timestamp is process start.

**Own thread, own failure mode.** The IPC socket may not exist (Discord closed), may
refuse an invalid id, or may disappear mid-session, so all of that lives in one
worker thread with a `Sender` held by the app. The thread retries every 15 s, replays
the last presence after a reconnect, applies changes within 500 ms while connected,
and logs one line instead of spamming when there is no socket. The UI path only
calls `Rpc::sync`, which starts, retargets or drops the worker; it never blocks on
I/O. Disabling the setting or clearing the id drops the sender, the socket closes,
and Discord removes the activity.

**Socket discovery covers sandboxes.** Flatpak Discord keeps its socket inside
`app/com.discordapp.Discord/`, so sandboxed clients, canaries, Vesktop and Snap are
tried before the bare runtime directory. The protocol itself is a small amount of
code — 8-byte little-endian header, JSON body, opcode 0 for the handshake and 1 for
frames — and the framing, header validation, field trimming (Discord ignores text
shorter than two characters) and socket ordering are unit-tested.

**Verified only as far as this machine allows.** The enable/disable path, payload
shape and transport start-up were exercised live, and the app logs
`discord rpc: no Discord IPC socket found; will keep checking` when the client is
absent. Discord is not running here, so an activity has not actually appeared on a
profile, and no valid application id was available to handshake with. That is
recorded in `TASKS.md`.

## D42. A folder's colour also tints its servers, so a group reads as a group

D38 gave each folder tile a colour, but expanding it showed plain `theme.surface`
circles: the colour said "these belong together" only while the folder was closed,
which is exactly when the individual servers are not visible.

**Tint, not repaint.** A member server's fill is the theme surface blended 55 % toward
the folder colour (`utils::mix`). At that weight the palette's mid-dark colours still
carry white initials at the same contrast as a plain surface, while the tint is
obvious side by side with an ungrouped server. The folder's own colour is also drawn
as a 1.5 px ring around each member, which survives selection — the selected server
keeps Discord's accent fill and white pill, so "where am I" still outranks "which
group". A folder with no colour (`None`) keeps the old surface fill and gains no
ring; nothing about the ungrouped rail changes.

**The closed folder previews it too.** The 2×2 mini initials inside a collapsed
folder are blended toward the same colour at 35 %, so the tile's colour, its preview
grid and its expanded members all agree.

**Colour source.** The tint is parsed from the same normalised `#rrggbb` the folder
stores (D38), inside the draw call that already resolves it, so a hand-edited or
invalid value degrades to the plain surface instead of painting something arbitrary.
`mix` clamps its weight, keeps the base colour's alpha (important under the
translucent glass, D39), and is covered by a test for both endpoints, the midpoint
and out-of-range weights.

**Not verified visually.** The tint maths is tested, the code compiles and the app
starts, but the rendered rail has not been seen in a signed-in window — same
credential blocker as D38/D39 (D28).

## D43. Spotify now-playing is a PKCE connection the user starts, not a bundled secret

Roadmap item 5 was "poll Spotify and drive the plugin". The plugin already had the
presentation (track, artist, album, progress bar, and a Discord activity payload)
and `api::spotify` had the typed client; what was missing was a legal way to get a
token and a loop that keeps it fresh.

**Public client, no secret.** Spotify documents the authorisation-code flow with
PKCE for public clients, so `src/api/spotify_auth.rs` mirrors `api::oauth`: a state
value and verifier are generated locally, only the S256 challenge leaves the
process, the redirect comes back to `127.0.0.1:53135` (a second loopback port, so a
Discord identity sign-in can be in flight at the same time), and the callback's
`state` must match before the code is looked at — it reuses `api::oauth`'s
`parse_callback` for exactly that reason. The only scope requested is
`user-read-currently-playing`; nothing about playlists, library or profile is
asked for. The tokens are a JSON blob in the keyring under `spotify_session`
(never in `settings.json`), and the record says which client id issued it, so
changing the id in settings invalidates the old grant instead of silently reusing
it.

**Polling lives on its own thread.** `src/spotify_sync.rs` owns the session and
asks Spotify every five seconds (the interval the README promised), refreshing the
access token when it is within a minute of expiry, keeping the old refresh token
when Spotify does not rotate one, and saving a rotated one back to the keyring. The
UI drains updates — `Track`, `Idle`, `Error` — once per frame, so a slow request can
never stall a frame; the thread stops when the handle is dropped (toggling the
feature off, disconnecting, or changing the client id). Repeated identical errors
are reported once rather than every five seconds, and a failed request forces a
refresh attempt next round, which is the usual cause.

**Presentation stays in the plugin.** A track replaces the demo, `Idle` clears the
state instead of leaving the last song frozen, and the progress bar keeps advancing
locally between polls through the existing `tick`. The album-art URL from Spotify is
carried into the activity's `large_image` so a future renderer can use it.

**Disclosure, not a new exception.** This is the first non-Discord network traffic,
so it is explicit: `accounts.spotify.com` and `api.spotify.com` join the exact-match
allowlist in `privacy.rs`, the Settings → Privacy store list gains the Spotify
session, the README table and allowlist paragraph name both hosts, and the controls
sit next to the existing "Show Spotify now-playing activity" switch. Nothing
contacts Spotify until the user presses Connect.

**Verified as far as possible here.** Token parsing (with and without a refresh
token), refresh merging, expiry slack, the authorise URL and the store list are
unit-tested, and the loopback listener was exercised against a real TCP request to
confirm a mismatched `state` yields no code (that test was temporary because it
binds a fixed port). The end-to-end connection has not been made: there is no
Spotify application id in this environment, so the browser round trip, the token
exchange and the live poll are unverified. `TASKS.md` records that.

## D44. Typing goes out at most once per eight seconds, and never blocks a frame

Inbound typing already existed: `TYPING_START` from the gateway goes into
`TypingTracker`, and the composer draws "alice is typing…" with its own 500 ms
repaint while the entry is alive. What was missing was the other direction, so the
person on the other side of a conversation had no sign that a reply was being
written.

**One REST call per eight seconds.** Discord rate-limits `POST
/channels/{id}/typing` to once per ten seconds per channel, and clients keep the
indicator alive by re-sending just under that. `models::typing_due` is the whole
policy: the first keystroke in a channel sends immediately, switching channels
sends immediately, and the same channel waits `TYPING_REFRESH` (8 s). It is a pure
function over the last (channel, instant) and is tested at the boundary, so the
rule cannot drift into a call per keystroke.

**Cosmetic, therefore quiet.** The request is spawned on the existing runtime from
`notify_typing`, which no-ops when there is no live gateway, no selected text
channel, an empty draft, or no session. Failures are logged at `debug` and never
touch the status line or a user-facing alert — a typing dot is not worth an error
banner. The send path is untouched, so a refused typing call cannot delay or block
a message.

**Only while composing.** The composer hooks `TextEdit::changed`, which fires on
text edits, paste and deletion; the empty-draft guard means clearing the field does
not announce anything, and the eight-second window means holding a key down is one
call every eight seconds rather than one per frame. The bot's own `TYPING_START`
is already ignored when recording inbound typing, so the composer never says
"Rustcord is typing…".

**Unverified against a live channel.** The throttle is unit-tested and the code
compiles, but no call has been made to Discord: this environment has no working bot
session (D28). `TASKS.md` records it as `[~]`.

## D45. Search covers the whole session cache, and channel is a first-class filter

The original overlay searched `App::messages`, which already holds every message
cached this session — but the UI said "held in this channel", the hint mentioned
only authors, and there was no way to narrow by channel. That under-sold what the
data could answer and made cross-channel results look like bugs.

**Session-wide by construction.** `App::refresh_search` still scans `messages`
(the buffer bounded by `ResourcePriority::history_cap()`), and results are newest
first across all channels. The overlay now reports how many messages and how many
distinct channels are in scope, so "nothing matched" has a clear meaning instead of
implying the current channel was the only input.

**Channel names resolve per search.** `search()` takes a channel-id → name map built
from `App::channels` each time it runs. That keeps the filter and the result labels
correct after a channel is renamed or a guild finishes loading, without an index to
invalidate. A message whose channel is not in the map still matches a text query —
it simply has no `#name` to print.

**Filter syntax, not another dialog.** `in:#general`, `channel:general` and a bare
`#general` all set the same lowercased channel fragment (substring match, like the
author filter), and combine with `from:name` and free-text terms, which all must
match. A bare `in:` is treated as an unfinished filter rather than a literal term,
so typing the syntax never silently drops a term. The overlay prints what the query
parsed to (`matching “ship” from bob in #general`) in the accent colour, which is
the feedback that makes syntax discoverable without a second row of controls. Each
result row leads with `#channel`, then author, time and snippet, and clicking it
opens that channel as before.

**Tests and limits.** Parsing (`in:#`, `channel:`, `#name`, unfinished markers),
substring narrowing, unknown channels matching nothing, channel names on hits and
orphan messages with no resolvable name are covered by two new tests. The ceiling is
unchanged: 50 results, cached messages only, no index and no network — Discord's
guild search remains a user-API endpoint this client cannot call (D20).

## D46. WASM plugins run under wasmi with no host imports; voice stays gated on DAVE

Roadmap items 6–8 were requested together. Item 8 (the WASM sandbox) was the one
that could be delivered honestly in this environment, because it needs no Discord
session and every guarantee is testable locally. Items 6 and 7 cannot be finished
without DAVE (see below), so they are documented instead of stubbed.

**Sandbox.** `src/plugins/wasm.rs` implements the loader the old
`load_wasm_plugin` stub refused to fake:

- `plugin.json` is parsed with `deny_unknown_fields`; unknown permissions fail
  deserialization, and the only capability today is `transform_messages`. A
  manifest without it is refused, so a plugin can never be loaded and then simply
  not run.
- The module path is rejected if absolute or containing `..`, then canonicalized
  and required to stay inside the plugin folder.
- Modules are capped at 1 MiB, must start with `\0asm`, may **not import anything**
  (there is no host function to call), may not have a `start` function, and must
  export `memory`, `alloc`, `run` and `result_len`.
- Each transform gets a fresh `Store` with fuel metering (default 5 M, manifest
  ceiling 20 M), a 16 MiB memory limit, a recursion limit and 8 KiB input/output
  caps. Pointers are bounds-checked, output must be UTF-8, and every failure is
  fail-closed: the message is sent unchanged and the error is logged at debug.
- Loading is opt-in (`Settings::wasm_plugins`, off by default) and scans
  `~/.config/rustcord/plugins/*/`; a broken folder is logged and skipped so it
  cannot stop startup or other plugins.

`wasmi` was chosen over `wasmtime` for size and a pure-Rust build; the cost is an
interpreter, which is the right trade for short text transforms.

**Voice and Go Live (items 6–7) are blocked on DAVE.** Discord has required
end-to-end encryption via the DAVE protocol for third-party voice clients since
1 March 2026 (voice close code `4017`). DAVE needs an MLS group, epoch transitions
and frame-level encryption, and Discord's own guidance is to use libdave (C++).
Implementing capture, signalling or Go Live UI before that would produce a client
that joins and is immediately disconnected — the half-built feature D21 forbids.
`docs/VOICE_AND_STREAMING.md` records the full requirement chain and the order to
build it in. Existing voice *presence* is unaffected because it uses no voice
connection.

**Packaging.** Item 10 gained `packaging/PKGBUILD` (builds with `--frozen`,
installs the binary, desktop entry, SVG/PNG icons and licence). AppImage and
Flatpak remain open; they need a clean-VM build that this environment cannot
provide.

## D47. The typing indicator animates, and the header shows it too

The inbound indicator was a static label above the composer, which meant the only
sign that somebody was still typing was the text itself disappearing. It now
animates and appears in the channel header as well.

**Dots are a pure function of the clock.** `models::typing_dots(now)` returns `.`,
`..`, `...` on a 400 ms cycle (`TYPING_FRAME_SECONDS`). `TypingTracker::summary`
no longer bakes an ellipsis into its sentence, so the same string feeds both
places and the animation lives in one tested function instead of in widget state.
The phase is computed with `floor`, not a cast: a cast truncates toward zero, so a
negative frame time would land in the wrong frame (caught by a test).

**One computation, two places.** `ui/chat.rs` resolves the selected channel's
summary and dots once at the top of the frame, then uses that string in the header
(next to the `#name`, prefixed by a small pencil glyph) and above the composer.
While somebody is typing the UI asks for a repaint every 120 ms — enough for a
400 ms cycle, and only while an indicator is actually on screen. Expiry still
needs no timer: `summary` prunes against the same clock, so the label disappears
when `TYPING_TTL` passes.

**Why the header.** The composer sits at the bottom of a tall message list; with
the input scrolled away or the window short, a typing notice there is easy to
miss. The header is always visible and already the place where channel-scoped
state (the LIVE badge) is shown. Both labels are the same muted italic style, and
the header one carries a tooltip so the glyph is not a mystery.

**Scope.** Display-only: outgoing notices still follow D44's eight-second throttle,
and nothing here makes a network call or adds a thread. `typing_dots` is covered
for all three frames, the cycle boundary and a negative clock; the existing
summary tests were updated for the removed ellipsis rather than loosened.

## D48. A focused draft renews its typing notice; nothing polls

D44 sent a notice on the first keystroke and then at most one per eight seconds,
but only *on a keystroke*. A draft typed and then thought about for more than ten
seconds let Discord's indicator lapse, even though the field still had focus and
the user was plainly composing.

**Renewal follows focus, not keystrokes.** The composer now calls `notify_typing`
every frame while the text field has focus and the draft is non-empty. That call
is already the throttle: it spends nothing when the eight-second window is open,
and it is the single place that checks live mode, the gateway, the channel type
and the selected channel. Pasting a draft and clicking into the field therefore
starts the notice too, which the keystroke-only path missed.

**One scheduled wake-up, not a poll.** `models::typing_refresh_in` returns the time
remaining in the current window, or `None` when no notice was sent for this
channel. The composer requests `request_repaint_after` for exactly that remainder,
so an idle app sleeps: the renewal frame fires at the eight-second mark, sends, and
the next wait is scheduled. `Some(0)` means it is due now, which is what the first
frame after the window opens sees. egui's clock is used for display timing, but the
throttle itself stays on `Instant`, so pausing or dragging the window cannot delay
a renewal.

**Failure stays quiet.** Renewals reuse D44's spawned request and debug-level
logging; a refused call does not touch the draft, the status line or an alert, and
the next window retries. Nothing is sent when the draft is blank or whitespace, so
clearing the field stops the notice immediately (and expiry still needs no timer).

**Tests.** The new pure helper is covered inside the existing typing test: five
seconds remaining after three have elapsed, zero once overdue, and `None` for a
different channel. Total remains 301 since no new test function was added.
