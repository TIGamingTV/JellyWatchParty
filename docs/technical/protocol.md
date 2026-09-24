---
title: Protocol
parent: Technical Reference
nav_order: 1
---

# WebSocket Protocol Specification

## Overview

JellyWatchParty uses a JSON-over-WebSocket protocol for real-time communication between clients and the session server.

**Endpoint:** `ws(s)://<host>:3000/ws?client_id=<persistent-client-id>`

### `client_id` Query Parameter

The client generates a UUID once, persists it in `localStorage`, and
sends it as `?client_id=` on every connection attempt (including
reconnects). This is a *different* identifier from the per-connection
`client` field used elsewhere in this protocol and from the
`client_hello.payload.client_id` below — this query param is what lets
the server recognize "this is the same client as before" across a
dropped connection, so it can reattach the client to its existing room
membership (and resend `room_state`) instead of treating it as brand
new. Only values that look like a real UUIDv4 are trusted; anything
else is ignored and the server mints a fresh ID instead. See
[Server: Persistent Client ID]({{ '/technical/server/' | relative_url }}#persistent-client-id) for the
reattachment mechanics.

## Message Format

All messages follow this structure:

```json
{
  "type": "message_type",
  "room": "room_id",
  "client": "client_id",
  "payload": { ... },
  "ts": 1678900000000,
  "server_ts": 1678900000100
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Yes | Message type |
| `room` | string | No | Room ID (if applicable) |
| `client` | string | No | Sender client ID |
| `payload` | object | No | Message-specific data |
| `ts` | number | Yes | Client timestamp (ms since epoch) |
| `server_ts` | number | No | Server timestamp (added by server) |

## Client → Server Messages

### `auth`

Authenticate with a JWT token (if authentication is enabled).

```json
{
  "type": "auth",
  "payload": {
    "token": "eyJhbGciOiJIUzI1NiIs..."
  },
  "ts": 1678900000000
}
```

### `list_rooms`

Request the list of active rooms.

```json
{
  "type": "list_rooms",
  "ts": 1678900000000
}
```

**Response:** `room_list`

### `create_room`

Create a new watch party room.

```json
{
  "type": "create_room",
  "payload": {
    "name": "Movie Night",
    "start_pos": 0.0,
    "media_id": "abc123def456",
    "password": "optional-room-password"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `name` | string | Room display name |
| `start_pos` | number | Initial position (seconds) |
| `media_id` | string | Jellyfin media ID (optional) |
| `password` | string | Optional room password. If set, `join_room` must supply a matching `password` (see below). Never echoed back to any client. |
| `started` | boolean | Optional. `true` when the host is already playing: the room counts as started, so it gets no start countdown. |

**Response:** `room_state`

**Effects:**
- Client becomes host
- Broadcast `room_list` to all clients

### `join_room`

Join an existing room.

```json
{
  "type": "join_room",
  "room": "uuid-room-id",
  "payload": {
    "password": "required-if-room-has-one"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `password` | string | Required only if the room was created with a password. Not checked for a client that's already a member of the room (e.g. a re-sent join after a panel refresh). |

**Response:** `room_state`, or `error` with `payload.reason: "wrong_password"` if the password is missing/incorrect.

After 5 wrong passwords within 60 s, the same user (keyed by `user_id`, i.e. the JWT `sub`, not the client id) is refused for the rest of that 60 s window with `payload.reason: "too_many_attempts"` and `payload.retry_after_ms`, without the password being checked. The throttle is per room and per user, so one user guessing can't lock others out. A successful join clears the user's count.

**Effects:**
- Client added to `room.clients`
- Client removed from `room.ready_clients`
- Broadcast `participants_update` to other participants

### `leave_room`

Leave the current room.

```json
{
  "type": "leave_room",
  "room": "uuid-room-id",
  "ts": 1678900000000
}
```

**Effects:**
- If host leaves and other participants remain: the earliest-joined
  remaining participant is promoted to host in place, broadcast
  `host_changed` (room stays open)
- If host leaves and no participants remain: room closes, broadcast
  `room_closed`
- If a non-host leaves: broadcast `participants_update`
- Broadcast `room_list` to all

### `ready`

Indicate client is ready to receive playback commands.

```json
{
  "type": "ready",
  "room": "uuid-room-id",
  "payload": {
    "media_id": "abc123def456"
  },
  "ts": 1678900000000
}
```

**Effects:**
- Client added to `room.ready_clients`
- If `pending_play` exists and `all_ready()`: triggers scheduled play

### `player_event`

Send a playback event (host only).

```json
{
  "type": "player_event",
  "room": "uuid-room-id",
  "payload": {
    "action": "play",
    "position": 120.5
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `action` | string | `"play"`, `"pause"`, `"seek"`, or `"buffering"` |
| `position` | number | Current position (seconds) |

**Behavior by action:**

| Action | Server Behavior |
|--------|-----------------|
| `play` | If `all_ready()`: broadcast with `target_server_ts = now + 1000ms`. Otherwise: create `pending_play` |
| `pause` | Broadcast with `target_server_ts = now + 300ms` |
| `seek` | Broadcast with `target_server_ts = now + 300ms` |
| `buffering` | Broadcast with `target_server_ts = now + 300ms` (treat as paused) |

**Effects:**
- Updates `room.state`
- Updates `room.last_command_ts` (cooldown)
- Broadcasts to other participants

### `state_update`

Periodic playback state update (host only).

```json
{
  "type": "state_update",
  "room": "uuid-room-id",
  "payload": {
    "position": 125.3,
    "play_state": "playing"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `position` | number | Current position (seconds) |
| `play_state` | string | `"playing"` or `"paused"` |

**Server filtering:**
1. Ignored if `now - last_command_ts < 2000ms` (cooldown)
2. Ignored if `now - last_state_ts < 500ms` (rate limit)
3. Ignored if position moves back 0.5s-2s (HLS jitter)
4. Ignored if position advances < 0.5s (insignificant)
5. Always accepted if `play_state` changes

### `ping`

Latency measurement and clock synchronization.

```json
{
  "type": "ping",
  "payload": {
    "client_ts": 1678900000000
  },
  "ts": 1678900000000
}
```

**Response:** `pong`

### `chat_message`

Send a text message to the room.

```json
{
  "type": "chat_message",
  "room": "uuid-room-id",
  "payload": {
    "text": "Hello everyone!"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `text` | string | Message text (max 500 characters) |

**Effects:**
- Message broadcast to all clients in the room (including sender)
- Rate limited by existing 30 msg/sec limit

**Error responses:**
- `"Chat message cannot be empty"` - Empty or whitespace-only text
- `"Chat message too long (max 500 characters)"` - Text exceeds limit
- `"Room ID required for chat"` - Missing room ID

### `set_media`

Change which item the room is watching (host only). Used both when the
host starts playing something after creating a room with no media, and
when the host switches to a different item mid-session (issue #71) —
otherwise `media_id` never changes after `create_room`.

```json
{
  "type": "set_media",
  "room": "uuid-room-id",
  "payload": {
    "media_id": "abc123def456",
    "position": 0
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `media_id` | string | Required. 32-char hex Jellyfin item id |
| `position` | number | Starting position in seconds (default `0`) |

**Effects:**
- No-op (nothing changed, nothing broadcast) if `media_id` already
  matches `room.media_id`, or the sender isn't `room.host_id`
- Sets `room.media_id`, resets `room.state` to `{position, play_state:
  "paused"}`, clears `pending_play`, and resets `ready_clients` to just
  the host — so the host's next `play` waits (up to 2s, same as any
  pending play) for guests to load the new item before scheduling
  playback for everyone
- Broadcasts `media_changed` to every other room member
- Broadcasts `room_list` to all (so lobby cards refresh their poster)

**Error responses:**
- `"media_id is required"` - Missing `media_id` in payload
- `"Invalid media_id"` - Not a 32-char hex string

### `client_status`

A participant reports its own playback status, for the room's participant list. Clients send it when the status changes, at most once per second.

```json
{
  "type": "client_status",
  "room": "uuid-room-id",
  "payload": { "status": "syncing" },
  "ts": 1678900000000
}
```

| `status` | Meaning |
|----------|---------|
| `synced` | Guest is in sync with the host |
| `syncing` | Guest is catching up (drift correction) |
| `buffering` | Video is buffering |
| `loading` | Waiting for a scheduled play, or opening the room's media |
| `idle` | Not in the player |
| `playing` / `paused` | The host's own state |

**Effects:** ignored unless the value is one of the above, the sender is in the room and the status changed; otherwise everyone in the room gets `participants`.

## Server → Client Messages

### `client_hello`

Sent immediately after WebSocket connection.

```json
{
  "type": "client_hello",
  "client": "uuid-client-id",
  "payload": {
    "client_id": "uuid-client-id"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `room_list`

List of active rooms.

```json
{
  "type": "room_list",
  "payload": [
    {
      "id": "uuid-room-id",
      "name": "Movie Night",
      "count": 3,
      "media_id": "abc123def456",
      "has_password": false
    }
  ],
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `room_state`

Full room state. Sent after `create_room` or `join_room`.

```json
{
  "type": "room_state",
  "room": "uuid-room-id",
  "client": "uuid-client-id",
  "payload": {
    "name": "Movie Night",
    "host_id": "uuid-host-id",
    "participant_count": 3,
    "media_id": "abc123def456",
    "state": {
      "position": 120.5,
      "play_state": "playing"
    },
    "chat_history": [
      {
        "client_id": "uuid-sender-id",
        "username": "Alice",
        "text": "Hello!",
        "server_ts": 1678899990000
      }
    ]
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `started` | boolean | `false` until the room's first play has gone out (see [Start countdown](#start-countdown)). Clients treat a missing value as `true`. |
| `chat_history` | array | Up to the last 50 chat messages sent in this room, oldest first — empty for a freshly created room. Replayed on both initial join and reconnect-reattach so late joiners and reconnecting clients aren't missing context. |

Sent after `create_room`, `join_room`, and on reattachment after a
dropped-connection reconnect (see
[Server: Reconnect and Room Lifecycle]({{ '/technical/server/' | relative_url }}#reconnect-and-room-lifecycle)).

### `start_pending`

The host pressed play for the first time, but not everyone is ready yet. Sent to the whole room, host included.

```json
{
  "type": "start_pending",
  "room": "uuid-room-id",
  "payload": { "timeout_ms": 10000 },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

Clients show "Waiting for everyone to be ready...". The server starts when everyone has sent `ready`, or after `timeout_ms` at the latest.

### Start countdown

A room's first play (`started` is `false`) works differently from later ones:

1. The host's client keeps its video paused and sends `player_event` `play`. It sends no `state_update` while waiting. If the host's new item is still being confirmed (before `set_media`), the play is held and sent right after `set_media`, so the server already knows which item everyone has to load.
2. If everyone is ready, the server starts right away; otherwise it sends `start_pending` and waits for `ready` from everyone, up to 10 s.
3. The server sends `player_event` `play` to **everyone, host included**, with `"countdown": true` and `target_server_ts` 3 s ahead, and marks the room started. All clients show 3, 2, 1 against server time and start at `target_server_ts`.

With nobody else in the room there is no countdown (`"countdown": false`, the usual 1 s schedule). A room also counts as started if it was created with `started: true`, or once the host sends a `state_update` with `play_state: "playing"`. If the host's client gets no answer within 15 s, it just plays. Later plays behave as before.

### `participants_update`

Participant count update.

```json
{
  "type": "participants_update",
  "room": "uuid-room-id",
  "payload": {
    "participant_count": 4
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `participants`

The room's participant list, in join order. Sent to the whole room when someone joins or leaves, the host changes, or a status changes; also sent to the host on create and to a client that reattaches.

```json
{
  "type": "participants",
  "room": "uuid-room-id",
  "payload": {
    "participants": [
      { "id": "uuid-a", "name": "Alice", "is_host": true, "status": "playing" },
      { "id": "uuid-b", "name": "Bob", "is_host": false, "status": "syncing" }
    ]
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

`status` is the participant's last `client_status`, or `unknown` before its first report. `participants_update` and `client_left` are still sent with the count, for older clients.

### `player_event`

Playback command relayed from host.

```json
{
  "type": "player_event",
  "room": "uuid-room-id",
  "payload": {
    "action": "play",
    "position": 120.5,
    "target_server_ts": 1678900001000
  },
  "ts": 1678900000000,
  "server_ts": 1678900001000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `action` | string | `"play"`, `"pause"`, `"seek"`, or `"buffering"` |
| `position` | number | Reference position (seconds) |
| `target_server_ts` | number | Target server timestamp for execution |

**Client processing:**
1. Enable `isSyncing` lock (2s)
2. Calculate adjusted position with elapsed time
3. Schedule action at `target_server_ts`

### `state_update`

Periodic state update relayed from host.

```json
{
  "type": "state_update",
  "room": "uuid-room-id",
  "payload": {
    "position": 125.3,
    "play_state": "playing"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `room_closed`

Room was closed (host disconnected or room empty).

```json
{
  "type": "room_closed",
  "ts": 1678900000000
}
```

### `client_left`

A participant left the room.

```json
{
  "type": "client_left",
  "room": "uuid-room-id",
  "client": "uuid-left-client-id",
  "payload": {
    "participant_count": 2
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `participant_count` | number | Updated participant count after the client left |

### `host_changed`

The host left (or disconnected past the reconnect grace period) while
other participants remained, so the earliest-joined remaining
participant was promoted to host in place — the room stays open rather
than closing.

```json
{
  "type": "host_changed",
  "room": "uuid-room-id",
  "client": "uuid-new-host-id",
  "payload": {
    "host_id": "uuid-new-host-id",
    "host_name": "Bob",
    "participant_count": 2
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

**Client processing:** update `state.isHost` (compare `payload.host_id`
to the local `client_id`) and force a full UI re-render — the host-only
Close/Leave button label only updates on a forced render, not the
normal fast-render path.

### `media_changed`

The host changed which item the room is watching, via `set_media`.
Sent to every room member except the host (who already knows).

```json
{
  "type": "media_changed",
  "room": "uuid-room-id",
  "payload": {
    "media_id": "abc123def456",
    "position": 0
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

**Client processing (guest):** reset sync state (`readyRoomId`,
`syncStatus`, `lastSyncServerTs/Position/PlayState`) as on join, call
`ensurePlayback(media_id, position)`, then `watchReady()`. Until the
guest's own current item matches `payload.media_id`, it should not
report `ready` for the new item, apply position corrections, or show a
"synced" status.

### `pong`

Response to ping.

```json
{
  "type": "pong",
  "payload": {
    "client_ts": 1678900000000
  },
  "ts": 1678900000050,
  "server_ts": 1678900000050
}
```

**Client-side RTT calculation:**
```javascript
const rtt = Date.now() - payload.client_ts;
const serverOffset = server_ts + (rtt / 2) - Date.now();
```

### `chat_message`

Chat message broadcast from server.

```json
{
  "type": "chat_message",
  "room": "uuid-room-id",
  "client": "uuid-sender-id",
  "payload": {
    "username": "Alice",
    "text": "Hello everyone!"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000050
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `username` | string | Sender's display name |
| `text` | string | Message text |

**Client processing:**
1. Add message to local chat history (max 100 messages)
2. If chat panel not visible, increment unread badge
3. Render message in chat UI

### `error`

Error response.

```json
{
  "type": "error",
  "payload": {
    "message": "Error description",
    "reason": "wrong_password"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `message` | string | Human-readable error description |
| `reason` | string | Optional machine-readable code for errors a client may want to special-case. Currently `"wrong_password"` and `"too_many_attempts"`, both from `join_room` |
| `retry_after_ms` | number | Only with `reason: "too_many_attempts"`: milliseconds until the user may try the room's password again |

## Sequence Diagram: Complete Session

```
Client A                    Server                    Client B
    │                          │                          │
    ├── WebSocket connect ────►│                          │
    │◄─── client_hello ────────┤                          │
    │◄─── room_list ───────────┤                          │
    │                          │                          │
    ├── create_room ──────────►│                          │
    │◄─── room_state ──────────┤                          │
    │                          ├─── room_list (broadcast) │
    │                          │                          │
    │                          │◄── WebSocket connect ────┤
    │                          ├─── client_hello ────────►│
    │                          ├─── room_list ───────────►│
    │                          │                          │
    │                          │◄── join_room ────────────┤
    │◄─ participants_update ───┤─── room_state ──────────►│
    │                          │                          │
    │                          │◄── ready ────────────────┤
    │                          │                          │
    ├── player_event (play) ──►│                          │
    │                          │   all_ready() = true     │
    │◄─ player_event ──────────┼─── player_event ────────►│
    │   target_ts = T+1000     │   target_ts = T+1000     │
    │                          │                          │
    │   [T+1000ms]             │                [T+1000ms]│
    │   video.play()           │              video.play()│
    │                          │                          │
    ├── state_update ─────────►│                          │
    │                          ├─── state_update ────────►│
    │                          │                          │
    ├── ping ─────────────────►│                          │
    │◄─── pong ────────────────┤                          │
    │                          │                          │
    ├── leave_room ───────────►│                          │
    │                          ├─── room_closed ─────────►│
    │◄─── room_list ───────────┼─── room_list ───────────►│
    │                          │                          │
```
