---
title: Third-Party Clients
parent: Development
nav_order: 6
---

# Third-Party Client Integration Guide

How a Jellyfin client that cannot run the injected web client (Fladder,
Swiftfin, Streamyfin, a custom Android TV app, ...) can take part in
JellyWatchParty by talking to the session server's WebSocket directly.

Two independent steps:

- **Step 1 (required):** be a guest. Join a room and have the native player
  follow the room's play/pause/seek/position like a browser guest. Without
  this, such a client can only be driven from outside by a bridge (see
  [Host Bridge]({{ '/technical/host-bridge/' | relative_url }})).
- **Step 2 (optional):** an in-app Watch Party widget: room list, create,
  leave, chat, and bridge controls for the user's other devices.

Reference implementation: `src/clients/jellyfin-web/` (`ws/`, `playback/`,
`state.js`). Full message reference:
[Protocol]({{ '/technical/protocol/' | relative_url }}). Timing constants:
[Sync Algorithms]({{ '/technical/sync/' | relative_url }}).

Every message is one JSON text frame
`{"type", "room"?, "payload"?, "ts"}`. `ts` (ms since epoch) is required on
everything you send; a message without it is rejected as
`Invalid message format`. Unknown types get an `error`. Each connection may
send at most 30 messages per second.

---

## Step 1: Being Controlled by the Room

### 1.1 Get a token and the server URL

```
GET /JellyWatchParty/Token
X-Emby-Token: <jellyfin access token>
```

```json
{
  "token": "eyJ...",
  "auth_enabled": true,
  "expires_in": 3600,
  "user_id": "5f0c...",
  "user_name": "John",
  "session_server_url": "wss://party.example.com/ws",
  "hide_native_sync_button": false,
  "allow_third_party_host": false,
  "allow_supported_receiver": false,
  "allow_democratic_mode": false
}
```

- With auth disabled, `token` is `null`, `auth_enabled` is `false` and there
  is no `expires_in`.
- `session_server_url` is the WebSocket URL as configured (`ws://` or
  `wss://`, path included). If it is empty, the web client uses
  `ws(s)://<Jellyfin host>:3000/ws`.
- The endpoint is limited to 30 calls per minute per user (`429` beyond).
  Fetch a new token before `expires_in` runs out and before reconnecting.

### 1.2 Connect with a persistent client id and resume secret

```
ws(s)://<session server>/ws?client_id=<uuid>&resume=<resume secret>
```

- Generate a UUIDv4 once and store it. Only 36-character hex-and-hyphen
  values are accepted; anything else gets a server-generated id.
- On connect the server sends `client_hello`
  (`payload: {client_id, resume_secret, room_id}`), then `room_list`.
  **Store `client_id` and `resume_secret` from every `client_hello`**: the
  secret changes on each successful reattach, and if the id was refused
  (in use, no or wrong secret) you get a new id.
- Reconnecting with the stored id and the latest secret within 90 s
  (`RECONNECT_GRACE_SECS`) reattaches to the existing entry: auth, room
  membership and host role are kept, and the server resends `room_state` and
  `participants`. `payload.room_id` in `client_hello` says which room (if
  any) the server has you in. Without the secret, every reconnect is a new
  client and the old one is dropped from its room after the grace period.
- Non-browser clients normally send no `Origin` header, which the server
  accepts. If yours does, it must be listed in the server's `ALLOWED_ORIGINS`.

### 1.3 Authenticate

Send right after connecting:

```json
{"type": "auth", "payload": {"token": "eyJ..."}, "ts": 1700000000000}
```

Success returns `auth_success` (`payload.user_name`); a bad token returns an
`error` (`Authentication failed`). With auth disabled, send
`{"user_name": "...", "user_id": "..."}` instead to set your display name.
`create_room` and `join_room` require an authenticated client.

Only add `"capabilities": ["democratic"]` to the payload if your client
implements democratic mode (applies other participants' play/pause/seek and
suppresses its own echoes, see 1.8). Without it, the server ignores your
play/pause/seek unless you are the host.

### 1.4 Join a room

```json
{"type": "join_room", "room": "<room id>", "payload": {"user_name": "John", "password": "..."}, "ts": 1700000000000}
```

`password` only if the room has one (`has_password` in `room_list`). Errors
come back as `error` with `payload.message`; join errors also carry a
machine-readable `payload.reason`:

| `reason` | Meaning |
|----------|---------|
| `wrong_password` | Incorrect password |
| `too_many_attempts` | 5 wrong passwords within 60 s; `retry_after_ms` says when to retry |
| `room_not_found` | Room is gone; a fresh `room_list` follows |
| `device_already_bridged` | The device is already in a room through a bridge |

A full room (20 clients) returns `error` with message `Room is full` and no
`reason`.

On success you get `room_state`:

```json
{
  "type": "room_state",
  "room": "<room id>",
  "server_ts": 1700000000123,
  "payload": {
    "name": "Room de John",
    "host_id": "<client id>",
    "participant_count": 3,
    "media_id": "<jellyfin item id or null>",
    "started": true,
    "state": {"position": 120.5, "play_state": "playing"},
    "chat_history": [],
    "democratic": false,
    "device_control": false
  }
}
```

Load `media_id` in your player and seek to the room position: if
`play_state` is `playing`, use
`position + (serverNow - server_ts) / 1000` (1.5) and play; otherwise pause
at `position`. Drift correction (1.7) takes over from there.

### 1.5 Clock synchronization

Server timestamps are in server time, and play commands are scheduled for a
future server time, so estimate the clock offset:

```json
{"type": "ping", "payload": {"client_ts": 1700000000000}, "ts": 1700000000000}
```

The `pong` echoes your payload and adds `server_ts` at the top level:

```js
const now = Date.now();
const rtt = now - msg.payload.client_ts;
const offset = msg.server_ts + rtt / 2 - now;
// serverNow() = Date.now() + smoothedOffset
```

The web client keeps the last 8 samples, takes the one with the lowest RTT
and smooths it with an EMA (alpha 0.4). It pings every 2 s until 5 pongs
arrived, then every 30 s.

Keep pinging while connected: a connection that sends nothing for 60 s is
treated as dead (`ZOMBIE_TIMEOUT_MS`), and with the 90 s grace period a
client that stays silent longer (suspended app, sleeping device) loses its
room. Test with the app backgrounded, not only in the foreground.

### 1.6 Report ready

```json
{"type": "ready", "room": "<room id>", "ts": 1700000000000}
```

Send it once the room's item is loaded and can play. Readiness resets when
you join and on every `media_changed`, so send it again each time. While
someone isn't ready, the host's play is held (`pending_play`): at most 2 s
(`MAX_READY_WAIT_MS`) normally, and 10 s (`START_READY_WAIT_MS`) for the
room's first play, which everyone is told about with `start_pending`
(`payload.timeout_ms`). A client that never sends `ready` therefore delays
every play in the room.

### 1.7 Apply playback messages

**`player_event`** (discrete play/pause/seek/buffering from the host, or in
democratic rooms from any participant or a chat bot):

```json
{
  "type": "player_event",
  "room": "<room id>",
  "server_ts": 1700000001000,
  "payload": {
    "action": "play",
    "position": 120.5,
    "play_state": "playing",
    "target_server_ts": 1700000001000,
    "by": "<client id>",
    "by_name": "John"
  }
}
```

The server sets `target_server_ts` (also in `server_ts`) to now + 1000 ms for
`play` (3000 ms with `countdown: true` for the room's first play) and
+300 ms for other actions. What the web client does:

- `play`: seek to `position + max(0, serverNow - server_ts)/1000 + 0.3`
  (`SYNC_LEAD_MS`) if more than 1 s off, then start playback at
  `target_server_ts` (a plain timer is accurate enough).
- `pause`: pause now, at the adjusted position.
- `buffering`: pause at `position`.
- `seek`: seek to `position`; if `play_state` is `playing`, keep playing.

The sender doesn't get its own event back, except a play that was held for
the ready gate (1.6): that one goes to everyone, sender included.

**Suppress your own event listeners** for about 2 s (`SUPPRESS_MS`) whenever
you change the player programmatically. Otherwise your play/pause/seek
handlers report the sync action as a user action, and if you send it (as
host, or in a democratic room) you create a feedback loop.

**`state_update`** (the host's position, about once per second; the server
filters out redundant ones: same play state and under 500 ms since the last,
or only small position jitter):

```json
{"type": "state_update", "room": "<room id>", "server_ts": 1700000005000,
 "payload": {"position": 125.3, "play_state": "playing"}}
```

Follow `play_state` (play or pause to match), then keep a reference point
`(position, server_ts)` and check every 500 ms (`SYNC_LOOP_MS`) while playing:

```
expected = position + (serverNow - server_ts) / 1000
drift    = expected - playerPosition
```

Minimum: if `|drift| >= 2 s` (`DRIFT_SOFT_MAX_SEC`), seek to `expected`. That
is a correct, if jumpy, guest.

Smoother, as in `playback/sync.js`: start correcting when `|drift|` exceeds
0.3 s (`DRIFT_CORRECTION_ENTER_SEC`), stop and return to 1x below 0.1 s
(`DRIFT_CORRECTION_EXIT_SEC`), and meanwhile set the playback rate to
`clamp(1 + sign(drift) * sqrt(|drift|) * 0.5, 0.85, 2.0)`. Hard-seek only
at 2 s or more. Skip the rate part if your player has no rate control.

### 1.8 Sending as host or in a democratic room

If your client can become host (it created the room, or was promoted when
the host left, see 1.9), it must send:

- `player_event` for user play/pause/seek, and `buffering` when the player
  stalls: `{"action": "pause", "position": 130.2, "play_state": "paused"}`
- `state_update` about once per second: `{"position": 130.2, "play_state": "playing"}`
- `set_media` when the user starts another item:
  `{"media_id": "<item id>", "position": 0}`

All with `"room": "<room id>"`; the server adds `target_server_ts`. You are
host when `host_id` (from `room_state` or `host_changed`) equals your
`client_id` from `client_hello`. Messages you are not allowed to send are
dropped silently.

In a democratic room (`democratic: true` in `room_state`, or
`democratic_changed` with `enabled: true`) that has started, any member that
announced the `democratic` capability may also send `play`, `pause` and
`seek` `player_event`s. `buffering`, `state_update` and `set_media` stay with
the host.

A guest-only client that can't do this should leave the room, or at least
tell the user, when it becomes host: nobody else will send the room's
position.

### 1.9 Room lifecycle

- `media_changed` (`payload: {media_id, position}`): load the new item, seek
  to `position`, send `ready` again. Ignore playback messages until it is
  loaded.
- `room_closed` (`payload.reason`, plus `removed: true` when an admin removed
  you): stop syncing and return to "no room".
- `host_changed` (`payload: {host_id, host_name, participant_count}`): the
  host left and the earliest remaining participant took over. If that is
  you, see 1.8.
- `client_left`, `participants_update` (`participant_count`) and
  `participants` (`payload.participants`: `[{id, name, is_host, status}]`):
  only needed for UI.
- `client_status` (optional): report your own state so others see it in the
  participant list: `{"status": "synced"}`, one of `synced`, `syncing`,
  `buffering`, `loading`, `idle`, `playing`, `paused`.

### 1.10 Checklist

| # | Requirement | Why |
|---|-------------|-----|
| 1 | `GET /JellyWatchParty/Token` | Token, `auth_enabled`, `session_server_url` |
| 2 | Persistent `client_id` + latest `resume_secret` on every connect | Reconnects keep room membership |
| 3 | `auth` | `create_room` / `join_room` require it when auth is enabled |
| 4 | `join_room` with password and `reason` handling | Becoming a guest |
| 5 | `ping` / `pong` clock offset, continuous pinging | `target_server_ts`, drift maths, staying alive |
| 6 | `ready` after join and after each `media_changed` | Otherwise every play in the room is delayed |
| 7 | Apply `player_event` | Being controlled |
| 8 | Drift correction from `state_update` (hard seek at minimum) | Long sessions stay aligned |
| 9 | Suppress own listeners during sync actions | No feedback loops |
| 10 | Handle `media_changed`, `room_closed`, `host_changed` | Correct behaviour as the room changes |

---

## Step 2: In-App Watch Party Widget (Optional)

For a native button or panel that lets users manage rooms from inside your
client, like the web client's panel (`ui/render.js`). Not needed for Step 1.

Plugin REST calls use the user's Jellyfin token (`X-Emby-Token`) against
`http(s)://<jellyfin>/JellyWatchParty`.

### 2.1 Room list

`room_list` arrives after connecting and again whenever rooms change; send
`{"type": "list_rooms", "ts": ...}` to request it. Its payload is an array of
`{id, name, count, media_id, has_password, democratic}`. Prompt for a
password when `has_password` is true.

### 2.2 Create a room

```json
{"type": "create_room", "payload": {"media_id": "<item id>", "start_pos": 0.0, "user_name": "John", "started": false, "password": "optional"}, "ts": 1700000000000}
```

- The server names the room after the host's display name; there is no name
  field.
- `started: false` gives the room's first play the ready wait and countdown;
  omit it (defaults to `true`) if playback is already running.
- Creating a room closes any other room you host (a chat-bot room is left
  instead).
- The `room_state` reply makes you host: implement 1.8 before offering this
  button.

### 2.3 Leave

```json
{"type": "leave_room", "ts": 1700000000000}
```

### 2.4 Chat

```json
{"type": "chat_message", "room": "<room id>", "payload": {"text": "hi"}, "ts": 1700000000000}
```

Members receive `chat_message` (`payload: {username, text}`, sender id in
`client`), the sender included. `room_state.chat_history` holds the last 50
messages as `{client_id, username, text, server_ts}`; history lives in
memory and ends with the room. Messages are limited to 500 characters.

### 2.5 Bridge controls

Let the user put another of their own playing Jellyfin sessions (e.g. a TV)
into a room, like the "host from another device" section of `ui/bridge.js`.
Show these controls only when `/Token` returned `allow_third_party_host`
(Start) or `allow_supported_receiver` (Follow); administrators normally use
the session server's admin panel instead.

```
GET  /JellyWatchParty/Bridge/Sessions
GET  /JellyWatchParty/Bridge/Status
POST /JellyWatchParty/Bridge/{sessionId}/Start
POST /JellyWatchParty/Bridge/{sessionId}/Follow?roomId=<room id>
POST /JellyWatchParty/Bridge/{sessionId}/Stop
```

```json
// Bridge/Sessions (empty when both options are off)
[{"sessionId": "...", "userName": "...", "deviceName": "...", "client": "...", "nowPlayingItemName": "..."}]
// Bridge/Status, and the response of Start / Follow
[{"sessionId": "...", "userName": "...", "roomId": "...", "connected": true, "role": "host"}]
```

- All endpoints require an authenticated Jellyfin user and act on the
  caller's own sessions (administrators: all). Sessions of Jellyfin Web,
  Jellyfin Desktop and Jellyfin Media Player are not listed; those run the
  web client themselves.
- `Start` makes the session host of a new room; `Follow` attaches it to an
  existing room as a receiver (it must already play the room's item). The
  returned `roomId` may still be `null` right after `Start`; poll
  `Bridge/Status` until it is set.
- `400` (`error`): the option is off, the session is gone, or the session is
  already bridged. `403`: not your session. `Stop` works even with the
  options off.
- JSON keys are exactly as shown (camelCase here, snake_case for `/Token`);
  Jellyfin does not convert them.

### 2.6 Password prompts

Use your platform's own dialog for room passwords. Embedded browser engines
can turn `window.prompt()` into a silent no-op (it does in the CEF-based
Jellyfin Desktop), so don't rely on a browser-style prompt.

### 2.7 Checklist

| # | Feature | Message / endpoint |
|---|---------|--------------------|
| 1 | Browse rooms | `list_rooms` / `room_list` |
| 2 | Create a room | `create_room` (needs 1.8) |
| 3 | Leave | `leave_room` |
| 4 | Chat | `chat_message`, `room_state.chat_history` |
| 5 | Bridgeable sessions | `GET /JellyWatchParty/Bridge/Sessions` |
| 6 | Active bridges | `GET /JellyWatchParty/Bridge/Status` |
| 7 | Start / follow / stop a bridge | `POST /JellyWatchParty/Bridge/{sessionId}/Start`, `/Follow`, `/Stop` |

---

## Native Player Pitfalls

- There is no `<video>` element: wrap your player's API (position, play,
  pause, seek, rate, state events) in a small adapter. If the player only
  exposes polled state, derive play/pause/seek events by comparing polls.
- Treat any player change you caused (sync, drift correction, track switches
  that reload the stream) as suppressed (1.7), or it comes back as a command.
- Audio and subtitle track switches often reload the stream and look like a
  seek or stall to listeners; the web client suppresses them for up to 8 s
  (`TRACK_SWITCH_SUPPRESS_MS`).

## Further Reading

- [Protocol]({{ '/technical/protocol/' | relative_url }}): every message and field
- [Sync Algorithms]({{ '/technical/sync/' | relative_url }}): clock sync, scheduling, drift correction, ready gate
- [Host Bridge]({{ '/technical/host-bridge/' | relative_url }}): the bridges behind 2.5
- [Client]({{ '/technical/client/' | relative_url }}): the reference implementation
