---
title: Core Structure
nav_order: 4
---

# Core Structure

This page explains how JellyWatchParty is built at a system level. For
implementation-level detail, see the [Technical Reference](technical)
section (Protocol, Server, Client, Plugin, Sync Algorithms, Host Bridge).

## System Overview

JellyWatchParty consists of three main components that work together to
provide synchronized media playback.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Jellyfin Server                                │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    JellyWatchParty Plugin (C#)                    │    │
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

Integrates with Jellyfin's plugin system.

**Responsibilities:**
- Serve the client JS loader (`/JellyWatchParty/ClientScript`) and each
  individual module it fetches (`/JellyWatchParty/Client/{path}`)
- Provide configuration UI for JWT settings
- Generate JWT tokens for authenticated users
- Bridge native (non-browser) Jellyfin sessions in as room hosts — see
  [Host Bridge](technical/host-bridge)

For the normal browser flow, the plugin backend makes **no** outbound
network calls to the session server — it only ever hands the browser a
token and a URL, and the browser does the talking. The one exception is
Host Bridge, which opens its own WebSocket to the session server on behalf
of a bridged native session. Full detail: [Plugin](technical/plugin).

### 2. Session Server (Rust)

A lightweight WebSocket server that manages rooms and relays messages.

**Responsibilities:**
- Accept WebSocket connections and manage room lifecycle (create, join, leave, close)
- Relay playback events between clients, validating host permissions
- Filter state updates (anti-jitter, rate limiting) and schedule synchronized actions

All state is in-memory (`Arc<RwLock<HashMap<...>>>`) — no database, and
everything is lost on restart, since rooms are ephemeral by design. Full
detail: [Server](technical/server).

### 3. Web Client (JavaScript)

Modular JavaScript injected into Jellyfin's web interface.

**Responsibilities:**
- Inject UI elements (button, panel, home section)
- Manage the WebSocket connection to the session server
- Intercept video playback events and apply synchronized playback commands
- Correct drift with playback rate adjustment, and synchronize clocks with the server

Full detail: [Client](technical/client).

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
     │      video.play()            │              video.play()  │
```

### Host Disconnect (Grace Period, Then Closure)

A dropped connection (Wi-Fi blip, tab throttling, app backgrounding)
doesn't close the room right away — the server holds the host's slot open
for a 90-second grace period before tearing anything down:

```
Host                            Server                    Participants
  │                                │                            │
  X (disconnect)                   │                            │
  │                          [90s grace period starts]           │
  │                                │                            │
  │   (reconnects within 90s)      │                            │
  ├── WS connect ?client_id=... ──►│                            │
  │                          [same client_id: reattach]         │
  │◄── room_state (resent) ────────┤                            │
  │   [nothing broadcast to participants — no visible disruption]│
  ────────────────────────────────────────────────────────────────
     OR, if the host never reconnects and others remain:
  │                          [promote earliest-joined participant]│
  │                                ├── host_changed ────────────►│
  │                                │   (room stays open)         │
  ────────────────────────────────────────────────────────────────
        OR, if the host never reconnects and no one else is left:
  │                                ├── room_closed ────────────►│
  │                                ├── room_list ──────────────►│
```

See [Server: Reconnect and Room Lifecycle](technical/server#reconnect-and-room-lifecycle)
for the full mechanics, including the persistent client ID that makes
reattachment possible.

## Technology Stack

| Component | Technology |
|-----------|------------|
| Plugin | C# (.NET 9.0), ASP.NET Core |
| Session Server | Rust, Warp, Tokio |
| Web Client | JavaScript (IIFE pattern) |
| Communication | WebSocket, JSON |
| Authentication | JWT (optional) |
| Containerization | Docker, Docker Compose |

## State Shape

**Server** (per client and per room, all in-memory):

```
Client { sender, room_id, user_id, user_name, authenticated, message_count, last_reset, last_seen }
Room   { room_id, name, host_id, clients, ready_clients, pending_play, state,
         last_state_ts, last_command_ts, chat_history (capped at 50), password_hash }
```

**Client** (`JWP.state`): `ws`, `roomId`, `clientId`, `isHost`, `serverOffsetMs`,
`lastSyncPosition`, `lastSyncServerTs`, `isSyncing`, `isBuffering`, and more —
see [Client: `state.js`](technical/client#module-statejs) for the full list.

## Security Model

- **Authentication**: Optional JWT tokens validated by session server
- **Authorization**: Only hosts can send playback commands
- **Transport**: WebSocket (ws://) or secure WebSocket (wss://)
- **Rate limiting**: 30 messages/sec per client, 10 token requests/min per user
- **Message size**: 64KB maximum

See [Security](security) for the full security model and threat analysis.

## Operational Limits

| Resource | Limit | Configurable |
|----------|-------|--------------|
| Clients per room | 20 | Server constant `MAX_CLIENTS_PER_ROOM` |
| Hosted rooms per user | 1 | Not configurable — creating a new room closes any room the user already hosts |
| Messages per second | 30 | Server constant `RATE_LIMIT_MESSAGES` |
| Message size | 64 KB | Server constant |
| Token requests | 10/min per user | Plugin constant |

| Metric | Typical Value | Notes |
|--------|---------------|-------|
| Sync accuracy | ±50ms | Under normal network conditions |
| Clock sync precision | ±20ms | After EMA smoothing stabilizes |
| Drift correction range | 0.85x - 2.0x | Playback rate adjustment |
| State update interval | 1000ms | From host to server |
| Sync loop interval | 500ms | Client-side drift check |

For scaling limits and capacity planning, see
[Deployment: Capacity Planning](deployment#capacity-planning). For the
detailed sync algorithms behind drift correction and clock sync, see
[Sync Algorithms](technical/sync).
