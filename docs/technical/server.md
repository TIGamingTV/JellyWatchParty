---
title: Server
parent: Technical Reference
nav_order: 2
---

# Session Server (Rust)

## Overview

The session server (`src/server`) is an asynchronous Rust application using
Axum for HTTP/WebSocket handling and Tokio as the runtime. It keeps rooms
and clients in memory and relays playback between room members.

## Module Structure

```
src/server/src/
├── main.rs           # Entry point: listeners, background tasks, graceful shutdown
├── types.rs          # Data structures
├── routes.rs         # Axum router (/ws, /health), origin guard, CORS
├── tasks.rs          # Zombie cleanup, empty-group reaper, shutdown signal
├── messaging.rs      # Payload builders and send/broadcast helpers
├── auth.rs           # JWT validation (optional)
├── utils.rs          # now_ms, random_token
├── password.rs       # Room password hashing, constant-time compare
├── events.rs         # "Rooms changed" counter (wakes the integration long poll)
├── admin/            # Admin panel (own listener, ADMIN_PORT, default 3001)
│   ├── mod.rs            # Router, session/CSRF middleware, security headers
│   ├── config.rs         # ADMIN_* environment variables
│   ├── auth.rs           # Login, sessions, failed-login throttle
│   ├── api.rs            # JSON API (overview, rooms, members, host, Jellyfin sessions)
│   ├── integrations.rs   # Chat integration settings, link codes, activity log
│   ├── ui.rs             # Serves the embedded UI
│   └── ui/               # index.html, app.js, app.css (no build step)
├── integration/      # Chat integrations (Discord/Telegram bot sidecars), see integration-api.md
│   ├── mod.rs            # Integration state, user cache, guards, audit, reaper
│   ├── config.rs         # DATA_DIR, INTEGRATION_HOST/PORT, DISCORD_/TELEGRAM_INTEGRATION_TOKEN
│   ├── store.rs          # integrations.json: settings, code HMACs, links
│   ├── actions.rs        # Who may do what; chat room operations
│   ├── view.rs           # Room list for sidecars
│   └── api.rs            # Token-protected HTTP API (own listener, default 3002)
├── jellyfin/         # Admin panel device bridge (Jellyfin REST API)
│   ├── mod.rs            # JELLYFIN_* / BRIDGE_POLL_INTERVAL_MS configuration
│   ├── api.rs            # /Sessions, Playing/{cmd}, PlayNow (reqwest)
│   ├── logic.rs          # Pure host/receiver decisions + position estimate
│   ├── bridge.rs         # Poller, one task per bridged device, add/remove
│   └── time.rs           # ISO-8601 timestamp parsing
├── ws/
│   ├── mod.rs
│   ├── connection.rs     # WebSocket connection lifecycle, reattach
│   ├── dispatch.rs       # Rate/size limits, message dispatch, send_error
│   ├── constants.rs      # Protocol constants and limits
│   ├── validation.rs     # Position/media id/name/bridge_device_id validation
│   ├── pending_play.rs   # Ready gate, scheduled plays, start countdown
│   └── handlers/
│       ├── mod.rs
│       ├── auth.rs       # auth (JWT or identity), capabilities
│       ├── chat.rs       # chat_message
│       ├── create.rs     # create_room
│       ├── join.rs       # join_room, password throttle
│       ├── media.rs      # set_media
│       ├── misc.rs       # ping, ready, leave_room, set_democratic, client_log, unknown
│       ├── playback.rs   # player_event, state_update, may_control, server_command
│       └── status.rs     # client_status
└── room/
    ├── mod.rs
    ├── leave.rs          # Leave / disconnect, host promotion
    ├── close.rs          # Room closure (async wrapper)
    ├── ops.rs            # Shared room operations (add/move, kick, set host, close, groups)
    └── reconnect.rs      # Grace-period disconnect + reattachment
```

## Module: `main.rs`

Reads the configuration, creates the shared state, starts the zombie
cleanup, the admin panel (with the Jellyfin device bridge and chat
integrations, which only run alongside it) and the main listener on
`HOST:PORT` (default `0.0.0.0:3000`). SIGTERM/SIGINT trigger a graceful
shutdown of every listener.

```rust
let clients: Clients = Arc::new(RwLock::new(HashMap::new()));
let rooms: Rooms = Arc::new(RwLock::new(HashMap::new()));
tasks::spawn_zombie_cleanup(clients.clone(), rooms.clone());
start_admin_panel(&clients, &rooms, jwt_config.enabled, shutdown_rx.clone());
let app = routes::build_router(clients, rooms, jwt_config, allowed_origins);
axum::serve(listener, app)
    .with_graceful_shutdown(tasks::wait_for_shutdown(shutdown_rx))
    .await
```

> The server speaks HTTP/1.1 only. Every client performs an HTTP/1.1
> `Upgrade` for WebSockets, so the `axum` and `reqwest` `http2` features are
> left off and `h2` never enters the dependency tree.

| State | Type | Description |
|-------|------|-------------|
| `clients` | `Clients` | All client entries, keyed by client id (live, in grace period, and server-side device bridges) |
| `rooms` | `Rooms` | All rooms, keyed by room id |

## Module: `routes.rs`

| Route | Description |
|-------|-------------|
| `GET /ws` | WebSocket upgrade. Query parameters `client_id` and `resume` are passed to `ws::client_connection`. Guarded by `origin_guard`. |
| `GET /health` | `{"status":"ok","auth_enabled":<bool>}`. Not origin-guarded (health checks send no `Origin`); CORS allows `GET` from the allowed origins. |

`ALLOWED_ORIGINS` is a comma-separated list, default
`http://localhost:8096,https://localhost:8096`. An entry `*` allows every
origin (logged as a security warning). A request whose `Origin` isn't
allowed gets `403 Origin not allowed`; a request without an `Origin` header
(or with a non-ASCII one) is let through, because non-browser clients such
as the plugin's bridges send none.

## Module: `types.rs`

```rust
pub type Clients = Arc<RwLock<HashMap<String, Client>>>;   // tokio RwLock
pub type Rooms = Arc<RwLock<HashMap<String, Room>>>;
pub type ClientSender = mpsc::Sender<OutboundMessage>;       // bounded
```

`OutboundMessage` is an already-serialized JSON text frame.

### Struct `Client`

| Field | Type | Description |
|-------|------|-------------|
| `sender` | `ClientSender` | Bounded outbound queue (100 messages) |
| `room_id` | `Option<String>` | Current room |
| `user_id` | `String` | JWT `sub` (or the identity's `user_id`; `anonymous` with JWT disabled) |
| `user_name` | `String` | Display name |
| `authenticated` | `bool` | `true` from the start with JWT disabled, else after a valid `auth` |
| `message_count` / `last_reset` | `u32` / `u64` | Rate-limit window |
| `last_seen` | `u64` | Last inbound frame (zombie detection) |
| `resume_secret` | `String` | 64 hex chars, needed to reattach (see [Persistent Client ID](#persistent-client-id)) |
| `connected_at` | `u64` | When the entry was registered |
| `conn_id` | `u64` | Id of the currently attached connection |
| `connected` | `bool` | `false` once that connection ended (grace period) |
| `kind` | `ClientKind` | `Web` (a websocket) or `Bridge` (a Jellyfin session driven by this server, see [`jellyfin/`](#module-jellyfin)) |
| `bridge_device` | `Option<String>` | Jellyfin DeviceId this client stands in for (`bridge_device_id` from the plugin's bridges, or set by the server's own bridges) |
| `democratic_capable` | `bool` | Sent `capabilities: ["democratic"]` in `auth` |

### Struct `Room`

| Field | Type | Description |
|-------|------|-------------|
| `room_id` | `String` | UUID |
| `name` | `String` | Display name (`Room de <host>` for client-created rooms) |
| `host_id` | `String` | Host client id; empty while hostless |
| `media_id` | `Option<String>` | Jellyfin item id (32 hex) |
| `clients` | `Vec<String>` | Members in join order |
| `ready_clients` | `HashSet<String>` | Members ready to receive a play |
| `pending_play` | `Option<PendingPlay>` | A play waiting for everyone to be ready |
| `state` | `PlaybackState` | Last known position and play state |
| `last_state_ts` | `u64` | When `state` was last set |
| `last_command_ts` | `u64` | Last `player_event` (state-update cooldown) |
| `chat_history` | `VecDeque<ChatHistoryEntry>` | Last 50 chat messages |
| `password_hash` | `Option<(String, String)>` | Salt and SHA-256 hash of the room password |
| `client_status` | `HashMap<String, String>` | Last `client_status` per member |
| `failed_joins` | `HashMap<String, (u32, u64)>` | Wrong passwords per user id: count, window start |
| `started` | `bool` | The room's first play has gone out |
| `admin_created` | `bool` | Empty group created from the admin panel |
| `created_at` | `u64` | Creation time |
| `chat` | `Option<ChatRoom>` | Set for rooms created from a chat integration (owner, participants, panel) |
| `democratic` | `bool` | Democratic mode (see `may_control` below) |
| `device_control` | `bool` | With `democratic`, server-side device bridges may control too (admin only) |
| `last_command_by` | `String` | Client id that sent the last play/pause/seek; empty for server commands (chat bots) |
| `last_change_ts` | `u64` | When the play state or position last changed other than by playing on (any `player_event`, a play-state change, a jump of more than 2 s from the expected position, `set_media`, a scheduled play). Device bridges use it for device-control detection. |

`PlaybackState` is `{ position: f64, play_state: String }` (`"playing"` /
`"paused"`); `PendingPlay` is `{ position: f64, created_at: u64 }`.

### Messages

`IncomingMessage` (client → server) has `type` parsed into
`ClientMessageType` (unknown values map to `Unknown`), optional `room`,
`client`, `payload`, `server_ts`, and a required `ts`. `WsMessage`
(server → client) has the same fields with `type` as a string.

## Module: `ws/`

### Constants (`ws/constants.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `CLIENT_CHANNEL_BUFFER` | 100 | Outbound queue size per client |
| `PLAY_SCHEDULE_MS` | 1000 | Delay before a play executes (ms) |
| `CONTROL_SCHEDULE_MS` | 300 | Delay before pause/seek/buffering (ms) |
| `MAX_READY_WAIT_MS` | 2000 | Max wait for everyone to be ready (ms) |
| `START_READY_WAIT_MS` | 10000 | Max wait for everyone before the room's first play (ms) |
| `START_COUNTDOWN_MS` | 3000 | Start countdown length (ms) |
| `MIN_STATE_UPDATE_INTERVAL_MS` | 500 | Min interval between accepted state updates (ms) |
| `POSITION_JITTER_THRESHOLD` | 0.5 | Position noise threshold (s) |
| `COMMAND_COOLDOWN_MS` | 2000 | State-update cooldown after a `player_event` (ms) |
| `RATE_LIMIT_MESSAGES` / `RATE_LIMIT_WINDOW_MS` | 30 / 1000 | Inbound frames per window per client |
| `MAX_FAILED_JOINS` / `FAILED_JOIN_WINDOW_MS` | 5 / 60000 | Room password throttle per user per room |
| `MAX_POSITION_SECONDS` | 86400 | Max valid position (s) |
| `MAX_MESSAGE_SIZE` | 65536 | Max inbound frame size (64 KiB) |
| `MAX_NAME_LENGTH` | 100 | Max display name length (characters) |
| `MAX_CHAT_MESSAGE_LENGTH` | 500 | Max chat message length (bytes) |
| `MAX_CHAT_HISTORY` | 50 | Chat messages kept per room |

The member limit, `MAX_CLIENTS_PER_ROOM` = 20, lives in `room/ops.rs`.

### `client_connection` (`ws/connection.rs`)

1. Splits the socket; a task drains the client's bounded queue into it, one
   unfragmented text frame per message.
2. Takes `client_id` if it is UUID-shaped (36 hex digits/hyphens), else a
   new UUIDv4, and decides with `decide_attach`: **reattach** (entry exists,
   `resume` matches in constant time, not a device bridge), **register** (no
   entry), or **refuse** (entry exists, wrong/missing secret: register under
   a new id instead).
3. On reattach: swaps in the new sender, new `conn_id`, fresh resume secret.
   On register: a new `Client` (authenticated as `Anonymous` if JWT is
   disabled).
4. Sends `client_hello`, `room_list`, and on reattach to a room
   `resend_room_state` (`room_state` + `participants`).
5. Runs every inbound frame through `dispatch::client_msg`.
6. When the socket ends: if this connection is still the entry's
   `conn_id`, marks it `connected = false` and calls `schedule_disconnect`.

### Dispatch (`ws/dispatch.rs`)

`client_msg` first applies the rate limit (also refreshes `last_seen`; every
frame type counts), then the size limit, ignores non-text frames, parses
JSON, and routes by type:

| Type | Handler |
|------|---------|
| `auth` | `handle_auth`: JWT validation, or identity (`user_name`/`user_id`) with JWT disabled; records `capabilities` |
| `list_rooms` | `send_room_list` |
| `create_room` | `handle_create_room` (auth required) |
| `join_room` | `handle_join_room` (auth required, password throttle, then `ops::add_member`) |
| `ready` | `handle_ready`: members only; releases a pending play once everyone is ready |
| `leave_room` | `handle_leave_room` → `room::handle_leave` |
| `player_event`, `state_update` | `handle_playback` |
| `set_media` | `handle_set_media` |
| `client_status` | `handle_client_status` |
| `set_democratic` | `handle_set_democratic` |
| `chat_message` | `handle_chat_message` |
| `ping` | `handle_ping`: `pong` echoing `room`, `client`, `payload` |
| `client_log` | Logged as `[CLIENT:<id prefix>:<category>] <message>` |
| other | `error` "Unknown message type" |

`dispatch_internal` runs messages from the server's own device bridges
through the same handlers (only `ready`, `player_event`, `state_update`,
`set_media`, `client_status`), without rate or size limits.
`send_error` sends `error {message}` to one client.

### Playback (`ws/handlers/playback.rs`)

#### `may_control`
Who may send a `player_event` / `state_update`. The host: anything. Anyone
else only when the room is `democratic` and `started`, the message is a
`player_event` `play`, `pause` or `seek`, the sender is in the room, and it
is either a `democratic_capable` client without `bridge_device` (not a
plugin bridge), or a `ClientKind::Bridge` while `device_control` is on.
Everything else is dropped silently.

#### `handle_playback`
After `may_control`:

1. `pause` clears any `pending_play`.
2. `play` in a room that hasn't started: if everyone is ready,
   `start_scheduled_play` at once; otherwise create the pending play,
   send `start_pending` and wait up to `START_READY_WAIT_MS`.
3. `play` with someone not ready: create (or update) `pending_play`, wait up
   to `MAX_READY_WAIT_MS`.
4. While a play is pending, any other message only updates its position.
5. A `state_update` with `play_state: "playing"` marks the room started,
   then passes `should_process_state_update` (cooldown, rate limit, jitter;
   see [Protocol: `state_update`]({{ '/technical/protocol/' | relative_url }}#state_update)).
   During the 2 s cooldown after a command from someone else
   (`last_command_by` isn't the sender) every update is dropped, play-state
   changes included.
6. `apply_state_changes` updates `room.state`, `last_state_ts`,
   `last_change_ts`; for a `player_event` also `last_command_ts`,
   `last_command_by` and `target_server_ts` (+1000 ms for play, +300 ms
   otherwise). `by`/`by_name` are added and the message goes to every other
   member.

#### `server_command`
A play, pause or seek from outside the room (the integration API's
`playback`, for chat bot users). Applied like a `player_event` with an
empty sender (so the host's next updates hit the cooldown), and sent to
every member, host included, with `by: ""` and the chat user's `by_name`.
Seeks are clamped to 0..`MAX_POSITION_SECONDS`. The caller checks that the
room is democratic and started.

### Scheduled plays (`ws/pending_play.rs`)

```rust
pub(super) fn all_ready(room: &Room) -> bool {
    room.ready_clients.len() >= room.clients.len()
}
```

`start_scheduled_play(room, clients, position, now)` sends `player_event`
`play` to **every** member (host included) with `target_server_ts = now +
START_COUNTDOWN_MS` and `countdown: true` for a not-yet-started room with
more than one member, else `now + PLAY_SCHEDULE_MS` and `countdown: false`.
It marks the room started, clears `pending_play` and sets the room to
playing at `position`. `schedule_pending_play` spawns a timer that calls it
after the wait unless that pending play (matched by `created_at`) is gone.

### Other handlers

- **`create_room`** (`create.rs`): closes a room the sender already hosts
  (`"Host started a new room"`; chat rooms are left instead), leaves any
  room it is a guest in, builds the room (`Room de <name>`, `media_id` and
  `start_pos` validated, `started` defaults to `true`, sender is host and
  ready), sends `room_state` + `participants`, broadcasts `room_list`.
- **`join_room`** (`join.rs`): `room_not_found` (plus a fresh `room_list`),
  `device_already_bridged`, `Room is full`, then for non-members of a
  password room the throttle (`lockout_remaining_ms` /
  `record_failed_join`, keyed by `user_id`) and `verify_password`. Then
  `ops::add_member` and a `room_list` broadcast.
- **`set_media`** (`media.rs`): host only, silently ignored otherwise or if
  the id is unchanged. Sets `media_id`, resets `state`, `pending_play`,
  `last_command_ts` and `ready_clients` (to just the host), sends
  `media_changed` to the other members and broadcasts `room_list`.
- **`set_democratic`** (`misc.rs`): host only (others get `error` "Only the
  host can change this setting"); calls `ops::set_democratic`, then
  broadcasts `room_list` on a change.
- **`client_status`** (`status.rs`): stores one of `synced`, `syncing`,
  `buffering`, `loading`, `idle`, `playing`, `paused` for a member and
  broadcasts `participants` on a change.
- **`chat_message`** (`chat.rs`): validates (non-empty, at most 500 bytes),
  drops non-members silently, appends to `chat_history` (max 50), sends to
  every member including the sender.

See [Protocol]({{ '/technical/protocol/' | relative_url }}) for payloads and error strings.

## Module: `room/`

`ops.rs` holds the operations that both the websocket handlers and the
admin/integration APIs use, all on already-locked maps (rooms first, then
clients):

| Function | Used by | What it does |
|----------|---------|--------------|
| `add_member` | `join_room`, admin "add", device bridges (`jellyfin/bridge.rs`) | Moves a client into a room. If it is in another room it leaves that one first (normal leave there). Makes it host if the room has none. Removes it from `ready_clients`. Sends `room_state` (with `admin_moved` when an admin did it), `participants_update` to the others and `participants` to all. No password check. |
| `kick_member` | admin / chat owner "remove" | A normal leave for the room, plus `room_closed {reason, removed: true}` to the removed client. |
| `set_host` | admin / chat owner "make host" | Hands over the host role, drops a pending play, sends `host_changed` and `participants`. |
| `close_room` | host starting a new room, admin / chat owner "close" | Removes the room and sends `room_closed {reason}` to its members. |
| `create_group` / `update_room` | admin | Creates an empty, hostless group (password optional, `started: true`); renames it or sets/clears its password (clears the password throttle). |
| `create_chat_room` | chat integration | Like a group, but owned by a chat user and kept open while empty. |
| `set_democratic` | `set_democratic`, admin `PATCH /api/rooms/{id}`, integration `democratic` | Sets `democratic` and/or `device_control` (`None` keeps a value). On a change, sends `democratic_changed {enabled, device_control, by}` to every member and bumps the change signal. Returns whether anything changed. |

A room may be **hostless** (`host_id` empty) only while it is empty: an
admin-created group nobody joined yet, or a chat-created room whose members
all left. Host-only messages are ignored then (no client id is empty), and
the first member to arrive becomes host. Admin groups nobody joins are
removed after `ADMIN_EMPTY_GROUP_TTL_SECS` (default 600, checked every
30 s by `tasks::spawn_empty_group_reaper`); empty chat rooms are closed by
the integration's empty-room timeout.

### `handle_leave` (`room/leave.rs`)

Takes a client out of its room (on the already-locked maps):

1. Removes it from `clients`, `ready_clients` and `client_status`; if it was
   host, drops `pending_play`.
2. Room now empty: a chat room stays open, hostless (`empty_since` set);
   any other room is removed.
3. It was host: `promote_new_host` picks the earliest-joined remaining
   `ClientKind::Web` member (a server-side device bridge only if nobody else
   is left) and sends `host_changed` and `participants`. The room stays open.
4. Otherwise: `client_left {participant_count}` and `participants`.

`handle_disconnect` runs `handle_leave`, removes the client entry and
broadcasts `room_list`.

### `schedule_disconnect` (`room/reconnect.rs`)

Called when a connection ends (or the zombie reaper flags it). If the entry
still has that `conn_id`, it waits `RECONNECT_GRACE_SECS` (90 s), then calls
`handle_disconnect` only if `should_evict` holds:

```rust
pub(crate) fn should_evict(client: &Client, conn_id: u64, now: u64) -> bool {
    client.conn_id == conn_id
        && (!client.connected || now.saturating_sub(client.last_seen) > STALE_AFTER_MS)
}
```

(`STALE_AFTER_MS` = 60 000.) A client that reconnected in time has a new
`conn_id`, so the old timer leaves it alone; it was already brought up to
date by `resend_room_state`, and the other members saw no change.

## Module: `tasks.rs`

| Task | Behavior |
|------|----------|
| `spawn_zombie_cleanup` | Every 30 s, finds connected `Web` clients silent for more than 60 s and calls `schedule_disconnect` for them (removed after the 90 s grace period unless they speak again or reconnect). Device bridges are skipped. |
| `spawn_empty_group_reaper` | Every 30 s, removes admin groups still empty `ADMIN_EMPTY_GROUP_TTL_SECS` after creation; broadcasts `room_list` |
| `setup_shutdown_signal` / `wait_for_shutdown` | SIGTERM/SIGINT (Ctrl+C elsewhere) → every listener shuts down gracefully |

## Module: `jellyfin/`

Lets the admin panel (and chat integrations) put Jellyfin sessions that
can't run the web client (TV apps, Fladder, ...) into rooms. Only started
with the admin panel and `JELLYFIN_URL` + `JELLYFIN_API_KEY` (or
`JELLYFIN_API_KEY_FILE`).

- A bridged device is an ordinary client entry with
  `kind: ClientKind::Bridge`, added through `ops::add_member`. Its
  outbound channel is read by a task in `bridge.rs` instead of a socket.
  Bridge entries are skipped by the zombie reaper and can never be
  reattached to over `/ws`.
- **Host or receiver is not stored**: on every tick the task checks
  whether it is `room.host_id`. As host it turns the device's state into
  `set_media` / `player_event` / `state_update`; as receiver it sends the
  device `PlayNow` / `Pause` / `Unpause` / `Seek`. Both go through
  `ws::dispatch_internal`, i.e. the same handlers (host checks, ready gate,
  scheduling) as websocket traffic, including `ready` and `client_status`.
- One poller reads `GET /Sessions` every `BRIDGE_POLL_INTERVAL_MS` (default
  1000, min 250), only while a bridge exists or the panel looked within
  30 s, and publishes a snapshot on a `watch` channel that wakes every
  bridge task.
- Positions: Jellyfin only updates `PlayState.PositionTicks` when the
  device reports progress, so `logic::device_view` extrapolates from
  `LastPlaybackCheckIn` (capped at three report intervals, 30-120 s).
  Jellyfin timestamps are shifted by an estimated clock offset
  (`ClockEstimator`: the smallest `fetched_at - check-in` over changed
  check-ins in the last 10 min). After sending a seek or pause, the
  receiver assumes it took effect until a newer check-in arrives, for at
  most 10 s (`FollowerMemory`, `COMMAND_TRUST_MS`), so stale reports don't
  cause repeat seeks.
- Scheduled plays: the task notes each `player_event` play's
  `target_server_ts` and position. Until the room reports a newer state
  it measures the room position from that target, keeps receivers'
  play/pause untouched before it (a `hold`), and wakes up 300 ms before
  the target to unpause them.
- Remote-control commands run in their own task, so the bridge keeps
  reading room messages while Jellyfin answers.
- Receivers: a device that was on the room's item and left it was
  stopped on purpose and is left alone until the room changes item;
  `PlayNow` is tried at most 3 times per item. A receiver session must
  support remote control.
- A bridge that becomes host marks the room started (no start countdown).
- Democratic rooms (`tick` reads `democratic && started`,
  `device_control`, `last_command_ts`/`last_command_by` and
  `last_change_ts` into a `Democratic` snapshot):
  - **Host device follows others** (`logic::host_follow`): when the room's
    last command is newer than the last one seen and came from someone
    else, the host device is sent `Pause`/`Unpause` and, past the 2 s drift
    threshold, `Seek`, if it accepts remote control. Until its next
    check-in (at most 10 s) the host reports nothing, so its old state
    can't undo the command. Commands from before the device became host
    are ignored; a scheduled play is acted on once it starts.
  - **Device control** (`logic::detect_user_action`, receivers only, with
    `device_control` on): each progress report is judged once. It counts
    only if it is more than `USER_ACTION_GRACE_MS` (3 s) after the bridge's
    own last pause/seek/`PlayNow` and the room's `last_change_ts`, the
    device is on the room's item and there is no scheduled play. A
    play/pause state that differs from the room is a play or pause; a jump
    of more than `USER_SEEK_THRESHOLD_SECS` (8 s) both from where the device
    was heading and from the room is a seek. The bridge sends it as its own
    `player_event` (passing `may_control`) and treats it as its own command,
    so `follower_step` doesn't undo it.
- Receiver thresholds: seek beyond 2 s drift (4 s cooldown, 1 s lead while
  playing), pause/unpause 2.5 s cooldown, `PlayNow` 15 s cooldown.
- `/Sessions` is fetched without `activeWithinSeconds` (an idle TV makes
  no requests and would drop out); the admin list filters by
  `LastActivityDate` (16 min) instead, always keeping bridged devices.
  Each session is parsed on its own, so one odd entry can't hide the rest.
- A device missing from `/Sessions` for 90 s leaves its room; a room
  closing stops its bridges. `add` reserves the device under one lock, so
  concurrent adds can't bridge it twice; `remove` detaches in its own
  task, so a dropped HTTP request can't leave an orphan.

### Jellyfin API used (checked against Jellyfin 12, `release-12.z`)

| Call | Jellyfin side | Notes |
|------|---------------|-------|
| `GET /Sessions` | `SessionController.GetSessions` | With an API key, every session is returned (`isApiKey`). Fields read: `Id`, `UserId` (GUID, "N" format), `UserName`, `Client`, `DeviceName`, `DeviceId`, `SupportsRemoteControl`, `NowPlayingItem.{Id,Name,SeriesName}`, `PlayState.{PositionTicks,IsPaused}`, `LastPlaybackCheckIn`, `LastActivityDate` (UTC, ends in `Z`). |
| `POST /Sessions/{id}/Playing/{Pause,Unpause,Seek}?seekPositionTicks=` | `SendPlaystateCommand` | 204 on success. |
| `POST /Sessions/{id}/Playing?playCommand=PlayNow&itemIds=&startPositionTicks=` | `Play` | Single episode + "next episode autoplay" makes Jellyfin queue the rest of the series. |

Auth header: `Authorization: MediaBrowser Client="...", Device="...",
DeviceId="jellywatchparty-session-server", Version="...", Token="<API key>"`.
Jellyfin needs Client/Version/DeviceId to build the calling session; for an
API key it replaces Client with the key's name and treats the caller as
privileged in `SessionManager.AssertCanControl`, so other users' sessions
can be controlled. The server's own session is recognised by its DeviceId
and hidden.

## Module: `messaging.rs`

- `build_room_state_payload(room, participant_count)` - the `room_state`
  payload (create, join, reattach)
- `build_participants` / `build_participants_msg` / `broadcast_participants(room, clients)` - the `participants` list (also bumps `events`)
- `send_room_list(client_id, clients, rooms)` - room list to one client
- `broadcast_room_list(clients, rooms)` - room list to every client (also bumps `events`)
- `send_to_client(client_id, clients, msg)` - one message to one client
- `broadcast_to_room(room, clients, msg, exclude)` - serialize once, send to every member except `exclude`

All sends use `try_send`: if a client's queue (100 messages) is full or
closed, the message is dropped for that client and a warning is logged.

## Module: `auth.rs`

`JwtConfig::from_env()` reads `JWT_SECRET` (empty or unset: auth disabled,
with a startup warning; a secret shorter than 32 characters or with low
estimated entropy also logs a warning), `JWT_AUDIENCE` (default
`JellyWatchParty`) and `JWT_ISSUER` (default `Jellyfin`).

```rust
pub fn validate_token(&self, token: &str) -> Result<Claims, String> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[&self.audience]);
    validation.set_issuer(&[&self.issuer]);
    validation.validate_exp = true;
    validation.leeway = 60; // seconds
    decode::<Claims>(token, &DecodingKey::from_secret(self.secret.as_bytes()), &validation)
    // ...
}
```

`Claims`: `sub` (Jellyfin user id), `name`, `aud`, `iss`, `exp`, `iat`.

## Module: `password.rs`

Room passwords are stored as `(salt, SHA-256(salt || password))` with a
random UUID salt and checked with a constant-time compare (`ct_eq`, also
used for resume secrets). A single fast hash is deliberate: rooms are
in-memory only, and a slow KDF would run under the global locks on every
join attempt; online guessing is bounded by the password throttle.

## Concurrency Model

```
┌──────────────────────────────────────────────────────────────────┐
│                        Tokio Runtime                             │
├──────────────────────────────────────────────────────────────────┤
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐      │
│  │  Task: Client1 │  │  Task: Client2 │  │  Task: Bridge  │      │
│  │  WebSocket     │  │  WebSocket     │  │  (Jellyfin)    │      │
│  └───────┬────────┘  └───────┬────────┘  └───────┬────────┘      │
│          │                   │                   │               │
│          └───────────────────┼───────────────────┘               │
│                              │                                   │
│                              ▼                                   │
│                   Arc<RwLock<Rooms>>                             │
│                   Arc<RwLock<Clients>>                           │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

1. **RwLock** (tokio): many readers, one writer
2. **Lock order**: handlers that need both maps always take `rooms` before
   `clients` (including leave/disconnect and every admin action), so they
   can't deadlock
3. **Serialize once**: a broadcast is serialized to one `OutboundMessage`
   and cloned per recipient
4. **Bounded channels**: each client has a bounded queue; a slow client
   loses messages instead of growing memory

## Reconnect and Room Lifecycle {#reconnect-and-room-lifecycle}

### Persistent Client ID

The web client generates a UUID once per browser tab, stores it in
`sessionStorage` (`getPersistentClientId()` / `withClientId()` in
`src/clients/jellyfin-web/ws/connection.js`) and sends it as
`?client_id=<uuid>` on every WebSocket connection attempt (see
[Protocol]({{ '/technical/protocol/' | relative_url }}#client_id-and-resume-query-parameters)).
If the server has an entry for that id (live, or in its grace period), the
new connection takes it over and keeps its room and host role
(`ws/connection.rs`) instead of registering a new client. A client-supplied
id is only used if it is UUID-shaped (36 hex digits and hyphens); anything
else gets a freshly minted id.

Client ids are visible to everyone in a room, so reattaching also needs
the entry's **resume secret**: a random 64-hex-char value created with the
entry and sent only to its owner in `client_hello`. The client stores it
next to its id and sends it as `&resume=`. Without the right secret
(`decide_attach`, constant-time compare), a connection asking for an id
that is in use gets a fresh id instead of taking over the entry's room
membership and host role. The secret is replaced on every reattach.

Each websocket connection also gets a `conn_id`. When a connection ends,
it marks the entry disconnected and schedules the grace-period check only
if it is still the entry's current connection; after the grace period the
entry is removed only if that same connection is still attached and
either ended or stayed silent for 60 s (`reconnect::should_evict`). A
half-dead old socket, noticed long after the client reconnected, therefore
can't evict the live session.

### Reconnection Behavior

| Scenario | Behavior |
|----------|----------|
| Client reconnects within 90 s with its id and resume secret | Reattaches to the same entry; if it was in a room, `room_state` and `participants` are resent and its host/guest role is kept |
| Host reconnects within 90 s | Room stays as it was; other members see no change |
| Host doesn't reconnect within 90 s, others remain | Earliest-joined remaining member becomes host; `host_changed` to the room; room stays open |
| Last member doesn't reconnect within 90 s | Room is removed (chat-created rooms stay open, hostless) |
| Server restart | All rooms lost (in-memory only); clients reconnect to an empty server |

The web client reconnects with exponential backoff (`RECONNECT_BASE_MS`
1 s, doubling up to `RECONNECT_MAX_MS` 30 s) while `autoReconnect` is on.

### Room Capacity and Scaling

Limits: 20 members per room (`Room is full` beyond that), all state
in-memory (rooms are ephemeral by design), one server instance (see
[Features: Known Limitations]({{ '/features/' | relative_url }}#known-limitations)
for why not to run more than one). The server only relays small JSON
messages; load grows with the number of members times their message rate
(each web client sends a `state_update` per second while hosting, plus
pings and status changes).
