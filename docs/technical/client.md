---
title: Client
parent: Technical
nav_order: 4
---

# JavaScript Client

## Overview

The OpenWatchParty client is a set of JavaScript modules (IIFE pattern) injected into Jellyfin's web interface. These modules handle playback synchronization between multiple users via WebSocket.

## Module Architecture

```
plugin.js                    # Loader - loads modules in parallel waves
    ├── state.js             # Global state and constants
    ├── utils/               # Utility functions
    │   ├── log.js, media.js, misc.js, time.js, video.js
    ├── ui/                  # User interface
    │   ├── cards.js, home.js, indicators.js
    │   ├── render.js, styles.js, toasts.js
    ├── playback/            # Video playback management
    │   ├── bind.js, play.js, sync.js
    ├── chat/                # Text chat
    │   ├── input.js, messages.js
    ├── ws/                  # WebSocket communication
    │   ├── send.js, auth.js, connection.js
    │   └── handlers/
    │       ├── clock.js, playback.js, room.js, sync.js
    └── app/                 # Initialization and cleanup
        ├── cleanup.js, lifecycle.js
```

Modules are loaded in dependency waves, parallelizing where possible:
`state.js` → `utils/*` → `ui/*` → `playback/*` → `chat/*` → `ws/send` → `ws/auth` → `ws/handlers/*` → `ws/connection` → `app/*`

## Module: `state.js`

### Description
Defines global shared state and configuration constants. Guarded by
`if (OWP.state) return;` so re-injection (e.g. after a hot reload) is a
no-op.

### Constants (`OWP.constants`)

| Constant | Value | Description |
|----------|-------|-------------|
| `SUPPRESS_MS` | `2000` | Event suppression / anti-feedback lock duration (ms) |
| `SEEK_THRESHOLD` | `1.0` | Position difference to trigger seek (s) |
| `STATE_UPDATE_MS` | `1000` | State update send interval (ms) |
| `SYNC_LEAD_MS` | `300` | Latency compensation advance (ms) |
| `DRIFT_CORRECTION_ENTER_SEC` | `0.3` | Drift needed to start a correction burst (s) |
| `DRIFT_CORRECTION_EXIT_SEC` | `0.1` | Drift must fall under this to stop correcting (s) |
| `DRIFT_SOFT_MAX_SEC` | `2.0` | Forced-seek threshold (s) |
| `PLAYBACK_RATE_MIN` / `MAX` | `0.85` / `2.0` | Catchup playback-rate clamp |
| `DRIFT_GAIN` | `0.50` | Sqrt-curve proportional gain |
| `UI_CHECK_MS` | `2000` | UI/video-binding check interval (ms) |
| `PING_INIT_MS` | `2000` | Ping interval before clock sync stabilizes (ms) |
| `PING_STABLE_MS` | `30000` | Ping interval after stabilizing (ms) |
| `PING_STABLE_AFTER` | `5` | Successful pongs before switching to the stable interval |
| `HOME_REFRESH_MS` | `5000` | Home watch-parties refresh interval (ms) |
| `SYNC_LOOP_MS` | `500` | Drift-correction loop interval (ms) |
| `RECONNECT_BASE_MS` / `MAX_MS` | `1000` / `30000` | Exponential-backoff reconnect delay bounds |
| `INITIAL_SYNC_COOLDOWN_MS` | `8000` | Cooldown after join before allowing a hard seek |
| `INITIAL_SYNC_MAX_MS` | `30000` | Max duration of the initial-sync phase |
| `INITIAL_SYNC_DRIFT_THRESHOLD` | `0.5` | Drift under which initial sync ends early (s) |
| `INITIAL_SYNC_MAX_DRIFT` | `10` | Drift that forces a hard seek during initial sync (s) |
| `TIME_SYNC_MAX_SAMPLES` | `8` | Clock-sync sliding-window sample count |
| `TIME_SYNC_EMA_ALPHA` | `0.4` | Clock-sync EMA smoothing coefficient |

See also `PANEL_ID`, `BTN_ID`, `STYLE_ID`, `HOME_SECTION_ID` (DOM element
IDs) and `DEFAULT_WS_URL` (`ws(s)://<host>:3000/ws`, scheme matching the
page's protocol).

### State (`OWP.state`)

Notable fields beyond the obvious (`ws`, `roomId`, `clientId`, `isHost`,
`inRoom`, `rooms`): `serverOffsetMs`/`hasTimeSync`/`timeSyncSamples`
(clock sync), `lastSyncServerTs`/`lastSyncPosition`/`lastSyncPlayState`
(drift-correction baseline), `isDriftCorrecting` (hysteresis latch —
true while a correction burst is active, see `playback/sync.js`),
`isInitialSync`/`initialSyncUntil`/`initialSyncTargetPos` (the
post-join catch-up phase, which suppresses hard seeks for
`INITIAL_SYNC_COOLDOWN_MS`), `syncStatus` (`'unknown'`/`'synced'`/
`'syncing'`/`'pending_play'`, drives the UI indicator),
`reconnectAttempts` (exponential backoff counter),
`intervals: { ui, ping, home, sync, stateUpdate }` (handles for
`clearInterval`, tracked to avoid leaks), `authToken`/`authEnabled`/
`tokenExpiresAt`/`tokenRefreshTimer` (JWT lifecycle), and
`homeRoomCache` (an `LRUCache` of 50 entries for home-page cover images).

## Module: `utils/` — Shared Helpers

### `utils/time.js`
`nowMs()`, `getServerNow()` (`nowMs() + serverOffsetMs`),
`adjustedPosition(position, serverTs)` (adds elapsed time since
`serverTs` plus `SYNC_LEAD_MS`), `scheduleAt(serverTs, fn)` (runs `fn`
immediately if the target time has passed, otherwise `setTimeout`s the
remainder — also clears any prior pending action timer), and
`startSyncing()` (sets the `isSyncing` anti-feedback lock for
`SUPPRESS_MS`).

### `utils/misc.js`
`shouldSend()` (outside the suppression window), `suppress(ms?)`,
`escapeHtml(str)` (chat/room-name XSS escaping), `getItemImageUrl(itemId,
imageTag)`, `isHomeView()`.

### `utils/media.js`
`getCurrentItem()`/`getCurrentItemId()` — locates the currently playing
item by checking, in order: the Jellyfin playback manager, `window`
globals (`NowPlayingItem`, `Emby.Page.currentRouteInfo`,
`sessionStorage.playbackInfo`), a DOM data-attribute lookup, and finally
the URL hash. This layered fallback exists because none of those sources
is reliably present across all Jellyfin skins/versions.

### `utils/log.js`
`log(category, data)` — formats structured log lines (`[OWP:CATEGORY]
key=val ...`, with unit-aware formatting for position/rate/offset
fields), logs to the console, and also relays them to the server as a
`client_log` message when connected (buffering up to `logBufferMax`
entries — default 100 — while disconnected, flushed via
`flushLogBuffer()` once the socket reopens).

### `utils/video.js` {#module-utilsvideojs}
`getVideo()`, `isVideoReady()`, `isBuffering()`, `isSeeking()`,
`getPlaybackManager()`.

Normally `getVideo()` just returns `document.querySelector('video')`.
But Jellyfin Desktop's CEF+mpv player (and Jellyfin Media Player before
its rename) never creates a DOM `<video>` element at all — it exposes
`window._mpvVideoPlayerInstance` instead. When no real `<video>` is
found but that instance exists, `getVideo()` wraps it in a **native
adapter**: an `HTMLMediaElement`-shaped object (`currentTime`, `paused`,
`playbackRate`, `readyState`, `seeking`, `play()`/`pause()`,
`add`/`removeEventListener`) backed by polling the mpv instance every
`NATIVE_POLL_MS` (250ms) instead of real DOM events, so
`playback/bind.js` and `playback/sync.js` work unmodified against it.

Known limitation: the mpv player only exposes coarse
`playing`/`pause`/`unpause`/`stopped`/`volumechange`/`error` signals —
there's no equivalent of HTML5's `waiting` (buffering) event, so
buffering can't be detected on this adapter, and its `readyState`/
`networkState` are approximations rather than real buffer-health
signals.

## Module: `ui/` — Interface

### `ui/render.js`
`render(forceFullRender?)` — the main panel render, skipping a full
re-render if already showing the right view (lobby vs. in-room) and
just refreshing indicators/lists instead. `renderLobby(panel)` builds
the room list, "Create Room" button, and the Host Bridge section (see
[Host Bridge](host-bridge.md)). `renderRoom(panel)` builds the in-room
view (name, participants, sync indicator, chat, RTT, leave/close
button). Also `injectOsdButton()` (button in the video OSD controls)
and `injectGlobalButton()` (a persistent header button, since Jellyfin's
SPA frequently replaces/removes the OSD — added so the launcher is
still reachable even with no video open).

### `ui/cards.js` + `ui/home.js`
Render the "Watch Parties" section on the Jellyfin home page.
`ui/home.js`'s `renderHomeWatchParties()` reconciles the DOM against
`state.rooms` (adding/removing cards, not a full re-render) and
`ui/cards.js`'s `createRoomCard()` builds each card, fetching cover
image/title from the Jellyfin API asynchronously. Clicking a card's
play button navigates to the item's detail page and polls for its
"Play" button to auto-click once the page has loaded.

### `ui/indicators.js`
`updateStatusIndicator()` (connection Online/Offline dot),
`buildSyncStatusIndicator()`/`updateSyncIndicator()` (renders
`state.syncStatus` as a status dot + label — `'synced'`, `'syncing'`,
`'pending_play'` with a countdown, or `'unknown'`/"Not synced yet" —
hidden entirely for the host, who has nothing to sync against).

### `ui/toasts.js`
`showToast(message)` (centered, auto-dismissing system notifications)
and `showChatToast(username, text)` (top-right, stacked chat
notifications — capped at 5 visible, oldest dismissed first).

### `ui/styles.js`
Injects the plugin's CSS into `<head>` as a single `<style>` tag keyed
by `STYLE_ID`.

### `ui/bridge.js`
Renders the Host Bridge picker inside the lobby panel — see
[Host Bridge](host-bridge.md) for the full feature writeup.

## Module: `playback/` — Video Binding and Sync

### `playback/bind.js`
`bindVideo()` attaches listeners to the active video element
(`waiting`/`canplay`/`playing`/`play`/`pause`/`seeked`) and starts a
`STATE_UPDATE_MS` interval that calls `sendStateUpdate()` while hosting.
`cleanupVideoListeners()` tears all of that down (called on room leave
or when the video player closes).

Host-side send gating (`onHostEvent`, one function handling
play/pause/seek uniformly): ignored entirely if not host, during the
`isSyncing` anti-feedback window, or (for pause) while buffering/seeking
(so HLS artifacts don't get broadcast as real user actions); seeks are
additionally debounced (250ms) and required to differ from the last
sent position by at least `SEEK_THRESHOLD`. A `waiting` event also
proactively broadcasts a `buffering` player_event so guests pause too
instead of drifting ahead during the host's stall.

### `playback/play.js`
`playItem(item)` starts playback via the Jellyfin `PlaybackManager`,
trying several call signatures in turn (`play({items})`, `play({item})`,
`play({ids})`, `playItems()`) since the exact API has varied across
Jellyfin versions. `ensurePlayback(itemId, attempt?)` is how a guest
loads the host's media on join — it no-ops if that item is already
playing, otherwise fetches it and calls `playItem`, retrying up to 5
times at 500ms intervals on failure.

### `playback/sync.js`
`watchReady()`/`notifyReady()` send the `ready` message once the video
can play (`readyState >= 2`), which unblocks the server's pending-play
handshake for late joiners (see [Sync Algorithms](sync.md)).

`syncLoop()` (called every `SYNC_LOOP_MS` for non-hosts) is the
hysteresis drift controller documented in full in
[Sync Algorithms](sync.md): resets to 1x whenever not applicable (host,
not in room, not playing, buffering, paused), otherwise computes
`drift = expected - video.currentTime` and only starts a correction
burst once `|drift|` exceeds `DRIFT_CORRECTION_ENTER_SEC`, holding it
until `|drift|` falls back under `DRIFT_CORRECTION_EXIT_SEC`. There's
also a distinct **initial-sync phase** right after joining
(`checkInitialSync`) that suppresses hard seeks for
`INITIAL_SYNC_COOLDOWN_MS`/`INITIAL_SYNC_MAX_MS` unless drift is
extreme (`INITIAL_SYNC_MAX_DRIFT`), since Jellyfin's own resume-position
jump right after load would otherwise look like a real desync.

## Module: `chat/` — Text Chat

### `chat/input.js`
`send(text)` — validates length (`MAX_MESSAGE_LENGTH` 500) and
connection/room state, then sends a `chat_message`.
`isChatVisible()`/`markRead()`/`updateBadge()` manage the unread badge
shown when the chat panel is closed.

### `chat/messages.js`
`receive(msg)` appends an incoming *live* message to `chat.messages`
(capped at `MAX_MESSAGES` 100 client-side), increments the unread count
and shows a toast if the panel is hidden, and renders it.
`hydrate(entries)` instead *replaces* `chat.messages` wholesale from the
server-replayed `chat_history` on `room_state` (see `protocol.md`) —
unlike `receive()`, it never touches the unread badge or fires a toast,
since it's backfill for a joining/reattaching client, not a new live
message. `renderAllMessages()`/`clear()` handle full re-render and
room-leave cleanup.

## Module: `ws/` — WebSocket Communication

### `ws/send.js`
`send(type, payload?, roomOverride?)` — the low-level message sender
(adds `ts` and `client` automatically). `createRoom(password?)`,
`joinRoom(id, password?)`, and `leaveRoom()` build on top of it;
`leaveRoom()` also resets all sync/drift state fields and hides the panel.

### `ws/auth.js`
`fetchAuthToken()` — calls `/OpenWatchParty/Token` with the user's
Jellyfin access token, waiting up to 10s for `window.ApiClient` to exist
if needed. Populates `state.userName`/`userId`/`authEnabled` regardless
of whether auth is enabled, and if enabled, stores the JWT and schedules
a refresh (`scheduleTokenRefresh`) at 80% of the token's lifetime
(capped at 5 minutes before expiry) that re-authenticates over the
existing socket rather than reconnecting.

### `ws/connection.js`
`connect()` opens the WebSocket, appending the client's **persistent**
`client_id` (a UUID generated once and stored in `localStorage`, see
[Architecture: Persistent Client ID](architecture.md)) as a
`?client_id=` query param — this is what lets the server reattach a
reconnecting client to its existing room/host state instead of treating
it as new. `onWsClose` reconnects with exponential backoff
(`RECONNECT_BASE_MS` doubling up to `RECONNECT_MAX_MS`), not a fixed
delay. `handleMessage` dispatches each incoming message type to the
matching function in `OWP._wsHandlers` (populated by `ws/handlers/*.js`).

### `ws/handlers/room.js`
`handleRoomList`, `handleClientHello` (stores the server-assigned
`client_id` used for the `client` message field — distinct from the
persistent reconnect `client_id` above), `handleParticipantsUpdate`/
`handleClientLeft` (toast + counter update), `handleRoomClosed`,
`handleError`.

### `ws/handlers/playback.js`
`handlePlayerEvent` — applies a host's play/pause/seek/buffering
command: activates `startSyncing()`, hard-corrects position if the gap
exceeds `SEEK_THRESHOLD`, then dispatches by action (`play` schedules
via `scheduleAt(target_server_ts, ...)` or applies immediately with lead
compensation; `pause`/`seek`/`buffering` apply directly and reset
sync-tracking state).

### `ws/handlers/sync.js`
`handleRoomState` — applies full room state on join/reattach (host
detection, initial clock offset if not yet synced, initial seek +
play/pause, triggers `ensurePlayback` for guests) and
`handleStateUpdate` — the periodic host position/play-state relay,
gated by a post-command cooldown so it doesn't fight with a just-applied
`player_event`.

### `ws/handlers/clock.js`
`handlePong` — the clock-sync sample handler: computes RTT and a
candidate offset, keeps the best (lowest-RTT) of the last
`TIME_SYNC_MAX_SAMPLES`, and folds it into `serverOffsetMs` via EMA
(`TIME_SYNC_EMA_ALPHA`) once initial sync is established. Also drives
adaptive ping frequency: pings start fast (`PING_INIT_MS`) and switch to
the slow interval (`PING_STABLE_MS`) after `PING_STABLE_AFTER`
consecutive stable pongs, reverting to fast pinging if the offset jumps
by more than 50ms.

## Module: `app/` — Initialization and Cleanup

### `app/lifecycle.js`
`init()` — injects styles, creates the (hidden) panel, connects the
WebSocket, and starts the tracked intervals (`ui`, `home`, `sync` — see
`state.intervals`). The `ui` interval (every `UI_CHECK_MS`) is also
where video-player-exit is detected: if a `<video>` was present on the
last check and now isn't, `onVideoPlayerExit()` hides the panel, leaves
the room if in one, and cleans up video listeners. It also
unconditionally injects the persistent header button
(`ui.injectGlobalButton()`) since Jellyfin's SPA frequently swaps out
the header DOM during navigation.

### `app/cleanup.js`
`cleanup()` — full teardown for plugin unload: clears all tracked
intervals and the pending-action timer, closes the WebSocket, removes
panel and video event listeners, resets `bound`/`initialized`.

## Synchronization Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                          HOST                                    │
├─────────────────────────────────────────────────────────────────┤
│  [User clicks Play]                                              │
│        │                                                         │
│        ▼                                                         │
│  onEvent('play')                                                 │
│        │                                                         │
│        ├── Checks: isHost? shouldSend? !isSyncing? isVideoReady?│
│        │                                                         │
│        ▼                                                         │
│  send('player_event', {action:'play', position})                │
│        │                                                         │
└────────┼────────────────────────────────────────────────────────┘
         │
         ▼ WebSocket
┌─────────────────────────────────────────────────────────────────┐
│                        SERVER                                    │
├─────────────────────────────────────────────────────────────────┤
│  Receives player_event                                          │
│        │                                                         │
│        ├── Validates: is host?                                  │
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
