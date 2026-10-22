---
title: Protocol
parent: Technical Reference
nav_order: 1
---

# WebSocket Protocol Specification

## Overview

JellyWatchParty uses a JSON-over-WebSocket protocol between clients and the session server.

**Endpoint:** `ws(s)://<host>:3000/ws?client_id=<persistent-client-id>&resume=<resume-secret>`
(port from `PORT`, default 3000).

Upgrades whose `Origin` header isn't in `ALLOWED_ORIGINS` are refused with
`403 Origin not allowed`; requests without an `Origin` header (the plugin's
bridges, other non-browser clients) are allowed. See
[Server: `routes.rs`]({{ '/technical/server/' | relative_url }}#module-routesrs).

### `client_id` and `resume` Query Parameters

The web client generates a UUID once per browser tab, keeps it in
`sessionStorage` (it survives reloads of that tab; other tabs get their
own), and sends it as `?client_id=` on every connection attempt, including
reconnects. This lets the server reattach the connection to its existing
client entry (room membership, host role) and resend `room_state` instead
of treating it as a new client. Only values of 36 hex digits and hyphens
(UUID-shaped) are used; anything else is ignored and the server mints a
fresh id.

Client ids are not secret (every room member sees them in
`participants`), so knowing one is not enough to take a session over.
Each new client entry gets a random **resume secret** (64 hex characters),
sent only to that client in `client_hello`. A connection reattaches only if
it also sends that secret as `&resume=`. If the id is in use and the secret
is missing or wrong, the server ignores the requested id and issues a new
one in `client_hello`; the client should store the new id and secret. The
secret is replaced on every successful reattach, so always store the one
from the latest `client_hello`.

See [Server: Persistent Client ID]({{ '/technical/server/' | relative_url }}#persistent-client-id) for the
reattachment mechanics.

## Message Format

All messages are single JSON text frames:

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
| `room` | string | No | Room ID (required by room-scoped messages) |
| `client` | string | No | Client ID. The server identifies senders by their connection, not by this field |
| `payload` | object | No | Message-specific data |
| `ts` | number | Yes | Sender timestamp (ms since epoch). A client message without it fails to parse |
| `server_ts` | number | No | Server timestamp, set on server messages |

### Limits and generic errors

Every inbound frame (control frames included) counts toward a rate limit
of 30 messages per 1000 ms window per client. Non-text frames are otherwise
ignored. Generic `error` messages (`payload.message`):

| Message | Cause |
|---------|-------|
| `Rate limit exceeded` | More than 30 frames in the current 1 s window; the frame is dropped |
| `Message too large` | Frame larger than 64 KiB |
| `Invalid message format` | Not valid JSON, unknown structure, or missing `ts` |
| `Unknown message type` | `type` isn't one of the client messages below |
| `Authentication required` | `create_room` / `join_room` before a successful `auth` (JWT enabled) |

## Client → Server Messages

### `auth`

Authenticate (JWT mode) or identify (no `JWT_SECRET` set).

```json
{
  "type": "auth",
  "payload": {
    "token": "eyJhbGciOiJIUzI1NiIs...",
    "user_name": "Alice",
    "user_id": "jellyfin-user-id",
    "capabilities": ["democratic"]
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `token` | string | JWT from the plugin's `GET /JellyWatchParty/Token`. On success the client takes the token's `sub` as user id and `name` as display name, and gets `auth_success` |
| `user_name` | string | Without a token and with JWT disabled: display name (trimmed, control characters removed, max 100 characters) |
| `user_id` | string | Without a token and with JWT disabled: user id, set together with `user_name` |
| `capabilities` | array of strings | Optional. `"democratic"`: this client applies and echo-suppresses other members' play/pause/seek, so it may control a [democratic room](#democratic-mode). The web client sends it with every `auth`; clients that leave it out (e.g. plugin bridges) stay follow-only in democratic rooms. |

**Errors:** `"Authentication failed"` (invalid or expired token),
`"Authentication required: no token provided"` (JWT enabled, no token).
With JWT disabled every connection starts authenticated as `Anonymous`;
a token is then accepted unchecked and also yields `Anonymous`, so clients
identify with `user_name` instead.

### `list_rooms`

Request the room list. **Response:** `room_list`.

```json
{ "type": "list_rooms", "ts": 1678900000000 }
```

### `create_room`

Create a new room with the sender as host.

```json
{
  "type": "create_room",
  "payload": {
    "user_name": "Alice",
    "start_pos": 0.0,
    "media_id": "0123456789abcdef0123456789abcdef",
    "password": "optional-room-password",
    "started": false
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `user_name` | string | Optional display name; replaces the one from `auth`. The room is named `Room de <host name>` (rooms can be renamed from the admin panel). |
| `start_pos` | number | Initial position in seconds (0-86400, else 0) |
| `media_id` | string | Jellyfin item id, 32 hex characters; anything else is ignored (room without media) |
| `password` | string | Optional room password (empty = none). If set, `join_room` must supply it. Stored as a salted SHA-256 hash and never sent to any client. |
| `started` | boolean | Optional. `false` asks for the [start countdown](#start-countdown) (the host isn't playing yet and holds its first play). **Missing means `true`** (no countdown), so hosts that don't hold their first play, like the plugin's bridges, never leave guests waiting. |
| `bridge_device_id` | string | Optional. Sent by the plugin's in-panel bridges: the Jellyfin `DeviceId` of the session this connection stands in for (1-200 printable ASCII characters, else ignored). The admin panel shows such clients as *Plugin bridge*. Refused with `error` `reason: "device_already_bridged"` if another client in a room already drives that device. |

**Effects:**
- If the sender already hosts a room (other than a chat-created room), that
  room is closed (`room_closed`, reason `"Host started a new room"`); if it is
  a guest somewhere, it leaves that room first
- Sender gets `room_state`, then `participants`
- `room_list` to all clients

### `join_room`

Join an existing room.

```json
{
  "type": "join_room",
  "room": "uuid-room-id",
  "payload": {
    "user_name": "Bob",
    "password": "required-if-room-has-one"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `user_name` | string | Optional display name, as for `create_room` |
| `password` | string | Required only if the room has a password. Not checked for a client that's already a member (e.g. a re-sent join after a panel refresh). |
| `bridge_device_id` | string | Optional; as for `create_room`. |

**Errors** (`error` with `payload.reason`):

| `reason` | `message` | When |
|----------|-----------|------|
| `room_not_found` | `This room no longer exists` | Room is gone; a fresh `room_list` follows |
| `device_already_bridged` | `This device is already in a watch party` | See `bridge_device_id` |
| (none) | `Room is full` | Room already has 20 members |
| `too_many_attempts` | `Too many incorrect password attempts. Try again in <n>s` | Throttled, see below; also carries `retry_after_ms` |
| `wrong_password` | `Incorrect password` | Missing or wrong password |

After 5 wrong passwords within 60 s, the same user (keyed by `user_id`, i.e.
the JWT `sub`, not the client id) is refused for the rest of that 60 s window
without the password being checked. The throttle is per room and per user,
so one user guessing can't lock others out. A successful join clears the
user's count.

**Effects:**
- Leaves any other room first (normal leave there)
- Added to `room.clients` (if not already a member), removed from
  `room.ready_clients`; becomes host if the room has none (an empty admin
  group or chat room)
- Joiner gets `room_state`; other members get `participants_update`; everyone
  in the room gets `participants`
- `room_list` to all clients

### `leave_room`

Leave the current room. No payload.

**Effects:**
- Host leaves, others remain: the earliest-joined remaining member is
  promoted in place (a server-side device bridge only if no one else is
  left); `host_changed` and `participants` to the room
- Non-host leaves: `client_left` and `participants` to the room
- Last member leaves: the room is removed. Chat-created rooms instead stay
  open, hostless, until the integration's empty-room timeout.
- `room_list` to all clients

### `ready`

The client has the room's media loaded and can receive playback commands.

```json
{
  "type": "ready",
  "room": "uuid-room-id",
  "payload": { "media_id": "0123456789abcdef0123456789abcdef" },
  "ts": 1678900000000
}
```

The server ignores the payload (the web client sends `media_id` for logs)
and ignores `ready` from non-members.

**Effects:**
- Client added to `room.ready_clients`
- If a `pending_play` exists and everyone is now ready: the scheduled play
  goes out (see [`player_event`](#player_event-1))

### `player_event`

A playback command. Host only, except in a [democratic room](#democratic-mode),
where other members may send `play`, `pause` and `seek`.

```json
{
  "type": "player_event",
  "room": "uuid-room-id",
  "payload": {
    "action": "play",
    "position": 120.5,
    "play_state": "playing"
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `action` | string | `"play"`, `"pause"`, `"seek"` or `"buffering"` |
| `position` | number | Current position in seconds (0-86400; invalid values leave the room position unchanged) |
| `play_state` | string | Optional, `"playing"` or `"paused"`; sets the room's play state. `play`/`pause`/`buffering` override it. The web client sends it with `play`/`pause`/`seek`; receivers use it on `seek`. |

**Server behavior:**

| Situation | Behavior |
|-----------|----------|
| `play`, room not started | [Start countdown](#start-countdown) |
| `play`, not everyone ready | Creates `pending_play` (or updates its position); nothing is relayed. After `MAX_READY_WAIT_MS` (2 s), or as soon as everyone is ready, `play` goes to **everyone, host included**, with `target_server_ts = now + 1000 ms` |
| `pause` | Cancels any `pending_play`; relayed with `target_server_ts = now + 300 ms` |
| Anything else while a play is pending | Only updates the pending play's position; not relayed |
| `play` | Relayed with `target_server_ts = now + 1000 ms` |
| `seek`, `buffering` | Relayed with `target_server_ts = now + 300 ms` (`buffering` sets the room to paused) |

A relayed event goes to every other member (not the sender), with
`server_ts = target_server_ts` and `by` / `by_name` added (see
[`player_event`](#player_event-1) below). It updates `room.state`,
`room.last_command_ts` (the state-update cooldown) and `room.last_command_by`.

A message the sender isn't allowed to send is dropped without an answer.

### `state_update`

Periodic playback state from the host (host only, also in democratic rooms).
The web client sends it every second while playing or paused, plus right
after each of its own `player_event`s.

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

`play_state: "playing"` marks the room started. **Server filtering**, in order:
1. Dropped while a play is pending (its position is taken over instead)
2. Dropped within 2000 ms of the last `player_event` if that command came from
   someone else (a democratic-mode member or a chat bot user), even with a
   `play_state` change, so the host's stale state can't undo the command
   before its player has carried it out
3. Accepted if `play_state` differs from the room's
4. Otherwise dropped if within 2000 ms of the host's own last command, within
   500 ms of the last accepted state, if the position moved back by 0.5-2 s
   (HLS jitter), or if it advanced by less than 0.5 s

Accepted updates go to every other member with `server_ts` set to the
server's time.

### `ping`

Latency measurement and clock synchronization. **Response:** `pong`, with
`room`, `client` and `payload` echoed back unchanged.

```json
{
  "type": "ping",
  "payload": { "client_ts": 1678900000000 },
  "ts": 1678900000000
}
```

### `chat_message`

Send a text message to the room.

```json
{
  "type": "chat_message",
  "room": "uuid-room-id",
  "payload": { "text": "Hello everyone!" },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `text` | string | Message text, at most 500 bytes (UTF-8). Not trimmed by the server; the web client trims it and doesn't send blank messages. |

**Effects:**
- Dropped silently if the sender isn't a member of the room
- Appended to the room's chat history (last 50 kept, replayed in `room_state`)
- `chat_message` to every member, sender included

**Errors:**
- `"Chat message cannot be empty"` - missing or empty `text`
- `"Chat message too long (max 500 characters)"` - `text` longer than 500 bytes
- `"Room ID required for chat"` - missing `room`

### `set_media`

Change which item the room is watching (host only): when the host starts
something after creating a room without media, or switches to another item
mid-session.

```json
{
  "type": "set_media",
  "room": "uuid-room-id",
  "payload": {
    "media_id": "0123456789abcdef0123456789abcdef",
    "position": 0
  },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `media_id` | string | Required. 32 hex characters |
| `position` | number | Starting position in seconds (default `0`) |

**Effects:**
- No-op (nothing changed, nothing sent) if `media_id` already matches
  `room.media_id`, or the sender isn't the host
- Sets `room.media_id`, resets `room.state` to `{position, play_state:
  "paused"}`, clears `pending_play` and the command cooldown, and resets
  `ready_clients` to just the host, so the host's next `play` waits for
  guests to load the new item (up to 2 s, or 10 s if the room hasn't started)
- `media_changed` to every other member
- `room_list` to all clients (lobby cards refresh their poster)

**Errors:**
- `"media_id is required"` - missing `media_id`
- `"Invalid media_id"` - not 32 hex characters

### `client_status`

A member reports its own playback status for the room's participant list.
The web client sends it when the status changes, at most once per second;
server-side device bridges send it too.

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
| `loading` | Waiting for a scheduled play, or not on the room's media yet |
| `idle` | Not in the player |
| `playing` / `paused` | The host's own state |

**Effects:** ignored unless the value is one of the above, the sender is in
the room and the status changed; otherwise everyone in the room gets
`participants`.

### `set_democratic`

Turn the room's [democratic mode](#democratic-mode) on or off. Host only.

```json
{
  "type": "set_democratic",
  "room": "uuid-room-id",
  "payload": { "enabled": true },
  "ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `enabled` | boolean | Required |

**Effects:**
- On a change: sets `room.democratic`, sends `democratic_changed` to every
  member (sender included) and `room_list` to all clients. No change, no
  messages.
- The plugin's *Allow democratic mode* setting only decides whether the web
  client shows the switch; the server doesn't know it.

**Errors:**
- `"enabled is required"` - missing or non-boolean `enabled`
- `"Only the host can change this setting"` - sender isn't the host

### `client_log`

Debug log line from a client, written to the server log as
`[CLIENT:<first 8 chars of id>:<category>] <message>`. No response.

```json
{
  "type": "client_log",
  "payload": { "category": "SYNC", "message": "drift=+0.4s" },
  "ts": 1678900000000
}
```

### Democratic mode

A room has two flags, both off when it is created:

- `democratic`: set by the host (`set_democratic`), an admin
  (`PATCH /api/rooms/{id}` on the admin panel) or a chat room's owner
  ([integration API]({{ '/technical/integration-api/' | relative_url }})).
- `device_control`: admin only. Lets server-side device bridges take part.

The host may always send anything. Another member's message is accepted
only if all of these hold:

1. the room is `democratic` and has started (the first, countdown play
   stays with the host);
2. it is a `player_event` with `action` `play`, `pause` or `seek`
   (`state_update`, `buffering` and `set_media` stay host only, so the host
   remains the room's position source);
3. the sender is in the room, and is either a client that announced
   `capabilities: ["democratic"]` and isn't a plugin bridge (no
   `bridge_device_id`), or a server-side device bridge (admin panel / chat
   bot) while `device_control` is on.

Anything else from a non-host is dropped silently. Chat bot users send
commands through the integration API instead; those reach every member,
host included, with `by: ""`.

## Server → Client Messages

### `client_hello`

Sent right after the WebSocket connects, followed by `room_list` (and
`room_state` + `participants` when the connection reattached to a room
member). `client_id` may differ from the requested `?client_id=` (see
[above](#client_id-and-resume-query-parameters)); `resume_secret` is needed
to reattach to this id later and is never sent to anyone else. `room_id` is
the room the server has this client in, or `null`: a reattaching client
that thinks it is in a room but gets `null` was removed (or the room
closed) while it was offline and should go back to the lobby.

```json
{
  "type": "client_hello",
  "client": "uuid-client-id",
  "payload": {
    "client_id": "uuid-client-id",
    "resume_secret": "64-hex-chars",
    "room_id": null
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `auth_success`

Answer to an `auth` with a valid token.

```json
{
  "type": "auth_success",
  "client": "uuid-client-id",
  "payload": { "user_name": "Alice" },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `room_list`

All rooms. Sent on connect, on `list_rooms`, and to every client whenever
rooms or member counts change.

```json
{
  "type": "room_list",
  "payload": [
    {
      "id": "uuid-room-id",
      "name": "Room de Alice",
      "count": 3,
      "media_id": "0123456789abcdef0123456789abcdef",
      "has_password": false,
      "democratic": false
    }
  ],
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `room_state`

Full room state, for the recipient (`client`). Sent after `create_room` or
`join_room`, when an admin adds the client to a room, and on reattachment
after a dropped-connection reconnect (see
[Server: Reconnect and Room Lifecycle]({{ '/technical/server/' | relative_url }}#reconnect-and-room-lifecycle)).

```json
{
  "type": "room_state",
  "room": "uuid-room-id",
  "client": "uuid-client-id",
  "payload": {
    "name": "Room de Alice",
    "host_id": "uuid-host-id",
    "participant_count": 3,
    "media_id": "0123456789abcdef0123456789abcdef",
    "state": {
      "position": 120.5,
      "play_state": "playing"
    },
    "started": true,
    "democratic": false,
    "device_control": false,
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
| `media_id` | string or null | The room's item, `null` if none yet |
| `state` | object | Last known `position` (seconds) and `play_state` |
| `started` | boolean | `false` until the room's first play has gone out (see [Start countdown](#start-countdown)). The web client treats a missing value as `true`. |
| `democratic` | boolean | The room is in [democratic mode](#democratic-mode). Missing is treated as `false`. |
| `device_control` | boolean | Server-side device bridges may control the room too (admin setting; only matters with `democratic`). |
| `chat_history` | array | Up to the last 50 chat messages, oldest first. Replayed on join and on reattach. |
| `admin_moved` | boolean | Only present (`true`) when an admin put this client into the room from the [admin panel]({{ '/admin-panel/' | relative_url }}). The server already took it out of any previous room (that room sees a normal leave). The web client drops its old room state and shows a toast. |

### `start_pending`

The host's first play arrived but not everyone is ready. Sent to the whole
room, host included.

```json
{
  "type": "start_pending",
  "room": "uuid-room-id",
  "payload": { "timeout_ms": 10000 },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

Clients show "Waiting for everyone to be ready...". The server starts when
everyone has sent `ready`, or after `timeout_ms` at the latest.

### Start countdown

A room's first play (`started` is `false`) works differently from later ones:

1. The host's client keeps its video paused and sends `player_event` `play`.
   It sends no `state_update` while waiting. If the host's item is still
   being confirmed (before `set_media`), the play is held and sent right
   after `set_media`, so the server already knows which item everyone has to
   load.
2. If everyone is ready, the server starts right away; otherwise it sends
   `start_pending` and waits for `ready` from everyone, up to 10 s.
3. The server sends `player_event` `play` to **everyone, host included**, with
   `"countdown": true` and `target_server_ts` 3 s ahead, and marks the room
   started. Clients show 3, 2, 1 against server time and start at
   `target_server_ts`.

With nobody else in the room there is no countdown (`"countdown": false`,
`target_server_ts` 1 s ahead). A room counts as started from the beginning
if it was created with `started: true` or without `started`, and so do
admin groups and chat-created rooms; it also becomes started once the host
sends a `state_update` with `play_state: "playing"` or a server-side device
bridge becomes host. If the host's client gets no answer within 15 s, it
just plays. Later plays use the normal schedule.

### `participants_update`

Member count, sent to the other members when someone joins (the joiner gets
`room_state`).

```json
{
  "type": "participants_update",
  "room": "uuid-room-id",
  "payload": { "participant_count": 4 },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `participants`

The room's member list, in join order. Sent to the whole room when someone
joins or leaves, the host changes, or a status changes; also sent to a
client that reattaches.

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

`status` is the member's last `client_status`, or `unknown` before its
first report.

### `player_event`

Playback command relayed from the host, from another member (democratic
room) or from a chat bot user, or a scheduled play released by the server.

```json
{
  "type": "player_event",
  "room": "uuid-room-id",
  "payload": {
    "action": "play",
    "position": 120.5,
    "play_state": "playing",
    "target_server_ts": 1678900001000,
    "by": "uuid-sender-id",
    "by_name": "Alice"
  },
  "ts": 1678900000000,
  "server_ts": 1678900001000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `action` | string | `"play"`, `"pause"`, `"seek"` or `"buffering"` |
| `position` | number | Reference position (seconds) |
| `play_state` | string | Passed through from the sender, if it sent one |
| `target_server_ts` | number | Server time at which to execute (also in `server_ts`) |
| `countdown` | boolean | Only on server-released plays: `true` for the room's first play with a countdown |
| `by` | string | Client id of the sender; `""` for chat bot commands. Absent on server-released plays (start countdown, pending play). |
| `by_name` | string | Sender's display name (the chat user's name for bot commands), for toasts like "Alice paused playback". |

A member's command goes to every other member; a chat bot command and a
server-released play go to everyone, host included.

**Web client processing (guest):**
1. Ignore if not on the room's media yet
2. Engage the `isSyncing` lock (2 s) so the resulting video events aren't
   sent back
3. Target position: `play`/`pause` positions are advanced by the time
   since `server_ts` plus a 300 ms lead; `seek`/`buffering` use the position
   as is. If the video is more than 1 s off, pause and seek there.
4. `play`: start at `target_server_ts` (showing the countdown if
   `countdown`); `pause`/`buffering`: pause at once; `seek`: resume only if
   `play_state` is `playing`

The host normally ignores relayed events. In a democratic room it applies
other members' `play`/`pause`/`seek` like a guest would (not its own, not
during a media switch, and not a `play` while already playing), but keeps
sending `state_update` as the position source.

### `state_update`

The host's state, relayed after the [server filtering](#state_update).
Same payload as sent by the host; `server_ts` is the server's receive time.

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

The room was closed, or this client was removed from it. Examples of
`reason`: `"Host started a new room"`, a reason from an admin or the chat
room's owner, `"An admin removed you from the room"`.

```json
{
  "type": "room_closed",
  "room": "uuid-room-id",
  "payload": { "reason": "An admin removed you from the room", "removed": true },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

`reason` is shown to the user. `removed` is only present (`true`) when just
this client was taken out and the room goes on.

### `client_left`

A non-host member left.

```json
{
  "type": "client_left",
  "room": "uuid-room-id",
  "client": "uuid-left-client-id",
  "payload": { "participant_count": 2 },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

### `host_changed`

Someone else is host now. Sent when the host left (or its reconnect grace
period ran out) while other members remained, when an admin or a chat
room's owner hands the host role to another member, and when the first
member arrives in a hostless room others are already waiting in (to those
others).

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

**Web client processing:** set `state.isHost` (compare `payload.host_id` to
its own client id), show a toast and force a full re-render (host-only
controls aren't on the fast render path).

### `democratic_changed`

The room's [democratic mode](#democratic-mode) or device control changed.
Sent to every member, including the host who changed it.

```json
{
  "type": "democratic_changed",
  "room": "uuid-room-id",
  "payload": {
    "enabled": true,
    "device_control": false,
    "by": "Alice"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `enabled` | boolean | Democratic mode is on |
| `device_control` | boolean | Server-side device bridges may control the room too |
| `by` | string | Who changed it: the host's or chat user's name, or `"an admin"` |

**Web client processing:** update `state.democraticMode` /
`state.deviceControl`, show a toast if the mode changed, and force a full
re-render.

### `media_changed`

The host changed the room's item via `set_media`. Sent to every member
except the host.

```json
{
  "type": "media_changed",
  "room": "uuid-room-id",
  "payload": {
    "media_id": "0123456789abcdef0123456789abcdef",
    "position": 0
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

**Web client processing (guest):** reset its sync state as on join
(`readyRoomId`, `syncStatus`, `lastSync*`, initial sync, command guard,
pending timers), call `ensurePlayback(media_id, position)`, then
`watchReady()`. Until its own item matches `payload.media_id`, it doesn't
report `ready`, apply position corrections, or show "synced".

### `pong`

Response to `ping`; the payload is echoed.

```json
{
  "type": "pong",
  "payload": { "client_ts": 1678900000000 },
  "ts": 1678900000050,
  "server_ts": 1678900000050
}
```

The client computes `rtt = now - client_ts` and an offset sample
`server_ts + rtt / 2 - now`; see
[Sync Algorithms: Clock Synchronization]({{ '/technical/sync/' | relative_url }}#clock-synchronization)
for how samples are combined.

### `chat_message`

Chat message broadcast to the room.

```json
{
  "type": "chat_message",
  "room": "uuid-room-id",
  "client": "uuid-sender-id",
  "payload": {
    "username": "Alice",
    "text": "Hello everyone!"
  },
  "ts": 1678900000050,
  "server_ts": 1678900000050
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `username` | string | Sender's display name |
| `text` | string | Message text |

The web client keeps the last 100 messages locally, and shows an unread
badge and a toast while the chat panel is hidden.

### `error`

```json
{
  "type": "error",
  "client": "uuid-client-id",
  "payload": {
    "message": "Incorrect password",
    "reason": "wrong_password"
  },
  "ts": 1678900000000,
  "server_ts": 1678900000000
}
```

| Payload Field | Type | Description |
|---------------|------|-------------|
| `message` | string | Human-readable error, shown to the user |
| `reason` | string | Only on `join_room` / `create_room` errors a client may want to special-case: `"wrong_password"`, `"too_many_attempts"`, `"room_not_found"`, `"device_already_bridged"`. Join errors also carry `room`. |
| `retry_after_ms` | number | Only with `reason: "too_many_attempts"`: milliseconds until the user may try the room's password again |

## Sequence Diagram: Complete Session

A host (A) and a guest (B); the room is created paused, so its first play
uses the countdown.

```
Client A                    Server                    Client B
    │                          │                          │
    ├── WebSocket connect ────►│                          │
    │◄─── client_hello ────────┤                          │
    │◄─── room_list ───────────┤                          │
    ├── auth ─────────────────►│                          │
    │◄─── auth_success ────────┤                          │
    │                          │                          │
    ├── create_room ──────────►│                          │
    │   (started: false)       │                          │
    │◄─── room_state ──────────┤                          │
    │◄─── participants ────────┤                          │
    │                          ├─── room_list (all) ─────►│
    │                          │                          │
    │                          │◄── join_room ────────────┤
    │                          ├─── room_state ──────────►│
    │◄─ participants_update ───┤                          │
    │◄─ participants ──────────┼─── participants ────────►│
    │                          │                          │
    │                          │◄── ready ────────────────┤
    │                          │                          │
    ├── player_event (play) ──►│                          │
    │                          │   all ready, not started │
    │◄─ player_event ──────────┼─── player_event ────────►│
    │   countdown, T+3000      │   countdown, T+3000      │
    │   [T+3000] video.play()  │   [T+3000] video.play()  │
    │                          │                          │
    ├── state_update ─────────►│                          │
    │                          ├─── state_update ────────►│
    │                          │                          │
    ├── ping ─────────────────►│                          │
    │◄─── pong ────────────────┤                          │
    │                          │                          │
    ├── leave_room ───────────►│                          │
    │                          ├─── host_changed ────────►│
    │                          ├─── participants ────────►│
    │◄─── room_list ───────────┼─── room_list ───────────►│
    │                          │                          │
```
