# HANDOFF.md

Read `PROJECT.md` → `CURRENT_STATE.md` → `TASKS.md` → this file, then verify the
repository state before changing anything.

## Verified state at handoff

- Path: `/mnt/storage/Default Project/rustcord`
- Branch: `main`, remote `origin` = `https://github.com/Aceweeb/rustcord`
- `cargo test --all-targets`: 349 lib + 5 bin tests passing (354)
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build --release`: success
- Runtime: 198 MB RSS, 138 MB PSS, 198 MB peak, 7–12 threads; CPU is 0.0% without animation and nonzero while animated surfaces redraw

## What changed most recently

Borderless window chrome (`DECISIONS.md` D81): the OS decorations are off and a
32 px theme-aware title strip carries the centered guild name, drag region,
double-click maximize/restore, real minimize/maximize/close commands, and eight
foreground resize grips. Two geometry tests; suite 354. Release 198 MB RSS /
138 MB PSS / 198 MB peak, 102 MB of headroom. Native drag/resize gestures were
not visually exercised in this environment.

Discord fidelity pass (`DECISIONS.md` D80): the channel sidebar now has a
100 px banner header using the bounded image cache, guild name/chevron, member
toggle and a real-count boost strip. The chat header has a Discord-style search
pill; message authors use roster-backed display names and their highest
positioned role colour; the NEW separator is a red rule with a right-aligned
pill. The account strip is accent-tinted with a working Settings gear, and the
member list starts hidden to match the reference. Two new tests; suite 352.
Release 206 MB RSS / 155 MB PSS / 217 MB peak, 83 MB of headroom. Native visuals
were not inspected; the OS title bar was still native then, so this was
content-level fidelity rather than literal pixel parity. D81 replaced that title
bar with the client's own borderless chrome.

Bounded attachment queue (`DECISIONS.md` D79): the paperclip accepts multiple
files, the composer keeps up to ten removable rows with a running total, and the
send path enforces 10 MiB per file and 20 MiB per batch in both the composer and
the API. Multipart bodies now map `files[n]` to `attachments[n]` ids and
filenames. Live upload was not exercised; the limits and payload mapping have
unit tests.

Caret-aware emoji insertion (`DECISIONS.md` D78): the composer reads the text
editor's persisted character range, inserts the chosen glyph at the collapsed
caret or replaces the selection, and restores focus with the caret after it.
With no editor state it falls back to appending. The UI event path was not
visually checked; the pure insertion helper is unit-tested.

Message dividers (`DECISIONS.md` D77): local-date separators and a NEW line at
the first live-unread message, rendered as measured virtual rows; divider rows
break author grouping. Release peak measured at 236 MB, under the 300 MB ceiling.
Visual rendering was not checked.

Role-grouped member panel (`DECISIONS.md` D76): guild roles come from the gateway
and are refreshed over REST with the roster; online/offline catch-alls remain for
members without a hoisted role. Release peak measured at 250 MB, under the
300 MB ceiling. Live role rendering was not visually checked.

Composer toolbar (`DECISIONS.md` D75): paperclip/native file picker, a removable
attachment chip, multipart upload and a Unicode emoji picker that inserts into
the draft. Release peak measured at 251 MB, under the 300 MB ceiling; the native
dialog raises the stripped binary to about 14 MB.

Hover-card member counts (`DECISIONS.md` D74): the server peek now shows total
and active approximate counts under the name, reusing the D73-refreshed data.
Release peak measured at 261 MB, under the 300 MB ceiling.

Immediate member counts (`DECISIONS.md` D73): returning focus to the window or
receiving READY/RESUMED forces the guild-count refresh instead of waiting for the
five-minute cadence. Release peak measured at 242 MB, under the 300 MB ceiling.

Guild icon focus pause (`DECISIONS.md` D72): the rail uses its own animation
clock, advances it only while the window has focus, and only then schedules the
next 12.5 fps repaint. Release peak measured at 264 MB, under the 300 MB ceiling.

Animated avatars and banners (`DECISIONS.md` D71): user avatar and profile-banner
hashes with `a_` request GIFs, guild banners are parsed and shown in the rail
hover peek, and every avatar surface plus both banner surfaces draw the current
GIF frame. Profile banners are fetched only while their card is open. Release peak
measured at 256 MB, under the 300 MB ceiling.

Discord header/chrome pass (`DECISIONS.md` D70): parsed channel topics, working
search and member-list buttons, settings in the user panel. Release peak measured
at 256 MB, under the 300 MB ceiling.

Roster-backed channel members (`DECISIONS.md` D69):

- `src/ui/chat.rs` — roster-seeded list with author/voice extras, virtualised
  member rows, visible-only avatar loading, 1 test.

Message hover actions (`DECISIONS.md` D68):

- `src/ui/chat.rs` — floating react/reply/more bar, shared menu content, 2 tests.

Periodic guild counts (`DECISIONS.md` D67):

- `src/app.rs` — five-minute count pass for all guilds, idle repaint scheduling,
  counts independent of roster eviction.
- `src/api/discord.rs` — non-destructive `GuildCounts::merge`, 1 test.

Message dividers (`DECISIONS.md` D77):

- `src/models.rs` — first-unread id in `UnreadTracker`.
- `src/app.rs` — snapshot the first unread when opening a channel.
- `src/ui/chat.rs` — date/NEW virtual rows, local labels and divider drawing.

Caret-aware emoji insertion (`DECISIONS.md` D78):

- `src/ui/chat.rs` — read the editor's character range, replace/insert at the
  caret, store the new cursor and restore focus; pure insertion helper + test.

Bounded attachment queue (`DECISIONS.md` D79):

- `src/ui/chat.rs` — multi-file picker, removable attachment rows, running total
  and clear-all control.
- `src/app.rs` — attachment vector, per-file/count/total validation, removal and
  the send path.
- `src/api/discord.rs` — multipart `files[0..n]`, matching `attachments` ids and
  filenames, plus a second count/size check over the files actually read.

Role-grouped member panel (`DECISIONS.md` D76):

- `src/models.rs` — `Role` and `User::role_ids`; `Guild::roles`.
- `src/api/gateway.rs` — parse roles from GUILD_CREATE/READY.
- `src/api/discord.rs` — role ids on parsed members and `/guilds/{id}/roles`.
- `src/app.rs` — roles joined with the roster and installed on the guild.
- `src/ui/chat.rs` — highest hoisted role sections, role-colour dots and fallback
  ONLINE/OFFLINE sections.

Composer toolbar (`DECISIONS.md` D75):

- `src/ui/chat.rs` — attachment chip, toolbar, emoji popup with a bounded scroll
  area, and `rfd` file picker.
- `src/app.rs` — one `ComposerAttachment`, size/regular-file validation, and the
  attachment/clone path into a send.
- `src/api/discord.rs` — multipart `payload_json` + `files[0]` through the shared
  rate-limited request helper, plus MIME selection and a 10 MiB cap.
- `Cargo.toml` — `rfd 0.17`, reqwest `multipart`, tokio `fs`.

Hover-card member counts (`DECISIONS.md` D74):

- `src/ui/sidebar.rs` — the peek builds a total/active summary from cached
  counts, the guild payload and the loaded roster; a helper hides unknown fields.

Immediate member counts (`DECISIONS.md` D73):

- `src/app.rs` — `window_focused` detects the focus edge, `counts_refresh_immediate`
  queues one forced pass, and `counts_refresh_due` bypasses the cadence. READY
  and RESUMED set the same flag.

Guild icon focus pause (`DECISIONS.md` D72):

- `src/app.rs` — `rail_animation_time` holds the last focused timestamp.
- `src/ui/sidebar.rs` — `animation_clock` freezes that timestamp while unfocused;
  the rail only schedules animation repaints when focused.

Animated avatars and banners (`DECISIONS.md` D71):

- `src/models.rs` — user avatar/profile-banner and guild-banner URLs use GIF for
  `a_` hashes; animated banners are fetched at 240 px.
- `src/api/gateway.rs` — guild `banner` parsed from `GUILD_CREATE`.
- `src/images.rs` — GIF frame pipeline accepts `/avatars/` and `/banners/` too.
- `src/ui/chat.rs` — `animated_texture` draws the current frame and repaints only
  while visible; all avatars and the profile banner use it. Profile-banner loads
  are queued from the hover card and drained after the UI closures finish.
- `src/ui/sidebar.rs` — 64 px server banner at the top of the guild hover peek.

Animated guild icons (`DECISIONS.md` D66):

- `src/models.rs` — `a_` icon hashes use the `.gif` endpoint.
- `src/images.rs` — multi-frame `Decoded`, `AnimatedTexture`, shared byte cache,
  `frame(url, time)`, 3 tests.
- `src/ui/sidebar.rs` — rail picks the current frame and repaints only while an
  animation is present.

Channel categories (`DECISIONS.md` D65):

- `src/models.rs` — `Channel.parent_id`/`position`.
- `src/api/gateway.rs` — parses both, 1 test.
- `src/ui/sidebar.rs` — collapsible category sections, shared row drawing.
- `src/app.rs` — category collapse state, keyboard order mirrors the sidebar,
  demo `COMMUNITY` category.

Immediate numbered defaults (`DECISIONS.md` D64):

- `assets/default-avatars/0-5.png` — embedded Discord defaults.
- `src/images.rs` — preload/decode API, 1 asset test.
- `src/models.rs` — `default_avatar_index`, `avatar_url` custom-only.
- `src/ui/chat.rs` — default texture before the initial fallback.
- `src/app.rs` — preload once at startup.

Resource bounds (`DECISIONS.md` D63):

- `src/main.rs` — glibc `mallopt(M_ARENA_MAX, 2)` before startup.
- `src/settings.rs` — default image cache 32 MB.
- `src/app.rs` — evict rosters beyond the selected guild and two others.

Channel-scoped member panel (`DECISIONS.md` D62):

- `src/ui/chat.rs` — member rows filtered to channel authors/voice members,
  `IN CHANNEL — n`, hover fills for messages and member rows.

Presence dots (`DECISIONS.md` D61):

- `src/ui/chat.rs` — `presence_dot_color`, `avatar_with_status`, member rows use it,
  1 test.

Server icons and member counts (`DECISIONS.md` D60):

- `src/models.rs` — `Guild::icon_url`, `member_count`.
- `src/api/gateway.rs` — parses `icon`/`member_count` (1 test with the URL).
- `src/api/discord.rs` — `fetch_guild_counts` / `GuildCounts`, 1 test.
- `src/app.rs` — counts fetched alongside the roster, cached per guild.
- `src/ui/sidebar.rs` — real guild icons in the rail.
- `src/ui/chat.rs` — `MEMBERS — total` and active count header.

Spotify Rich Presence (`DECISIONS.md` D59):

- `src/rpc.rs` — `Presence` grew `end_unix`/`large_text`; activity payload sends
  end timestamps and the album tooltip, 1 test.
- `src/app.rs` — `presence()` prefers the playing track, `poll_spotify` refreshes
  only when a poll update arrives.

Discord-style message/member rhythm (`DECISIONS.md` D58):

- `src/ui/chat.rs` — `same_message_group`, grouped rows with gutter clock, no gap
  inside a block, ONLINE/OFFLINE member sections.
- `src/ui/sidebar.rs` — `@` glyph for the Direct Messages rail button.

DM list at startup (`DECISIONS.md` D57):

- `src/api/discord.rs` — `fetch_dm_channels`, `DiscordDmListEntry` (DMs + group
  DMs, self filtered), 1 test.
- `src/app.rs` — `ensure_dms`, `dm_loaded`/`dm_loading`, `DmResult::Listed`,
  rail insert at top, refresh replaces DM channels, empty selection handling.
- `src/ui/sidebar.rs` — empty-state hint under DIRECT MESSAGES.

DMs from the member list (`DECISIONS.md` D56):

- `src/models.rs` — `DM_GUILD_ID`.
- `src/api/discord.rs` — `open_dm`, `DiscordDmChannel::into_channel`, 1 test.
- `src/app.rs` — `DmResult`, `open_dm_with`, `poll_dm`, `dm_channels` cache;
  `ensure_members` skips the synthetic guild.
- `src/ui/chat.rs` — member-row `DM` button, `@` header.
- `src/ui/sidebar.rs` — DIRECT MESSAGES labels, `@` glyph, no voice section.

Album art and pause state in the now-playing strip (`DECISIONS.md` D55):

- `src/api/spotify.rs` — `SpotifyTrack::is_playing`; paused tracks no longer become
  `None`; 2 tests.
- `src/plugins/spotify_status.rs` — stores the flag, `tick` freezes while paused;
  1 test.
- `src/ui/sidebar.rs` — 40 px art thumbnail, `⏸` marker, dimmed bar and paused clock.
- `src/images.rs`, `src/privacy.rs` — `i.scdn.co` allowlisted.

Member-list avatars (`DECISIONS.md` D54):

- `src/ui/chat.rs` — roster rows call `avatar`, roster avatars requested through
  `ImageLoader`.

Sidebar account avatar (`DECISIONS.md` D53):

- `src/api/gateway.rs` — READY carries `avatar`, `global_name`,
  `avatar_decoration`; test updated.
- `src/app.rs` — stores the READY user id and cosmetics on `current_user`.
- `src/ui/chat.rs` — `avatar` is `pub(crate)`.
- `src/ui/sidebar.rs` — account panel uses the shared avatar and profile card.

Now-playing strip starts empty (`DECISIONS.md` D52):

- `src/plugins/spotify_status.rs` — removed the seeded Sade track from `init`,
  replaced with `clear()`; 1 test.

Profile pictures and Nitro cosmetics (`DECISIONS.md` D51):

- `src/models.rs` — `User` gains `global_name`, `banner`, `accent_color`,
  `avatar_decoration`; `display_name`, `avatar_url` (custom or Discord default),
  `banner_url`, `decoration_url`; 1 test.
- `src/api/gateway.rs`, `src/api/discord.rs` — parse the new fields.
- `src/ui/chat.rs` — avatar helper, decoration overlay, hover profile card; visible
  rows request avatar/banner/decoration images through `ImageLoader`.

Search match highlighting and jump-to-message (`DECISIONS.md` D50):

- `src/search.rs` — `highlight_ranges` (merged byte ranges, lowercase-length safe),
  1 test.
- `src/ui/virtual_list.rs` — `estimated_offset_of` with measured-height averaging,
  1 test.
- `src/app.rs` — `jump_to_message`, `scroll_to_message`, `jump_highlight`,
  `jump_attempts`; `jump_to_channel` now shares `open_channel`.
- `src/ui/chat.rs` — offset estimate, ≤6-frame convergence, 2.5 s tint, cancel on
  manual scroll.
- `src/ui/search.rs` — `result_job` `LayoutJob` with highlighted terms; click jumps
  to the message.

Search date bounds (`DECISIONS.md` D49):

- `src/search.rs` — `SearchQuery::{before, after}` parsed from
  `before:`/`after:` (`YYYY-MM-DD`, `YYYY-MM-DDThh:mm[:ss]`, UTC), exclusive
  checks in `hit_for`, dates in `describe()`, 1 test.
- `src/ui/search.rs` — hint and footer name the filters.

Typing-notice renewal while a draft stays focused (`DECISIONS.md` D48):

- `src/models.rs` — `typing_refresh_in` returns the time left in the eight-second
  window (or `None`); assertions added to the existing typing test.
- `src/app.rs` — `typing_refresh_wait` gates on live mode/gateway/draft and
  exposes the remaining wait.
- `src/ui/chat.rs` — focused composer with text calls `notify_typing` each frame
  and schedules one `request_repaint_after` at the renewal mark.

Animated typing indicator in the header and composer (`DECISIONS.md` D47):

- `src/models.rs` — `typing_dots(now)` (400 ms `.`/`..`/`...`, `floor`-based
  phase) and `TYPING_FRAME_SECONDS`; `TypingTracker::summary` no longer appends an
  ellipsis; one new test.
- `src/ui/chat.rs` — label computed once at the top of the frame, drawn after the
  header's channel name (pencil glyph + tooltip) and above the composer; repaint
  every 120 ms only while somebody is typing.

Roadmap item 8 (WASM sandbox, `DECISIONS.md` D46) and item 10 (PKGBUILD):

- `src/plugins/wasm.rs` — `wasmi` loader, manifest/permissions, fuel + memory +
  I/O caps, no host imports, path-traversal checks, fail-closed calls, 15 tests.
- `src/plugins/mod.rs` — `load_wasm_plugins(dir)` scans folders, skips broken
  ones, `external_count()`; `load_wasm_plugin` is now real.
- `src/app.rs` — loads plugins at startup when `settings.wasm_plugins` is on.
- `src/settings.rs` / `src/ui/settings.rs` — opt-in toggle in Extras with the
  loaded count and folder path.
- `packaging/PKGBUILD`, `docs/VOICE_AND_STREAMING.md` (DAVE gate for items 6–7).

Session-wide search with channel filters (`DECISIONS.md` D45):

- `src/search.rs` — `SearchQuery::channel` parsed from `in:#name`,
  `channel:name` and `#name`; `search(messages, channel_names, query, limit)` with
  the channel filter and resolved `Hit::channel`; `describe()` includes the channel;
  2 tests.
- `src/app.rs` — `refresh_search` builds the id→name map from `channels` per search.
- `src/ui/search.rs` — hint names both filters, header reports cached messages and
  channels, parsed filter line in accent colour, rows lead with `#channel`.

Outgoing typing indicators (`DECISIONS.md` D44):

- `src/api/discord.rs` — `trigger_typing(channel_id)` posting to
  `/channels/{id}/typing` through the shared rate-limited `request`.
- `src/models.rs` — `TYPING_REFRESH` (8 s) and pure `typing_due(last, channel, now)`
  with boundary assertions inside the typing tests.
- `src/app.rs` — `typing_notice` state and `notify_typing`, which guards on live
  data, text channel, empty draft and gateway before spawning the request.
- `src/ui/chat.rs` — composer calls `notify_typing` on `TextEdit::changed`.

Spotify live now-playing (`DECISIONS.md` D43, roadmap item 5):

- `src/api/spotify_auth.rs` (new) — PKCE flow, token parse/refresh, keyring
  session carrying its client id, loopback listener on port 53135; 3 tests.
- `src/spotify_sync.rs` (new) — worker-thread poller (5 s), refresh near expiry,
  Track/Idle/Error updates, error de-duplication; 2 tests.
- `src/app.rs` — `spotify_poll` / `spotify_connect` / `spotify_status`, restore on
  start, `poll_spotify` per frame, `start_spotify_connect`, `disconnect_spotify`,
  settings reconciliation (toggle off, changed client id).
- `src/plugins/spotify_status.rs` + `plugins/mod.rs` — `set_from_track`, `clear`,
  manager pass-throughs.
- `src/ui/settings.rs` — client id field, Connect/Disconnect, status under the
  existing Spotify switch.
- `src/privacy.rs` — both Spotify hosts on the allowlist, a store row, updated
  privacy text and tests.

Folder-tinted server tiles (`DECISIONS.md` D42):

- `src/utils/mod.rs` — `mix(a, b, t)`, an alpha-preserving blend with a clamped
  weight, plus a test.
- `src/ui/sidebar.rs` — `guild_row` takes an optional folder `tint`; unselected
  members fill with `mix(surface, colour, 0.55)` and get a 1.5 px ring in the folder
  colour, the collapsed folder's 2×2 preview cells blend 35 % toward it, and
  top-level servers pass `None`.

Discord Rich Presence (`DECISIONS.md` D41):

- `src/rpc.rs` (new) — IPC frame encode/validate, socket candidates (Flatpak,
  Snap, Vesktop, bare runtime dir), handshake, `SET_ACTIVITY`/clear, presence
  field trimming, and a worker thread that reconnects every 15 s and replays the
  last presence; 6 tests.
- `src/settings.rs` — `discord_rpc` (default false), `rpc_client_id` (digits-only,
  falls back to `oauth_client_id`), `rpc_show_channel`.
- `src/app.rs` — `rpc` handle, `started_unix`, `refresh_presence` on construction,
  settings changes and channel selection; presence is guild name + `#channel`.
- `src/ui/settings.rs` — Extras section hosts the switch, application id field and
  channel toggle.
- `src/privacy.rs` — documents the local socket as non-network, opt-in traffic.

Vencord-style extras (`DECISIONS.md` D40):

- `src/settings.rs` — `message_timestamps` and `character_counter`, both default
  true and `#[serde(default)]`-guarded for older settings files.
- `src/ui/settings.rs` — new `SettingsSection::Extras` (label "Extras", ✨) with
  both switches and explanations; `SettingsSection::ALL` is now ten sections.
- `src/ui/chat.rs` — pure `message_timestamp(timestamp, now, long)` returning the
  row stamp and tooltip (Today / Yesterday / `d Mon` / `d Mon yyyy`), and the
  composer counter reading `DiscordClient::MAX_CONTENT_CHARS`; 2 tests.

Translucency and the icon (`DECISIONS.md` D39):

- `src/wallpaper.rs` (new) — wallpaper detection (Hyprland hyprpaper/swww/config/
  Lua-config scan, KDE, GNOME, XFCE), `expand` for `~`/`$HOME`, `blurred` +
  `box_blur`, `cover_uv`; 8 tests.
- `src/app.rs` — `Backdrop`, `GLASS_ALPHA = 205`, `load_backdrop`,
  `refresh_backdrop` (on settings changes), `paint_backdrop` on the background
  layer, transparent `clear_color`; re-detection only on focus loss from the path
  field so typing does not spawn one process per keystroke.
- `src/ui/theme.rs` — `AppTheme::veiled(alpha)`; `src/plugins/vencord_compat.rs` +
  `src/plugins/mod.rs` — `set_veil`, applied last on theme reload.
- `src/settings.rs` — `translucent` (default true) and `wallpaper: Option<String>`
  with normalisation; `src/ui/settings.rs` — glass checkbox and wallpaper field.
- `src/main.rs` — `.with_transparent(true)` and the embedded PNG window icon;
  `assets/rustcord.svg` + `assets/rustcord.png` are the new icon artwork.

Folder colours (`DECISIONS.md` D38):

- `src/rail.rs` — `GuildFolder::color` (optional `#rrggbb`), `FOLDER_COLORS`
  palette assigned round-robin by `new_folder`, `set_folder_color` with
  normalisation and refusal of invalid values; 2 tests.
- `src/ui/sidebar.rs` — folder tiles fill with their colour, white rings for
  expanded/selected instead of repainting, `Folder colour` submenu swatches and
  `RailAction::SetFolderColor`.


Clickable channel peek (`DECISIONS.md` D37):

- `src/ui/sidebar.rs` — `Peek { guild_id, icon, area }` and pure `peek_target`;
  foreground-layer area, clickable channel rows that open the channel, 14 px reach
  so the pointer can cross from the icon to the flyout; 3 tests.
- `src/app.rs` — `rail_peek` state (the flyout as drawn last frame).


Native message log (`DECISIONS.md` D36):

- `src/message_log.rs` — `LogEntry`/`LogKind`/`MessageLog` with edit and delete
  recording, newest-first iteration, a 500-entry cap, filtering and clearing; 6
  tests. Vencord plugins are not loaded: they patch Discord's own bundle.
- `src/app.rs` — snapshots the previous message before `apply_update` and before
  `remove_message`, records into the log, clears it on sign-out, `MessageLog`
  shortcut toggle, navigation stands down while the panel is open.
- `src/ui/log.rs` — filterable panel, channel names resolved, old text plus the
  replacement after an arrow, Clear/Close/Escape.


Server channel peek (`DECISIONS.md` D35):

- `src/ui/sidebar.rs` — `guild_row` returns the hovered icon rect; `hover_peek`
  draws the peek as a tooltip-layer `Area` (server name, text and voice channels,
  unread brightening, mention counts, caps at 8/5 with "and N more").
- `src/app.rs` — `rail_hover` holds the hovered guild and rect for one frame.


Folder renaming (`DECISIONS.md` D34):

- `src/rail.rs` — `rename_folder` (trims, refuses blank, false on unknown id) and
  `FolderRename` (folder id, draft, one-shot focus); 2 tests.
- `src/ui/sidebar.rs` — `Rename folder…` in the folder context menu and
  `rename_dialog`, which saves through `settings_changed()`; `RailAction::RenameFolder`.
- `src/app.rs` — `rail_rename` state; arrow-key navigation stands down while the
  dialog is open.


Keyboard-only navigation (`DECISIONS.md` D33):

- `src/nav.rs` — `Navigator` (rail/channel area, cursors) and `NavKey`/
  `NavOutcome`; wrapping, clamping, area rules; 6 tests.
- `src/app.rs` — `begin_navigation`/`end_navigation`, raw-key handler with guards
  for palettes and focused widgets, activation for folders/guilds/channels;
  searching or switching guilds by pointer ends the mode.
- `src/shortcuts.rs` — `KeyboardNavigation` action, default `Ctrl+Shift+K`.
- `src/ui/sidebar.rs` — white focus ring on rail icons and folder tiles, accent
  ring on channel rows; `RailView::focus` tracks rail item indices.


Guild folders and drag-free ordering (`DECISIONS.md` D32):

- `src/rail.rs` — `GuildRail { order, folders }` with `items`, `move_guild`,
  `move_folder`, `new_folder`, `add_to_folder`, `remove_from_folder`,
  `delete_folder`; stale ids and empty folders degrade instead of breaking; 10
  tests including a JSON roundtrip.
- `src/settings.rs` — `guild_rail` field, saved with everything else.
- `src/ui/sidebar.rs` — rail draws `RailItem`s, folder tiles with 2×2 initials,
  right-click menus for move/add/ungroup, and badges for members.
- `src/app.rs` — `rail_expanded` (session-only expansion state).


Ctrl+K quick switcher (`DECISIONS.md` D31):

- `src/switcher.rs` — `Entry`/`build_entries` (servers, then text and voice
  channels), `filter` (all tokens, prefix-first ranking, 12-row cap) and
  `SwitcherState` (open/query/cursor, wrapping and clamping); 6 tests.
- `src/ui/switcher.rs` — floating overlay with a focused field, arrow keys,
  Enter/click activation and an accent outline on the highlighted row.
- `src/shortcuts.rs` — `QuickSwitcher` action, default `Ctrl+K`, rebindable like
  every other shortcut; `src/app.rs` toggles it, closes search, and jumps via
  `select_channel`.


Unread and mention badges (`DECISIONS.md` D30):

- `src/models.rs` — `Unread`, `UnreadTracker` (per-channel counts, `99+` cap,
  guild aggregation) and `mentions_user`; 5 tests.
- `src/app.rs` — counts `MESSAGE_CREATE` for channels that are not open, skips
  our own messages, and `select_channel` marks read on every path.
- `src/ui/sidebar.rs` — brighter unread label with a left dot, red mention pill
  per channel and per guild icon, white dot for unread-only guilds.


Guild member roster (`DECISIONS.md` D29):

- `src/api/gateway.rs` — `GUILD_MEMBERS` in `DEFAULT_INTENTS`; 4014 close is
  classified, the bit is dropped, `MemberIntentMissing` is emitted and the
  session re-identifies; `is_disallowed_intents` tested.
- `src/api/discord.rs` — `fetch_members` (paginated, 5000 cap), `parse_members`
  (order preserved for the cursor, duplicates dropped), `MEMBER_PAGE_LIMIT`.
- `src/app.rs` — roster cache per guild, one fetch per guild, `MemberResult`
  applied on the UI thread, `member_intent_missing` to stop retries.
- `src/ui/chat.rs` — panel shows the roster, falls back to message authors, or
  explains the missing intent; scrolls.


Token diagnostics (`DECISIONS.md` D28):

- `src/api/discord.rs` — `error_for_status` turns 401 into "this is a user token,
  use a bot token" and 403 into the permission message; tested.
- `src/ui/chat.rs` — the member panel explains why it is empty (members come from
  loaded messages; the full roster needs the privileged Members intent).
- `src/ui/chat.rs` — `history_controls` runs before the empty-channel return, so
  a load failure (the 401) is visible instead of a bare "No messages yet".
- Context: the keyring token on this machine is a user token. The gateway
  accepts it, REST does not, and supporting it is out of scope by D2.


Guild list fixed (`DECISIONS.md` D27):

- `src/api/gateway.rs` — READY's `d.guilds` entries are emitted as
  `GuildCreate` through the same `parse_guild` as the real event; unavailable
  stubs are skipped. Test `ready_carries_whole_guilds` pins the shape.
- `src/app.rs` — a guild that arrives twice updates in place; channels are
  merged by id instead of duplicated.
- `src/ui/sidebar.rs` — the guild rail scrolls, so all 23 icons are reachable.


Reaction chips toggle (`DECISIONS.md` D26):

- `src/api/discord.rs` — `remove_reaction` (DELETE), shared `reaction_path`
  with the empty-emoji guard, one test covering both verbs' path.
- `src/ui/chat.rs` — chips are frameless buttons; hover says add or remove;
  clicking records `RowAction::React` (the handler decides direction).
- `src/app.rs` — `react_to` inspects `me` to pick PUT or DELETE, applies the
  change optimistically, and `ReactFailed { removed }` restores the exact
  previous state.


Right-click context menu, replies and reactions (`DECISIONS.md` D25):

- `src/ui/chat.rs` — rows are click-sensing containers with `context_menu`
  offering quick reactions, copy, reply, edit, delete; reply preview line; reply
  banner in the composer with Esc/✕ cancel.
- `src/api/discord.rs` — `add_reaction` (PUT, percent-encoded emoji path),
  `send_reply` / `send_message_with_reference`, `encode_path_segment`, and
  `referenced_message` → `ReplyRef` mapping.
- `src/models.rs` — `ReplyRef`, char-safe `snippet`, `Message::reply_to`,
  `add_own_reaction` / `remove_own_reaction` for the optimistic update.
- `src/app.rs` — `replying_to`, `begin_reply`/`cancel_reply`, `react_to` with
  `MessageAction::ReactFailed` rollback, reply-aware send.
- `src/api/gateway.rs` — inbound `referenced_message` parsing; `GatewayEvent::Message`
  is boxed to keep the enum small.


Messaging actions and voice presence (`DECISIONS.md` D24):

- `src/api/discord.rs` — `edit_message` (PATCH) and `delete_message` (DELETE, 204),
  plus the pure `valid_content` rule (1–2000 **characters** after trimming) and
  four tests, including a 2000-emoji case that proves the limit is not bytes.
- `src/app.rs` — `editing: Option<String>` reuses the composer for edit mode;
  `pending_delete` asks before deleting; `MessageAction` results come back over
  the existing `mpsc` pattern and are applied by `poll_action_results`
  (edited message replaces the row, deleted one is removed, failures go to the
  status line). `can_modify` limits the menu to the bot's own messages.
- `src/api/gateway.rs` — `VOICE_STATE_UPDATE` → `GatewayEvent::VoiceState`,
  with `member.user.username` (guild) or `user.username` (DM) and `null`
  `channel_id` meaning left; JSON test covers join, leave and a malformed payload.
- `src/models.rs` — pure `VoiceTracker` (one channel per user, 50-per-channel cap,
  leave on `null`) with six tests.
- `src/ui/sidebar.rs` — `voice_member_row` lists members under each voice channel.
- `src/ui/chat.rs` — row `⋯` menu (Edit/Delete), `RowAction` returned through the
  viewport closure, delete confirmation banner, "Editing message · Esc cancels".
- Voice **audio** is still absent on purpose (voice server, UDP/RTP, Opus,
  DAVE/E2EE); there are no mute/join buttons that would do nothing.


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

## Local-only invariants

Do not add a database, a cache directory or a telemetry crate without changing
`DECISIONS.md` D22 deliberately. `src/privacy.rs` is the allowlist every network
path goes through, and its tests fail if the store list mentions a database or
the CDN lists disagree. `scripts/check-local.sh` is the quick check before a
release. `api::spotify` is unreachable on purpose — if it is ever wired up, its
host and its setting have to be disclosed first.

## "Log in as me" requests

Expect this again. `DECISIONS.md` D21 has the answer with sources: self-bots are
forbidden and can terminate an account, `messages.read` is RPC-only, and user DMs
need the approval-gated Social SDK. What the login page offers instead is the
legitimate path — create app, invite the bot with exactly the six permissions the
client uses, paste the token, optionally connect identity with `identify`.
Do not add a password field, do not read the official client's files, and do not
put a user token on the gateway.

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
