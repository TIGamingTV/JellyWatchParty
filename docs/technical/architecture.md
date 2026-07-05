---
title: Architecture
parent: Technical
nav_order: 1
---

# Architecture

## System Overview

OpenWatchParty consists of three main components that work together to provide synchronized media playback.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Jellyfin Server                                │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    OpenWatchParty Plugin (C#)                    │    │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────┐   │    │
│  │  │  ClientScript   │  │  Configuration  │  │   JWT Token    │   │    │
│  │  │    Endpoint     │  │      Page       │  │    Endpoint    │   │    │
│  │  └─────────────────┘  └─────────────────┘  └────────────────┘   │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ HTTP (loads JS)
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Browser (Jellyfin Web)                           │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    Web Client (JavaScript)                       │    │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │    │
│  │  │  State  │ │   UI    │ │Playback │ │   WS    │ │  Utils  │   │    │
│  │  │ Module  │ │ Module  │ │ Module  │ │ Module  │ │ Module  │   │    │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘   │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ WebSocket
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      Session Server (Rust)                               │
│  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────────┐       │
│  │  Room Manager   │  │  Client Handler │  │  Message Router    │       │
│  └─────────────────┘  └─────────────────┘  └────────────────────┘       │
└─────────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. Jellyfin Plugin (C#)

The plugin integrates with Jellyfin's plugin system.

**Responsibilities:**
- Serve the client JS loader (`/OpenWatchParty/ClientScript`) and each
  individual module it fetches (`/OpenWatchParty/Client/{path}`)
- Provide configuration UI for JWT settings
- Generate JWT tokens for authenticated users
- Handle HTTP caching with ETag support
- Bridge native (non-browser) Jellyfin sessions in as room hosts — see
  [Host Bridge](host-bridge.md)

**Files:**
- `Plugin.cs` - Plugin entry point, configuration loading
- `OpenWatchPartyController.cs` - REST API endpoints
- `PluginConfiguration.cs` - Configuration model
- `Services/HostBridgeManager.cs`, `Services/SessionHostBridge.cs`,
  `Services/SessionServerAuth.cs` - native-client host bridging (see
  [Host Bridge](host-bridge.md))
- `Web/configPage.html` - Admin configuration page
- `Web/plugin.js` - Loader that dynamically fetches each client module
  from `/OpenWatchParty/Client/{path}` (not a pre-bundled script)

**Note:** the plugin backend itself makes no outbound network calls to
the session server for the normal browser flow — it only ever hands the
browser a token and a URL, and the browser does the talking. The one
exception is the Host Bridge: `HostBridgeManager` opens its own
WebSocket to the session server on behalf of a bridged native session.

### 2. Session Server (Rust)

A lightweight WebSocket server that manages rooms and relays messages.

**Responsibilities:**
- Accept WebSocket connections
- Manage room lifecycle (create, join, leave, close)
- Relay playback events between clients
- Validate host permissions
- Filter state updates (anti-jitter, rate limiting)
- Schedule synchronized actions

**Modules:**
- `main.rs` - Server setup, Warp configuration
- `types.rs` - Data structures (Client, Room, Message)
- `routes.rs` - Warp route filters
- `tasks.rs` - Background tasks (zombie cleanup, shutdown)
- `messaging.rs` - Message sending utilities
- `auth.rs` - JWT validation (optional)
- `utils.rs` - Utilities (timestamp)
- `ws/` - WebSocket handling (connection, dispatch, constants, validation, pending_play, handlers/)
- `room/` - Room lifecycle (leave, close)

### 3. Web Client (JavaScript)

Modular JavaScript injected into Jellyfin's web interface.

**Responsibilities:**
- Inject UI elements (button, panel, home section)
- Manage WebSocket connection to session server
- Intercept video playback events
- Apply synchronized playback commands
- Correct drift with playback rate adjustment
- Synchronize clocks with server

**Modules:**
- `plugin.js` - Loader, script initialization
- `state.js` - Global state and constants
- `utils/` - Utility functions (log, media, misc, time, video)
- `ui/` - User interface (cards, home, indicators, render, styles, toasts)
- `playback/` - Video binding and sync (bind, play, sync)
- `chat/` - Text chat (input, messages)
- `ws/` - WebSocket communication (auth, connection, send, handlers/)
- `app/` - Initialization and cleanup (cleanup, lifecycle)

## Data Flow

### Joining a Room

```
Browser                          Server                      Host Browser
   │                                │                              │
   ├── WebSocket connect ──────────►│                              │
   │◄─── client_hello ──────────────┤                              │
   │◄─── room_list ─────────────────┤                              │
   │                                │                              │
   ├── join_room ──────────────────►│                              │
   │◄─── room_state ────────────────┤                              │
   │                                ├── participants_update ──────►│
   │                                │                              │
   ├── ready ──────────────────────►│                              │
   │                                │                              │
```

### Synchronized Playback

```
Host Browser                     Server                   Client Browser
     │                              │                            │
     ├── player_event (play) ──────►│                            │
     │                              │                            │
     │                         [Validate host]                   │
     │                         [Calculate target_ts]             │
     │                              │                            │
     │◄─── player_event ────────────┼─── player_event ──────────►│
     │     target_ts = T+1000       │    target_ts = T+1000      │
     │                              │                            │
     │         [Wait for T]         │            [Wait for T]    │
     │                              │                            │
     │      video.play()            │              video.play()  │
     │                              │                            │
```

### Leaving a Room (Normal Disconnect)

```
Participant                      Server                       Host
     │                              │                           │
     ├── leave_room ───────────────►│                           │
     │                              │                           │
     │                         [Remove from room]               │
     │                         [Update room state]              │
     │                              │                           │
     │                              ├── participants_update ───►│
     │                              │   (count decreased)       │
     │                              │                           │
     │◄─── room_list ───────────────┤                           │
     │                              │                           │
  [Back to lobby]                   │                           │
```

### Host Disconnect (Grace Period, Then Closure)

A dropped connection (Wi-Fi blip, tab throttling, app backgrounding)
doesn't close the room right away — the server holds the host's slot
open for a 90-second grace period before tearing anything down:

```
Host                            Server                    Participants
  │                                │                            │
  X (disconnect)                   │                            │
  │                          [schedule_disconnect]               │
  │                          [90s grace period starts]           │
  │                                │                            │
  │   (reconnects within 90s)      │                            │
  ├── WS connect ?client_id=... ──►│                            │
  │                          [same client_id: reattach]         │
  │◄── room_state (resent) ────────┤                            │
  │   [host role, playback state restored, nothing broadcast    │
  │    to participants — they never saw a disruption]           │
  │                                │                            │
  ────────────────────────────────────────────────────────────────
           OR, if the host never reconnects within 90s:
  │                          [grace period expires]              │
  │                          [tear down room]                    │
  │                                │                            │
  │                                ├── room_closed ────────────►│
  │                                ├── room_list ──────────────►│
  │                                │   (room removed)           │
  │                                │                    [Show notification]
  │                                │                    [Return to lobby]
```

## Technology Stack

| Component | Technology |
|-----------|------------|
| Plugin | C# (.NET 9.0), ASP.NET Core |
| Session Server | Rust, Warp, Tokio |
| Web Client | JavaScript (IIFE pattern) |
| Communication | WebSocket, JSON |
| Authentication | JWT (optional) |
| Containerization | Docker, Docker Compose |

## State Management

### Server State

```
Clients: HashMap<ClientId, Client>
Rooms: HashMap<RoomId, Room>

Client {
  sender: Sender<Message>       // bounded channel
  room_id: Option<RoomId>
  user_id: Option<String>
  user_name: Option<String>
  authenticated: bool
  message_count: u32
  last_reset: u64
  last_seen: u64
}

Room {
  room_id: String
  name: String
  host_id: ClientId
  clients: Vec<ClientId>
  ready_clients: HashSet<ClientId>
  pending_play: Option<PendingPlay>
  state: PlaybackState
  last_state_ts: u64
  last_command_ts: u64
}
```

### Client State

```javascript
OWP.state = {
  ws: WebSocket,
  roomId: string,
  clientId: string,
  isHost: boolean,
  serverOffsetMs: number,
  lastSyncPosition: number,
  lastSyncServerTs: number,
  isSyncing: boolean,
  isBuffering: boolean,
  // ... and more
}
```

## Security Model

- **Authentication**: Optional JWT tokens validated by session server
- **Authorization**: Only hosts can send playback commands
- **Transport**: WebSocket (ws://) or secure WebSocket (wss://)
- **Rate limiting**: 10 tokens per minute per user
- **Message size**: 64KB maximum

See [Security Guide](../operations/security.md) for detailed security configuration.

## Operational Limits

### Resource Constraints

| Resource | Limit | Configurable |
|----------|-------|--------------|
| Clients per room | 20 | Server constant `MAX_CLIENTS_PER_ROOM` |
| Rooms per user | 3 | Server constant `MAX_ROOMS_PER_USER` |
| Messages per second | 30 | Server constant `RATE_LIMIT_MESSAGES` |
| Message size | 64 KB | Server constant |
| Token requests | 10/min per user | Plugin constant |

### Performance Characteristics

| Metric | Typical Value | Notes |
|--------|---------------|-------|
| Sync accuracy | ±50ms | Under normal network conditions |
| Clock sync precision | ±20ms | After EMA smoothing stabilizes |
| Drift correction range | 0.85x - 2.0x | Playback rate adjustment |
| State update interval | 1000ms | From host to server |
| Sync loop interval | 500ms | Client-side drift check |

## Edge Cases and Behavior

### Multiple Clients Joining Rapidly

When several clients join a room in quick succession:

1. **Server handling**: Each join is processed sequentially with room lock
2. **Participant updates**: Batched within 100ms to avoid message flood
3. **Ready mechanism**: Host's play command waits up to 2s for all clients to be ready
4. **Mitigation**: If clients aren't ready within timeout, play proceeds anyway

**Recommendation**: Allow 2-3 seconds between mass joins for optimal sync.

### Host Network Disconnect

When the host loses connection:

```
Host disconnects (WebSocket close/error, or zombie-reaped)
       │
       ▼
Server schedules a 90s grace period (does NOT close the room yet)
       │
       ├── Host reconnects with the same persistent client_id ──► Reattached,
       │   within 90s (see "Persistent Client ID" below)            room_state
       │                                                            resent, no
       │                                                            disruption
       │                                                            visible to
       │                                                            participants
       │
       ▼ (90s elapses, no reconnect)
Room is closed
       │
       ▼
All participants receive "room_closed" message
       │
       ▼
Clients show "Room closed" notification
Playback continues locally (not synced)
```

**Notes**:
- A brief disconnect (network blip, tab throttling, background app) is
  invisible to participants as long as the host reconnects within 90
  seconds (`RECONNECT_GRACE_SECS` in `src/server/src/room/reconnect.rs`)
- Only after the grace period expires without a reconnect does the room
  actually close
- Participants can create a new room to continue if the host doesn't
  come back
- Automatic host transfer (to a different user, not just the same host
  reconnecting) is planned (see roadmap)

### Persistent Client ID

Reconnection is matched by identity, not by luck: the client generates
a UUID once and stores it in `localStorage`
(`getPersistentClientId()`/`withClientId()` in
`src/clients/jellyfin-web/ws/connection.js`), then sends it as
`?client_id=<uuid>` on every WebSocket connection attempt
(`src/server/src/routes.rs`). If the server sees a connection with a
`client_id` that still has a live (or grace-period) entry, it swaps in
the new transport and keeps the existing room/host state
(`src/server/src/ws/connection.rs`) instead of registering a new client.
A client-supplied ID is only trusted if it looks like a real UUIDv4 —
anything else falls back to a freshly minted server-side ID.

### Clock Skew Tolerance

The system tolerates significant clock differences between clients:

| Skew Level | Behavior |
|------------|----------|
| < 100ms | Ideal - no noticeable drift |
| 100ms - 500ms | Good - corrected by playback rate adjustment |
| 500ms - 2000ms | Acceptable - noticeable catch-up but functional |
| > 2000ms | Poor - may trigger hard seek, visible jumps |

**Clock sync mechanism**:
- NTP-like ping/pong every 10 seconds
- EMA smoothing (α=0.4) prevents sudden jumps
- Initial sync uses first measurement directly
- Offset stored in `serverOffsetMs` state

### Buffering and HLS Edge Cases

HLS streaming introduces unique challenges:

| Scenario | Behavior |
|----------|----------|
| Segment loading | `isBuffering=true`, sync paused |
| Seek during buffer | Queued until ready |
| False pause (HLS artifact) | Filtered by buffering check |
| Backward position jump | Ignored if < 2s (HLS noise) |

**Protection mechanisms**:
- `isSyncing` lock (2s) prevents feedback loops
- `readyState >= 3` required before sending updates
- Server-side cooldown (2s) after commands

### Room Capacity and Scaling

**Design limits**:
- 20 clients per room (comfortable for watch parties)
- All state in-memory (rooms are ephemeral by design)
- Single server instance (sufficient for typical use)

**At capacity**:
```
Client attempts join
       │
       ▼
Server checks room.clients.len() >= 20
       │
       ▼
Returns error: "Room is full"
Client shows error message
```

**Performance at scale**:

| Rooms | Clients/Room | Total Clients | Expected Behavior |
|-------|--------------|---------------|-------------------|
| 10 | 5 | 50 | Excellent |
| 50 | 10 | 500 | Good |
| 100 | 15 | 1500 | Acceptable (monitor memory) |
| 200+ | 20 | 4000+ | May need resource limits |

**Bottlenecks**:
1. Memory: ~2KB per client, ~5KB per room
2. CPU: Minimal (message relay, no heavy computation)
3. Network: Proportional to message rate × clients

### Reconnection Behavior

When a client disconnects and reconnects, using the same persistent
`client_id` (see "Persistent Client ID" above):

| Scenario | Behavior |
|----------|----------|
| Any client reconnects within 90s | Reattaches to the same client entry; if they were in a room, `room_state` is resent and their host/guest role is restored |
| Host reconnects within 90s | Room stays open the whole time; participants see no disruption |
| Host does not reconnect within 90s | Room is torn down, `room_closed` broadcast to remaining participants |
| Server restart | All rooms lost (in-memory only); clients reconnect to an empty server |

**Auto-reconnect**:
- Client retries with exponential backoff (`RECONNECT_BASE_MS` up to
  `RECONNECT_MAX_MS`), not a fixed interval
- Maintains `autoReconnect=true` state
- Shows "Reconnecting..." in UI
