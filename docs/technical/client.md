---
title: Client
parent: Technical Reference
nav_order: 3
---

# JavaScript Client

## Overview

The JellyWatchParty client is a set of plain JavaScript modules (each an
IIFE that extends the global `window.JellyWatchParty` object, `JWP` below)
injected into Jellyfin's web interface (Jellyfin Web and Jellyfin
Desktop). The source lives in `src/clients/jellyfin-web/`; the build copies
it into the plugin's `Web/` folder, where it is embedded and served by
`GET /JellyWatchParty/Client/{*path}` (see
[Plugin]({{ '/technical/plugin/' | relative_url }})).

## Module Architecture

```
plugin.js                    # Loader
    ├── state.js             # JWP.constants and JWP.state
    ├── utils/               # JWP.utils
    │   ├── time.js, video.js, misc.js, validation.js, media.js, log.js
    ├── ui/                  # JWP.ui
    │   ├── styles.js, indicators.js, toasts.js, modal.js, cards.js, bridge.js
    │   ├── home.js, render.js
    ├── playback/            # JWP.playback
    │   ├── play.js, bind.js, sync.js, tracks.js
    ├── chat/                # JWP.chat
    │   ├── messages.js, input.js
    ├── ws/                  # JWP.actions and JWP._wsHandlers
    │   ├── send.js, auth.js, connection.js
    │   └── handlers/
    │       ├── room.js, sync.js, playback.js, clock.js
    └── app/                 # JWP.app
        ├── lifecycle.js, cleanup.js
```

`plugin.js` is loaded by the injected `<script>` tag
(`/JellyWatchParty/ClientScript`). It is guarded by `JWP.__loaded`, takes
the base path and the `?v=` cache-buster from its own `src`, and loads each
module from `<base>/JellyWatchParty/Client/<path>?v=…` (10 s timeout per
script) in dependency waves, in parallel within a wave:

`state.js` → `utils/{time,video,misc,validation}` → `utils/{media,log}` →
`ui/{styles,indicators,toasts,modal,cards,bridge}` → `ui/{home,render}` →
`playback/*` → `chat/*` → `ws/send` → `ws/auth` → `ws/handlers/*` →
`ws/connection` → `app/lifecycle` → `app/cleanup`, then `JWP.app.init()`.

## Module: `state.js`

Defines shared state and constants. Guarded by `if (JWP.state) return;`,
so re-injection is a no-op.

### Constants (`JWP.constants`)

| Constant | Value | Description |
|----------|-------|-------------|
| `SUPPRESS_MS` | `2000` | Default anti-feedback window (`suppress()`, `startSyncing()`) (ms) |
| `CLIENT_CAPABILITIES` | `['democratic']` | Sent as `capabilities` in `auth`, so the server lets this client control democratic rooms |
| `START_SAFETY_MS` | `15000` | Host: give up waiting for the start countdown and just play (ms) |
| `TRACK_SWITCH_SUPPRESS_MS` | `8000` | Safety-net suppression for an audio/subtitle switch (ms) |
| `SEEK_THRESHOLD` | `1.0` | Position difference that triggers a seek / counts as a seek (s) |
| `COMMAND_GUARD_MS` | `3000` | Democratic mode: how long a sent/applied play/pause/seek overrides disagreeing `state_update`s (ms) |
| `COMMAND_GUARD_TOLERANCE_SEC` | `2.0` | How far a `state_update` may be from the guarded command and still agree (s) |
| `FOLLOWER_LOAD_MAX_MS` | `15000` | Guest: longest time a loading stream's own play/seek events are never treated as commands (ms) |
| `STATE_UPDATE_MS` | `1000` | Host `state_update` interval (ms) |
| `SYNC_LEAD_MS` | `300` | Latency compensation lead (ms) |
| `DRIFT_CORRECTION_ENTER_SEC` | `0.3` | Drift needed to start a correction burst (s) |
| `DRIFT_CORRECTION_EXIT_SEC` | `0.1` | Drift must fall under this to stop correcting (s) |
| `DRIFT_SOFT_MAX_SEC` | `2.0` | Hard-seek threshold (s) |
| `PLAYBACK_RATE_MIN` / `MAX` | `0.85` / `2.0` | Catch-up playback-rate clamp |
| `DRIFT_GAIN` | `0.50` | Sqrt-curve proportional gain |
| `UI_CHECK_MS` | `2000` | UI / video-binding poll interval (ms) |
| `PING_INIT_MS` | `2000` | Ping interval before clock sync stabilizes (ms) |
| `PING_STABLE_MS` | `30000` | Ping interval after stabilizing (ms) |
| `PING_STABLE_AFTER` | `5` | Successful pongs before switching to the stable interval |
| `HOME_REFRESH_MS` | `5000` | Home "Watch Parties" refresh interval (ms) |
| `SYNC_LOOP_MS` | `500` | Drift-correction / status-report loop interval (ms) |
| `RECONNECT_BASE_MS` / `MAX_MS` | `1000` / `30000` | Exponential-backoff reconnect delay bounds (ms) |
| `INITIAL_SYNC_COOLDOWN_MS` | `8000` | After joining, no hard seek for this long (ms) |
| `INITIAL_SYNC_MAX_MS` | `30000` | Max duration of the initial-sync phase (ms) |
| `INITIAL_SYNC_DRIFT_THRESHOLD` | `0.5` | Drift under which initial sync ends early (s) |
| `INITIAL_SYNC_MAX_DRIFT` | `10` | Drift that forces a seek during initial sync (s) |
| `TIME_SYNC_MAX_SAMPLES` | `8` | Clock-sync sliding-window sample count |
| `TIME_SYNC_EMA_ALPHA` | `0.4` | Clock-sync EMA smoothing coefficient |

Also: DOM ids `PANEL_ID` (`jwp-panel`), `BTN_ID` (`jwp-osd-btn`),
`STYLE_ID` (`jwp-style`), `SYNC_HIDE_STYLE_ID` (`jwp-hide-native-sync`),
`HOME_SECTION_ID` (`jwp-home-section`), and `DEFAULT_WS_URL`
(`ws(s)://<page host>:3000/ws`, scheme matching the page's protocol), used
when the plugin's `SessionServerUrl` is empty.

### State (`JWP.state`)

Notable fields beyond the obvious (`ws`, `roomId`, `roomName`, `clientId`,
`isHost`, `inRoom`, `rooms`, `participants`, `participantCount`):

- **Clock sync:** `serverOffsetMs`, `hasTimeSync`, `timeSyncSamples`,
  `successfulPings`.
- **Sync baseline:** `lastSyncServerTs` / `lastSyncPosition` /
  `lastSyncPlayState`, `isDriftCorrecting` (hysteresis latch),
  `syncCooldownUntil`, `isInitialSync` / `initialSyncUntil` /
  `initialSyncTargetPos`, `syncStatus` (`'unknown'`, `'synced'`,
  `'syncing'`, `'pending_play'`, `'wrong_media'`; drives the indicator),
  `currentDrift`, `pendingPlayUntil`, `isSyncing`, `suppressUntil`.
- **Room media:** `roomMediaId` (mirrors the server's `media_id`, `''`
  for none), `mediaSwitchPending` / `mediaSwitchPendingUntil` (host
  confirming a new item before `set_media`), `serverNowPlayingId` (item
  this browser's session plays according to `GET /Sessions`),
  `playCommandItemId` / `playCommandUntil` (PlayNow de-duplication),
  `joiningItemId`, `pendingJoinRoomId` (join once the player opens),
  `readyRoomId`.
- **Start countdown:** `roomStarted` (defaults to `true` for servers that
  don't send `started`), `startPending`, `startQueued`,
  `startSafetyTimer`.
- **Democratic mode:** `democraticMode` / `deviceControl` (from
  `room_state` and `democratic_changed`), `commandGuardUntil` /
  `commandGuardPlayState` / `commandGuardPosition` /
  `commandGuardServerTs`, `followerLoadingUntil`.
- **Status reports:** `lastReportedStatus`, `lastStatusReportAt`.
- **Plugin flags** (from `/JellyWatchParty/Token`): `hideNativeSyncButton`,
  `allowThirdPartyHost`, `allowSupportedReceiver`, `allowDemocraticMode`.
- **Auth:** `authToken`, `authEnabled`, `userId`, `userName`,
  `tokenExpiresAt`, `tokenRefreshTimer`, `wsUrl`.
- **Housekeeping:** `intervals: { ui, ping, home, sync, stateUpdate }`,
  `videoListeners` / `currentVideoElement`, `reconnectAttempts`,
  `logBuffer` / `logBufferMax` (100), `homeRoomCache` (`LRUCache` of 50).

## Module: `utils/` — Shared Helpers

### `utils/time.js`
`nowMs()`, `getServerNow()` (`nowMs() + serverOffsetMs`),
`adjustedPosition(position, serverTs)` (adds the time elapsed since
`serverTs` plus `SYNC_LEAD_MS`), `scheduleAt(serverTs, fn)` (clears any
pending action timer, then runs `fn` now if the time has passed, else
after the remainder), and `startSyncing(ms = SUPPRESS_MS)` (sets the
`isSyncing` anti-feedback lock).

### `utils/misc.js`
`shouldSend()` (outside the `suppress()` window), `suppress(ms =
SUPPRESS_MS)`, `escapeHtml(str)`, `getItemImageUrl(itemId, imageTag)`,
`isHomeView()`, and `buildAuthHeader(apiClient, token)`: builds an
`Authorization: MediaBrowser Client=…, Device=…, DeviceId=…, Version=…,
Token=…` header (parts ApiClient doesn't expose are omitted). Jellyfin 12
rejects `X-Emby-Token` unless legacy authorization is enabled, so every
request to Jellyfin uses this header (with `X-Emby-Token` alongside).

Democratic mode helpers: `canControlPlayback()` (host, or
`democraticMode && roomStarted`), `expectedRoomPosition()` (from the sync
baseline), `mirrorRoomState(position, playState)` (takes a command this
client just sent as the new baseline, since the server doesn't echo it to
the sender), `armCommandGuard(position, playState)` /
`commandGuardBlocks(msg)` (for `COMMAND_GUARD_MS` ignore `state_update`s
that disagree with the command; the first that agrees ends the guard), and
`isEchoOfRoomState(action, video)` (an event that only brings this client
in line with the room, e.g. a slow seek finishing after the suppression
window, is not a new command).

### `utils/validation.js`
`validateWsUrl(url)` returns human-readable warnings (not a URL, scheme
not `ws:`/`wss:`, `ws://` on an `https://` page, a bare internal/Docker
hostname). `ws/connection.js` logs them and toasts the first one; only an
admin-configured URL is checked.

### `utils/media.js`
`getCurrentItemId()` finds the playing item by trying, in order:
`window` globals (`NowPlayingItem`, `Emby.Page.currentItem`,
`appRouter.currentRouteInfo.options.item`, `sessionStorage.playbackInfo`,
then the playback manager's current item), DOM `data-id`/`data-itemid`
attributes in the OSD (skipping hidden `.page.hide` pages, which keep the
previous item's ids), the URL hash, and finally `state.serverNowPlayingId`.
`getCurrentItem()` asks the playback manager directly.

Jellyfin 12.1+ web builds don't expose a global `playbackManager`, so the
module can also ask the server: `getOwnSession()` (`GET
/Sessions?deviceId=…`, this browser's session id and now-playing item),
`refreshServerNowPlaying()` / `clearServerNowPlaying()`, and
`resolveCurrentItemId({retries = 4, delayMs = 500})` (local detection,
else the server, retried because it only knows the item after the first
progress report). Also `apiFetch(path, options)` (authenticated fetch via
`ApiClient` using `buildAuthHeader`), `normalizeItemId(id)` (dashed or not
→ 32 lowercase hex chars, else `null`) and `isOnRoomMedia()` (this
client's item is the room's `roomMediaId`, or the room has none).

### `utils/log.js`
`log(category, data)` formats `[JWP:CATEGORY] key=val ...` (unit-aware
for position/rate/offset/delay/rtt keys), logs it to the console, and
sends it to the server as `client_log` when connected; while disconnected
up to `logBufferMax` (100) entries are buffered and flushed by
`flushLogBuffer()` once the socket opens.

### `utils/video.js` {#module-utilsvideojs}
`getVideo()`, `isVideoReady()` (`readyState >= 3`), `isBuffering()`,
`isSeeking()`, `getPlaybackManager()` (`window.playbackManager`,
`window.PlaybackManager` or `window.app.playbackManager`).

`getVideo()` returns `document.querySelector('video')` when there is one.
Jellyfin Desktop's CEF+mpv player (formerly Jellyfin Media Player) never
creates a DOM `<video>`; it exposes `window._mpvVideoPlayerInstance`
instead. When that instance exists and no `<video>` does, `getVideo()`
wraps it in a **native adapter**: an `HTMLMediaElement`-shaped object
(`currentTime`, `paused`, `playbackRate`, `readyState`, `seeking`,
`play()`/`pause()`, `add`/`removeEventListener`) that polls the mpv
instance every `NATIVE_POLL_MS` (250 ms) and synthesizes
`loadeddata`/`canplay`, `play`/`playing`/`pause`, and `seeked` (on a jump
over 1 s), so `playback/bind.js` and `playback/sync.js` work unmodified.

Known limitation: mpv only reports coarse
`playing`/`pause`/`unpause`/`stopped`/`volumechange`/`error` signals, with
no equivalent of HTML5 `waiting`, so buffering can't be detected on the
adapter, and its `readyState`/`networkState` are approximations.

## Module: `ui/` — Interface

### `ui/render.js`
`render(forceFullRender?)` — the main panel render. If the panel already
shows the right view (lobby vs. room) it only refreshes indicators and
lists; host changes and democratic-mode changes force a full render.

- `renderLobby(panel)`: room list, **Create Room** (asks for an optional
  password with `ui.promptText`), the server footer, and, only when
  `allowThirdPartyHost` is set, the *Host From Another Device (e.g.
  Fladder)* bridge section.
- `renderRoom(panel)`: room name, **Close room** (host) or **Leave**,
  participant list, sync indicator, the democratic-mode block, chat, RTT
  and client id, and, only when `allowSupportedReceiver` is set, the *Add a
  Device to This Room* bridge section (see
  [Host Bridge]({{ '/technical/host-bridge/' | relative_url }})).
- Democratic block (`buildDemocraticHtml`): for the host an *Everyone can
  control playback* checkbox when `allowDemocraticMode` is set or the room
  already is democratic (so a host can turn off what an admin turned on),
  which calls `actions.setDemocratic()`; for everyone an info line while
  the mode is on (*(TV devices too)* with device control).
- Panel dismissal: an X button, Escape (captured, so Jellyfin doesn't
  treat it as "back"; ignored while a password prompt is open) and a click
  outside the panel (`setupPanelDismiss()` / `teardownPanelDismiss()`).
- `applyNativeSyncButtonVisibility()`: when `hideNativeSyncButton` is set,
  injects a `<style id="jwp-hide-native-sync">` hiding `.headerSyncButton`,
  `.syncButton` and `[aria-controls="app-sync-play-menu"]` (the MUI
  SyncPlay button). CSS survives Jellyfin's header re-renders.

**Buttons.** `injectOsdButton()` adds a button to the video OSD
(`.videoOsdBottom .buttons`, before the favourite button).
`injectGlobalButton()` adds a persistent toolbar button (`#jwp-global-btn`)
so the panel is reachable without a video: it tries
`tryInjectLegacyHeader()` first (`.headerRight`, used by the legacy
layout; on the default Jellyfin 12 layout `.headerRight` still exists but
sits in a hidden subtree, so it is only used when actually rendered) and
falls back to `tryInjectMuiToolbar()` for the default React/MUI layout.

On the MUI layout the button is a real in-flow child of the toolbar's
actions `Box`, the flex container that holds SyncPlay, RemotePlay and
Search. `findMuiActionsBox()` locates it as the toolbar child immediately
preceding the user-menu avatar's `Box` (the avatar,
`aria-controls="app-user-menu"`, is the only stable selector). React
reconciles against its own fiber tree and only removes nodes it created,
and jellyfin-web mounts with `createRoot` (no hydration), so the foreign
button survives route changes; `injectGlobalButton()` re-appends it if
React re-orders the actions, and moves a stranded legacy button to the
MUI toolbar after a live layout switch.

`findDonorButton()` copies the class list of a neighbouring MUI
`IconButton`, since MUI keeps its real styling in emotion-generated hash
classes; `.jwp-global-btn-standalone` is the fallback when there is no
donor. `findMuiActionsBox()` returns `null` on the video OSD and public
pages (no avatar), and the admin dashboard is excluded via the
`dashboardDocument` class on `document.body`, so the button is absent
there.

`observeToolbar()` / `disconnectToolbarObserver()` drive re-injection
from a `MutationObserver` on `document.body`, coalesced through
`requestAnimationFrame` (one React commit emits many records, and
`injectGlobalButton()` itself mutates the DOM). The `UI_CHECK_MS` poll in
`app/lifecycle.js` is a safety net for the initial mount and environments
without `MutationObserver`.

### `ui/cards.js` + `ui/home.js`
The lobby room list (`updateRoomListUI()`: lock icon for password rooms,
`how_to_vote` icon for democratic rooms) and the "Watch Parties" section on
the Jellyfin home page. `renderHomeWatchParties()` reconciles cards against
`state.rooms` (adds/removes cards, updates counts, rebuilds a card when its
`media_id` changed). `createRoomCard()` fetches the item's title and poster
through `ApiClient.getItem`. A card's play button sets `pendingJoinRoomId`
and calls `openDetailsAndPlay(mediaId)` (navigate to the item's details
page and click its Play button once rendered, polling up to 5 s); a room
without media is joined directly. `promptJoinWithPassword(roomId)` asks for
a password and joins (also used after a `wrong_password` error).

### `ui/indicators.js`
`updateStatusIndicator()` (Online/Offline), `updateServerFooter()`,
`buildSyncStatusIndicator()` / `updateSyncIndicator()` (*In sync*, *Out of
sync*, *Waiting for sync... n.ns* with a spinner, *Loading host's media…*,
*Not synced yet*; hidden for the host), `buildParticipantsHtml()` /
`renderParticipants()` (names, host star, *(you)*, and each member's
reported status: *In sync*, *Playing*, *Catching up*, *Buffering*,
*Loading*, *Paused*, *Not watching*), the start countdown overlay
(`showStartWaiting()` "Waiting for everyone to be ready...",
`showCountdown(targetServerTs)` 3-2-1 against server time,
`hideCountdown()`), and `stopPlayerCapture(input)` (keeps keystrokes in
inputs from reaching the player).

### `ui/modal.js`
`promptText({title, placeholder, submitLabel, inputType = 'password'})` —
a promise-based in-DOM replacement for `window.prompt()` (which embedded
clients such as Jellyfin Desktop don't show). Resolves with the trimmed
value, or `null` on cancel.

### `ui/toasts.js`
`showToast(message)` (centered, dismissed after 1.5 s) and
`showChatToast(username, text)` (top-right, 5 s, at most 5 visible, oldest
dismissed first).

### `ui/styles.js`
`injectStyles()` adds the plugin's CSS as one `<style id="jwp-style">`.

### `ui/bridge.js`
`updateBridgeListUI()` fills the bridge sections rendered by
`ui/render.js` (and does nothing when neither is rendered) from `GET
/JellyWatchParty/Bridge/Sessions` and `GET /JellyWatchParty/Bridge/Status`.
Each eligible session gets **Host** in the lobby (`POST
Bridge/{id}/Start`) or **Receiver** in a room (`POST
Bridge/{id}/Follow?roomId=<state.roomId>`); active bridges show their role
and a **Stop** button. All calls use `utils.apiFetch`. See
[Host Bridge]({{ '/technical/host-bridge/' | relative_url }}).

## Module: `playback/` — Video Binding and Sync

### `playback/bind.js`
`bindVideo()` attaches listeners to the active video element
(`waiting`/`canplay`/`playing`/`play`/`pause`/`seeked`/`loadstart`),
rebinding when the element changes, and starts a `STATE_UPDATE_MS`
interval that calls `sendStateUpdate()` while hosting.
`cleanupVideoListeners()` tears this down.

`sendStateUpdate()` (host) sends `state_update` unless syncing, a media
switch is pending, the start countdown is pending, seeking, buffering or
not ready, and mirrors the sent state into the sync baseline.

`onHostEvent(action, video)` handles play/pause/seek. It ignores events
inside the `suppress()` window or while `isSyncing`; a guest's events go
to `sendGuestCommand` (below). For the host:

- **Start countdown:** the room's first `play` (while `!roomStarted`)
  pauses the video and sends `player_event` `play` (`requestStart`); the
  server answers with a scheduled play, possibly after a countdown. If a
  media switch is still being confirmed, the start is queued until
  `set_media` is out. After `START_SAFETY_MS` without an answer the host
  just plays.
- Nothing is sent while a media switch is pending; seeks need a ready
  video; in a democratic room events that only match the room's state
  (`isEchoOfRoomState`) are skipped; pause is ignored while buffering or
  seeking, play while seeking; seeks are debounced (250 ms) and must differ
  from the last sent position by `SEEK_THRESHOLD`.
- Each sent `player_event` is followed by a `state_update`.

A `waiting` event makes the host send a `buffering` `player_event`, and
`playing` after buffering sends `play`.

**Media switches:** `loadstart` (and binding) start
`beginMediaResolution()`: the host suppresses broadcasts, waits 1.5 s,
refreshes the server-side now-playing item and, if the item differs from
`roomMediaId`, sends `set_media` (`maybeSendSetMedia`). Without a global
playback manager only the server-confirmed id is trusted. The pending
state times out after 5 s.

**Guest commands** (`sendGuestCommand`, only when `canControlPlayback()`):
sends only `play`, `pause` or `seek` (never `state_update`, `buffering`,
`set_media` or the start play), and not while a start is pending, while
not on the room's item, while its stream is loading
(`followerLoadingUntil`: from `loadstart`/bind, cut to `SUPPRESS_MS` once
it can play), before it has a sync baseline, while buffering (pause) or
seeking (play/pause), or when the event echoes the room's state. After
sending it mirrors the new state and arms the command guard. Outside
democratic mode a guest's pause while the room plays shows "Only the host
controls playback" (`onGuestPause`, at most every 10 s).

### `playback/play.js`
`ensurePlayback(itemId, startPos?, attempt?)` is how a guest loads the
room's media (on join and on `media_changed`); it no-ops if the item is
already playing or being loaded.

- With a playback manager: `ApiClient.getItem`, then `playItem(item,
  startPositionTicks)`, which tries `pm.play({items})`, `pm.play({item})`,
  `pm.play({ids})` and `pm.playItems()` in turn; failures are retried up
  to 5 times at 500 ms.
- Without one (Jellyfin 12.1+): `playViaSessionCommand()` asks the server
  to send a `PlayNow` command to this browser's own session (`POST
  /Sessions/{id}/Playing?playCommand=PlayNow&itemIds=…&startPositionTicks=…`;
  skipped if the session already plays the item, de-duplicated for 15 s).
  After two retries it falls back to `ui.openDetailsAndPlay()`.

### `playback/sync.js`
`watchReady()` / `notifyReady()` send `ready` (with `media_id`) once the
room's item can play (`readyState >= 2`); never against another item.
`syncLoop()` also reports ready as soon as possible, so the server doesn't
hold back the room's plays.

`syncLoop()` (every `SYNC_LOOP_MS` for guests) is the hysteresis drift
controller described in [Sync Algorithms]({{ '/technical/sync/' | relative_url }}): it resets to 1x
when not applicable (host, not in a room, not on the room's item, which
sets `syncStatus = 'wrong_media'`, no baseline, room not playing, video
paused) and skips while buffering. Otherwise it computes `drift = expected
- video.currentTime`, starts a correction burst once `|drift|` exceeds
`DRIFT_CORRECTION_ENTER_SEC` and holds it until `|drift|` falls under
`DRIFT_CORRECTION_EXIT_SEC`; at `DRIFT_SOFT_MAX_SEC` it hard-seeks, except
during initial sync or the post-command cooldown. The **initial-sync
phase** after joining (`checkInitialSync`) seeks once if Jellyfin's own
resume jump moved the video away from the join target, seeks on drift
above `INITIAL_SYNC_MAX_DRIFT`, and ends when drift is under
`INITIAL_SYNC_DRIFT_THRESHOLD` or after `INITIAL_SYNC_MAX_MS`.

`computeOwnStatus()` / `reportOwnStatus()` send the client's own status as
`client_status` (`idle`, `buffering`, `playing`/`paused` for the host,
`synced`, `syncing`, `loading` for guests) when it changes, at most once
per second.

### `playback/tracks.js`
Audio and subtitle tracks are outside the sync protocol: everyone picks
their own. A track switch can make Jellyfin reload the stream and fire the
same `waiting`/`pause`/`seeked`/`play` events `bind.js` listens for, so
`patchTrackSwitching()` wraps `playbackManager.setAudioStreamIndex` /
`setSubtitleStreamIndex` (feature-detected, idempotent, called from the UI
poll) to engage the `isSyncing` lock for `TRACK_SWITCH_SUPPRESS_MS` when
this client's events are sent to the room (host, or anyone in a democratic
room). A one-shot `canplay`/`playing` listener shortens the window back to
`SUPPRESS_MS` once the reload finishes. Without a global playback manager,
`onStreamReload()` (called on `loadstart`) does the same. See
[Sync Algorithms]({{ '/technical/sync/' | relative_url }}) §5A.

## Module: `chat/` — Text Chat

### `chat/input.js`
`send(text)` trims, checks length (`MAX_MESSAGE_LENGTH` 500), connection
and room, then sends `chat_message`. `isChatVisible()` / `markRead()` /
`updateBadge()` manage the unread badge (shown as `99+` above 99).

### `chat/messages.js`
`receive(msg)` appends a live message (capped at `MAX_MESSAGES` 100), and
if the panel is hidden bumps the unread count and shows a chat toast
(not for own messages). `hydrate(entries)` replaces the history with the
server's `chat_history` from `room_state`, without badge or toasts.
`renderAllMessages()` / `clear()` handle full re-render and room-leave
cleanup.

## Module: `ws/` — WebSocket Communication

### `ws/send.js`
`send(type, payload?, roomOverride?)` — the low-level sender (adds `room`,
`ts` and, once known, `client`). Built on it:

- `createRoom(password?)`: resolves the current item (asking the server if
  needed) and sends `create_room` with `start_pos`, `media_id`,
  `user_name` and `started` (already playing: no start countdown).
- `joinRoom(id, password?)`: `join_room` with `user_name`.
- `leaveRoom()`: `leave_room`, then `resetRoomState()` (room, media,
  countdown, democratic, sync and chat state) and hides the panel.
- `setDemocratic(enabled)`: `set_democratic` (host only; everyone gets
  `democratic_changed`).

### `ws/auth.js`
`fetchAuthToken()` calls `GET /JellyWatchParty/Token` with
`buildAuthHeader` (plus `X-Emby-Token`), waiting up to 10 s for
`window.ApiClient`. It stores `auth_enabled`, `user_id`, `user_name`,
`session_server_url` (as `wsUrl`) and the four plugin flags
(`hide_native_sync_button`, `allow_third_party_host`,
`allow_supported_receiver`, `allow_democratic_mode`), applies the native
SyncPlay button setting, and, when auth is enabled, keeps the JWT and
schedules a refresh `min(5 min, 20 % of expires_in)` before expiry
(`scheduleTokenRefresh`). The refresh sends a new `auth` over the open
socket instead of reconnecting. On failure the username falls back to
what `ApiClient`/stored credentials know.

### `ws/connection.js`
`connect()` fetches a token if needed, then opens `wsUrl ||
DEFAULT_WS_URL` with `?client_id=<id>&resume=<secret>`. The client id and
resume secret are kept per tab in `sessionStorage`
(`owp_persistent_client_id`, `owp_resume_secret`; old `localStorage` copies
are removed), and `rememberSession()` stores the id and secret from every
`client_hello`. Presenting the secret lets the server reattach a
reconnecting tab to its room and host role (see
[Server: Persistent Client ID]({{ '/technical/server/' | relative_url }}#persistent-client-id)). The
resume secret is never logged.

On open it sends `auth` (`token`, `user_name`, `user_id`, `capabilities:
CLIENT_CAPABILITIES`; only when there is an identity, since a bare `auth`
is refused where JWT is required) and a first `ping`, then pings on the
adaptive schedule (`schedulePing`). `onWsClose` reconnects with exponential
backoff (`RECONNECT_BASE_MS` doubling up to `RECONNECT_MAX_MS`). While in
a room, messages for other rooms are dropped (except `room_state`).
`handleMessage` dispatches to `JWP._wsHandlers`:

| Message | Handler |
|---|---|
| `room_list` | `handleRoomList` |
| `client_hello` | `handleClientHello` |
| `room_state` | `handleRoomState` |
| `participants_update`, `participants`, `client_left` | `handleParticipantsUpdate`, `handleParticipants`, `handleClientLeft` |
| `room_closed`, `host_changed`, `democratic_changed` | `handleRoomClosed`, `handleHostChanged`, `handleDemocraticChanged` |
| `media_changed` | `handleMediaChanged` |
| `player_event`, `start_pending`, `state_update` | `handlePlayerEvent`, `handleStartPending`, `handleStateUpdate` |
| `pong` | `handlePong` |
| `chat_message` | `JWP.chat.receive` |
| `error` | `handleError` |

### `ws/handlers/room.js`
`handleRoomList`; `handleClientHello` (stores the server-assigned
`client_id`, remembers the session, and resets local room state with a
toast if the server says this client is no longer in a room);
`handleParticipants` (the full list) / `handleParticipantsUpdate` /
`handleClientLeft` (counts and join/leave toasts); `handleRoomClosed`
(full local reset, toast with the reason); `handleHostChanged` ("You are
now the host" / "<name> is now the host", full render);
`handleDemocraticChanged` (stores `democraticMode`/`deviceControl`, toasts
who turned the mode on or off, clears the command guard when it goes off,
full render); `handleError` (toast; clears an optimistic `roomId` on
`room_not_found`; prompts for a password on `wrong_password`).

### `ws/handlers/playback.js`
`handlePlayerEvent` applies a relayed `play`/`pause`/`seek`/`buffering`.
The host normally ignores relayed events, except the answer to its own
start request (`handleHostStart`: countdown, then play at
`target_server_ts`) and, in democratic mode, other members' discrete
commands (`hostApplies`: not its own, not mid media switch, not a `play`
while already playing). Nothing is applied while not on the room's item.
Otherwise it starts the `isSyncing` lock, hard-corrects the position if
the gap exceeds `SEEK_THRESHOLD`, and dispatches: `play` schedules
`video.play()` at `target_server_ts` (`syncStatus = 'pending_play'`
meanwhile, countdown overlay if `countdown` is set), `pause` pauses and
resets sync state, `seek` jumps (and plays when `play_state` is
`playing`), `buffering` pauses. Toasts name `payload.by_name` ("Alice
paused playback"; "Host" when missing) and stay quiet for this client's
own command (`by` equals its id). In democratic mode each discrete command
arms the command guard. `handleStartPending` shows "Waiting for everyone
to be ready...".

### `ws/handlers/sync.js`
`handleRoomState` applies the full room state on join/reattach (resetting
first if an admin moved the client to a different room, with a toast for
`admin_moved`): host detection, `media_id`, `started`, `democratic` and
`device_control` (missing: off), chat history, an initial clock offset if
not yet synced, and the sync baseline. Guests then seek/play/pause to the
room (starting the initial-sync phase when it plays), call
`ensurePlayback` and `watchReady`.

`handleMediaChanged` (guests): stores the new `roomMediaId`, resets sync
state and readiness, toasts "Host switched media", and loads the item like
a join.

`handleStateUpdate` (guests, on the room's item only): ignored while the
command guard disagrees (`commandGuardBlocks`); resumes or pauses the
video to match `play_state`, and otherwise updates the sync baseline
unless buffering or inside the post-command cooldown.

### `ws/handlers/clock.js`
`handlePong` computes RTT (shown in the panel) and a candidate offset,
keeps the lowest-RTT sample of the last `TIME_SYNC_MAX_SAMPLES`, and folds
it into `serverOffsetMs` via EMA (`TIME_SYNC_EMA_ALPHA`) once synced. Ping
frequency adapts: `PING_INIT_MS` until `PING_STABLE_AFTER` pongs, then
`PING_STABLE_MS`, back to fast pinging if the offset moves by more than
50 ms.

## Module: `app/` — Initialization and Cleanup

### `app/lifecycle.js`
`init()` (once): injects styles, creates the hidden panel (which stops
click/key events from reaching Jellyfin), sets up panel dismissal and the
toolbar observer, connects, and starts the tracked intervals:

- `ui` (every `UI_CHECK_MS`, visible tab only): with a video, injects the
  OSD button, binds the video, patches track switching, and joins
  `pendingJoinRoomId` (prompting first for a password room). When the
  video disappears, `onVideoPlayerExit()` hides the panel, makes a
  **guest** leave the room (the host stays, so it can browse to the next
  item and `set_media` it), and cleans up video listeners and the cached
  now-playing id. It also calls `ui.injectGlobalButton()` as a safety
  net.
- `home` (every `HOME_REFRESH_MS`): re-renders the home "Watch Parties".
- `sync` (every `SYNC_LOOP_MS`): `syncLoop()` for guests and
  `reportOwnStatus()` for everyone in a room.

### `app/cleanup.js`
`cleanup()` — full teardown: clears all intervals and the pending-action
timer, disconnects the toolbar observer and panel-dismiss listeners,
closes the WebSocket, removes panel and video listeners, and resets
`bound`/`initialized`.

## Synchronization Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                          HOST                                    │
├─────────────────────────────────────────────────────────────────┤
│  [User clicks Play]                                              │
│        │                                                         │
│        ▼                                                         │
│  onHostEvent('play')                                             │
│        │                                                         │
│        ├── Checks: shouldSend? !isSyncing? isHost? room started?│
│        │           no media switch pending? not seeking?        │
│        ▼                                                         │
│  send('player_event', {action:'play', position, play_state})    │
│        │                                                         │
└────────┼────────────────────────────────────────────────────────┘
         │
         ▼ WebSocket
┌─────────────────────────────────────────────────────────────────┐
│                        SERVER                                    │
├─────────────────────────────────────────────────────────────────┤
│  Receives player_event                                          │
│        │                                                         │
│        ├── Validates: may_control (host, or democratic member)  │
│        ├── Updates room.state                                   │
│        ├── Sets last_command_ts (cooldown)                      │
│        │                                                         │
│        ▼                                                         │
│  Broadcasts with target_server_ts = now + PLAY_SCHEDULE_MS      │
│        │                                                         │
└────────┼────────────────────────────────────────────────────────┘
         │
         ▼ WebSocket
┌─────────────────────────────────────────────────────────────────┐
│                       NON-HOST CLIENT                            │
├─────────────────────────────────────────────────────────────────┤
│  handleMessage('player_event')                                  │
│        │                                                         │
│        ├── startSyncing() → isSyncing = true for 2s             │
│        ├── Update lastSyncServerTs, lastSyncPosition            │
│        │                                                         │
│        ▼                                                         │
│  scheduleAt(target_server_ts, () => video.play())               │
│        │                                                         │
│        ▼                                                         │
│  [Video plays at synchronized time]                             │
│        │                                                         │
│        ├── syncLoop() adjusts playbackRate for drift            │
│        │                                                         │
└─────────────────────────────────────────────────────────────────┘
```
