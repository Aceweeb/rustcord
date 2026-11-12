# YAPFAST_PARITY.md — audit against Yapfast's public feature set

Audit date: 2026-11-12. Rustcord at commit `4b44ca0` plus the gateway-reliability
work in this session.

Yapfast (`github.com/yapfast/yapfast`, GPL-3.0) is a native Rust client with a
Slint UI, about 30 MB idle, Discord and Matrix in one window, and voice, video
and screen sharing. Its README says third-party clients are against Discord's
terms. It links an **account** by scanning a QR code with Discord on a phone and
then speaks as that user.

That last sentence is the whole shape of this comparison. Yapfast's breadth comes
from being allowed to act as a person, which Rustcord refuses (`DECISIONS.md` D2,
D15) and which no amount of engineering makes safe or permitted. So the useful
question is not "does Rustcord have what Yapfast has" but "of what Yapfast does,
what can a bot-token client legitimately do, and does Rustcord do it yet".

Nothing here was copied: Yapfast is GPL-3.0, Rustcord is MIT/Apache-2.0. Its
README and user documentation were read as a feature checklist only.

## 1. Already implemented

| Area | Rustcord | Where |
| --- | --- | --- |
| Native window, no web view | egui + glow, CPU idle 0 %, 2 threads | `src/main.rs`, `src/ui/` |
| Server rail with switching | guild buttons, selection, per-guild channel filter | `src/ui/sidebar.rs` |
| Channel list | text and voice channels per guild, selection | `src/ui/sidebar.rs` |
| Main message panel | virtualised variable-height rows, only visible rows cost | `src/ui/chat.rs`, `src/ui/virtual_list.rs` |
| Pagination / history | `before` cursor, 100 per page, explicit "Load older messages" | `src/api/discord.rs`, `src/app.rs` |
| Send messages | REST create, demo mode offline | `src/api/discord.rs` |
| Attachments and embeds | rendered inline, byte-bounded LRU texture cache, CDN allowlist | `src/images.rs` |
| Reactions (display) | chips with counts, self-reaction flag | `src/models.rs`, `src/ui/chat.rs` |
| Live edit / delete / reaction / typing | six gateway events applied in place | `src/api/gateway.rs` (D17) |
| Typing indicator | composer line, 8 s expiry, bounded repaints | `src/models.rs`, `src/ui/chat.rs` |
| Gateway transport | Hello, heartbeat + ACK, Identify, Resume, op 7/9 handling | `src/api/gateway.rs` |
| Gateway reliability | 10 identifies/hour, jittered backoff, stability-gated reset | `src/api/backoff.rs` (D19) |
| REST rate limits | per-route buckets, `X-RateLimit-*`, global 429, one retry | `src/api/ratelimit.rs` (D16) |
| Bot authentication | token in the OS keyring, shape check, sign-out | `src/auth.rs`, `src/ui/login.rs` |
| Identity login | OAuth2 `identify` + PKCE, separate keyring entry | `src/api/oauth.rs` (D18) |
| Settings | nine sections, persisted JSON, reset | `src/ui/settings.rs` |
| Theming | 12 presets, accent override, CSS theme files | `src/ui/theme.rs`, `src/plugins/vencord_compat.rs` |
| Diagnostics | log ring buffer, checks, report export, `/proc` telemetry | `src/diagnostics.rs`, `src/sysinfo.rs` |
| Small Discord extras | Spotify status, streaming indicator, banner plugin | `src/plugins/` |

## 2. Partially implemented

| Area | What exists | What is missing |
| --- | --- | --- |
| Channel categories | `ChannelKind::Category` is modelled and parsed | no collapsible grouping in the sidebar |
| Guild/channel sync | `GUILD_CREATE` brings guild + channels | no `CHANNEL_CREATE/UPDATE/DELETE`, `GUILD_UPDATE/DELETE` |
| Presence | `User.status` exists and demo data sets it | no `PRESENCE_UPDATE`, no live dots in the member list |
| Message rendering | plain text, code points preserved | no Markdown (bold, code blocks, links), no mention highlighting |
| Message interactions | inbound edits/deletes/reactions work | no context menu, no reply, no outbound edit/delete/react |
| Member list | rendered from guild data | no roles, no grouping, no member paging for large guilds |
| Themes | presets + CSS variables | no user-editable layout density beyond `compact_mode` |

## 3. Missing (legitimate, bots can do these)

| Area | Notes |
| --- | --- |
| Message search | Discord's `/guilds/{id}/messages/search` needs a **user** token, so bots cannot use it. The honest version is a local index over loaded history; that is what should be built. |
| Threads | readable by bots over REST (`/channels/{id}/threads/...`); no UI at all today |
| Replies | needs `message_reference` on send and a reply preview on render |
| Direct messages UI | the `DIRECT_MESSAGES` intent is requested but there is no DM list or DM composer |
| Context menus | right-click message/channel/user menus |
| Unread indicators, mentions | needs `READ_STATE`/`MESSAGE_ACK` handling and a mention counter |
| Notifications | no desktop notification path (D-Bus/`notify-rust`) |
| Attachment upload | downloads only |
| Emoji/GIF pickers | text entry only; custom emoji render as text |
| Permissions | no role/permission model, so no gating of channels or actions |
| Voice channels | listed, not connectable |

## 4. Technically unsupported for Rustcord (by decision, not by accident)

| Yapfast feature | Why Rustcord will not do it |
| --- | --- |
| Sign in as a person (QR link or token) | Self-bot: against Discord's terms and a ban risk. Refused in D2/D15 and again for OAuth2 in D18 — the identity login is `identify` only and says so in the UI. |
| Friends list, personal DMs, personal profile editing | Require a user token; no bot API exists for them. |
| Voice, video, screen share as a person | Needs user auth. A *bot* voice connection is a different protocol (Opus + DAVE/E2EE) and is research, not a port of Yapfast's. |
| Vencord plugins | Need a JavaScript runtime; Rustcord's plugin surface is native Rust. |
| Matrix, Telegram, Signal bridges | Out of scope: this is a Discord client. |

## 5. Requires further research

| Question | Why it matters |
| --- | --- |
| Bot voice with DAVE/E2EE | Discord now requires end-to-end encryption for voice; library support in Rust is the open question. |
| Thread pagination semantics | Threads have their own `before` semantics and archived states. |
| Local search quality | Ranking and index memory cost against the 500 MB ceiling. |
| Desktop notifications on Wayland | Portal vs D-Bus; needs a real session to verify. |

## 6. What this session added

Gateway reliability, the second item on the roadmap and the highest-priority
missing item after the message lifecycle work:

- `src/api/backoff.rs` — identify budget (10 an hour, sliding), jittered delays
  (50–100 % of each step), stability-gated backoff reset (60 s), monotonic clock.
  Identifies are spent only when a fresh handshake is actually needed; a Resume
  costs nothing and the budget wait is not counted as a failed attempt.
- Wired into `src/api/gateway.rs`: a budget-exhausted cycle returns
  `CycleOutcome::BudgetWait`, reports the wait to the UI, and sleeps cancellably.
- 11 new tests; 169 passing in total.

## 7. Recommended order from here

1. Local message search over loaded history (no user token needed, bounded index).
2. Message context menu: copy, edit, delete, react, reply — makes the lifecycle
   events bidirectional.
3. Replies end to end (send with `message_reference`, render preview).
4. Channel category grouping and unread/mention counts.
5. DM list and DM composer (the intent is already requested).
6. Threads, then notifications, then bot voice research.
