---
title: Core Structure
nav_order: 4
---

# Core Structure

How the parts of JellyWatchParty fit together. For implementation detail,
see the [Technical Reference]({{ '/technical/' | relative_url }}).

## Components

```
                ┌──────────────────────────────────────────────┐
                │ Jellyfin server + JellyWatchParty plugin (C#) │
                │  script injection, /JellyWatchParty/Token,    │
                │  config page, optional panel bridges          │
                └───────────────┬──────────────────────────────┘
          serves the web client │ and a signed token (JWT)
                                ▼
┌───────────────────────────────────────────────┐
│ Browser / Jellyfin Desktop: web client (JS)   │
│  Watch Party panel, player sync, chat         │
└───────────────┬───────────────────────────────┘
                │ WebSocket (port 3000, /ws)
                ▼
┌─────────────────────────────────────────────────────────────┐
│ Session server (Rust)                                       │
│  rooms and sync          :3000                              │
│  admin panel (optional)  :3001 ──REST──> Jellyfin (devices) │
│  chat bot API (optional) :3002 <──────── chat bot container │
└─────────────────────────────────────────────────────────────┘
```

### Jellyfin plugin (C#)

- Adds the client script to Jellyfin's web page (through the File
  Transformation plugin when installed, otherwise by patching
  `index.html`) and serves the script's modules from
  `/JellyWatchParty/ClientScript` and `/JellyWatchParty/Client/...`.
- Issues signed tokens (`/JellyWatchParty/Token`) that identify the Jellyfin
  user to the session server, and tells the client the session server URL
  and which optional features are allowed.
- Provides the configuration page.
- Optionally bridges a user's own Jellyfin session into a room from the
  panel ([Host Bridge]({{ '/technical/host-bridge/' | relative_url }})); only
  then does the plugin itself connect to the session server.

Details: [Plugin]({{ '/technical/plugin/' | relative_url }}).

### Web client (JavaScript)

Runs inside Jellyfin's web interface: adds the header, player and home
screen entries, manages the WebSocket connection, applies the room's
playback commands to the local player, corrects drift, and synchronizes its
clock with the server. Details: [Client]({{ '/technical/client/' | relative_url }}).

### Session server (Rust)

- Accepts WebSocket connections, manages rooms (create, join, leave, host
  handover, passwords, chat) and relays playback commands, enforcing who may
  control playback.
- Schedules plays slightly in the future so everyone starts at the same
  moment, and filters noisy position updates.
- Optionally runs the admin panel, drives Jellyfin devices over Jellyfin's
  REST API, and serves the chat bot API.

All rooms are kept in memory; only chat bot settings are stored on disk
(`DATA_DIR`). Details: [Server]({{ '/technical/server/' | relative_url }}).

### Chat bot (Rust, optional)

A separate container that connects to Discord and/or Telegram and forwards
users' requests to the session server's chat bot API. It holds no state; the
session server decides everything. See
[Integration API]({{ '/technical/integration-api/' | relative_url }}).

## Data Flow

### Joining a room

```
Guest browser                    Session server                Host browser
   ├── WebSocket connect ───────────►│                              │
   │◄── client_hello, room_list ─────┤                              │
   ├── auth (token) ────────────────►│                              │
   ├── join_room ───────────────────►│                              │
   │◄── room_state ──────────────────┼── participants_update ──────►│
   ├── ready (item loaded) ─────────►│                              │
```

### Synchronized play

```
Host browser                     Session server                Guest browser
   ├── player_event (play) ────────►│                               │
   │                          check permission,                     │
   │                          target = now + 1000 ms                │
   │◄── player_event (target) ──────┼── player_event (target) ─────►│
   both start playing at the target time, using their clock offset
```

### Host disconnect

A dropped host keeps their place for 90 seconds and can reattach without
anyone noticing. If they leave instead, the participant who joined earliest
becomes host (`host_changed`); the room ends when nobody is left.

## Technology

| Component | Technology |
|-----------|------------|
| Plugin | C#, .NET 10, Jellyfin 12 plugin API |
| Session server | Rust, Axum, Tokio |
| Chat bot | Rust |
| Web client | Plain JavaScript modules |
| Communication | WebSocket with JSON messages; REST for the admin panel and chat bot API |
| Authentication | JWT (HS256), optional |
| Packaging | Docker images (Alpine), Windows binary, plugin zip |

## Limits

| Resource | Limit |
|----------|-------|
| Members per room | 20 |
| Rooms hosted per user (web client) | 1; creating a new room closes the one you host |
| WebSocket messages per connection | 30 per second |
| WebSocket message size | 64 KB |
| Token requests per user | 30 per minute |
| Host position updates | every 1 s |
| Guest drift check | every 500 ms |

These are constants in the code, not settings. See
[Sync Algorithms]({{ '/technical/sync/' | relative_url }}) for the timing
details.
