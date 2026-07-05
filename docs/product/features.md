---
title: Features
parent: Getting Started
nav_order: 2
---

# Features

## Current Features

### Room Management
- **Create rooms** - Start a watch party with a custom name
- **Room passwords** - Optionally require a password to join a room
- **Join rooms** - Enter a room ID to join an existing session
- **Leave rooms** - Exit cleanly with proper cleanup
- **Room list** - See all active rooms on the server
- **Participant count** - Track how many people are watching
- **Automatic host transfer** - If the host leaves with others still in the room, the earliest-joined remaining participant is promoted to host instead of the room closing

### Playback Synchronization
- **Play/Pause sync** - Host controls playback state for all clients
- **Democratic mode** - Host-toggleable setting letting all participants control playback, not just the host
- **Seek sync** - Jumping to a position syncs everyone
- **Position sync** - Continuous updates keep clients aligned
- **Drift correction** - Automatic playback speed adjustment (0.85x-2.0x), using hysteresis so it only kicks in once drift exceeds 0.3s and stays quiet until it falls back under 0.1s (see [Sync Algorithms](../technical/sync.md))
- **HLS support** - Works with Jellyfin's adaptive streaming

### User Interface
- **OSD button** - Watch Party button in the video player controls
- **Slide-out panel** - Room list and controls
- **Home section** - Watch parties shown on Jellyfin homepage
- **System notifications** - Centered toasts for play/pause, join/leave events
- **Chat notifications** - Stacking toasts for incoming messages (top-right)
- **Sync indicator** - Visual status showing sync state (synced/syncing/waiting)
- **Connection status** - Online/offline indicator

### Chat
- **Text chat** - Real-time messaging within watch party rooms
- **Message history** - Last 50 messages replayed to clients joining or reattaching to a room
- **Username display** - Shows sender's Jellyfin username
- **Timestamps** - Message timestamps for context
- **Unread badge** - Notification when new messages arrive
- **XSS protection** - Messages are escaped to prevent injection

### Networking
- **WebSocket communication** - Low-latency real-time sync
- **Auto-reconnect** - Automatic reconnection on disconnect
- **Clock synchronization** - NTP-like time sync between clients
- **Rate limiting** - Protection against abuse (10 tokens/min)

### Security
- **JWT authentication** - Optional token-based auth
- **Configurable secret** - Admin-controlled JWT signing key
- **CORS protection** - Origin validation (configurable)
- **Message size limits** - 64KB max message size

## Compatibility

### Jellyfin Versions
| Version | Status |
|---------|--------|
| 10.11.x | Supported (current target) |
| 10.9.x - 10.10.x | Not tested |
| 10.8.x and earlier | Not supported |

### Browsers

| Browser | Version | Status | Notes |
|---------|---------|--------|-------|
| Chrome/Chromium | 80+ | Fully supported | Recommended for best experience |
| Firefox | 75+ | Fully supported | |
| Edge | 80+ | Fully supported | Chromium-based versions |
| Safari | 14+ | Supported | See known issues below |
| Safari (iOS) | 14+ | Partial | See mobile limitations |
| Chrome (Android) | 80+ | Partial | See mobile limitations |
| Firefox (Android) | 79+ | Partial | See mobile limitations |
| Jellyfin Desktop | Any (CEF+mpv) | Supported | Uses a native player adapter (see [Client](../technical/client.md#module-utilsvideojs)) instead of an HTML5 `<video>` element |

#### Safari Known Issues

Safari uses its native HLS implementation which behaves differently:

- **Buffering state reporting** - Safari may report incorrect `readyState` during HLS segment loading, causing brief sync hiccups
- **Playback rate limits** - Safari may clamp playback rates more aggressively than other browsers
- **Background tab throttling** - Aggressive throttling can affect sync when tab is not focused

**Workarounds:**
- Keep the Safari tab in focus during watch parties
- If sync issues persist, try leaving and rejoining the room

#### Mobile Browser Limitations

Mobile browsers have reduced functionality due to platform restrictions:

| Feature | Desktop | Mobile |
|---------|---------|--------|
| Background playback | Yes | Limited (OS may pause) |
| Playback rate adjustment | Full range | May be restricted |
| Auto-play | Yes | Requires user interaction |
| Picture-in-picture sync | Yes | Not supported |

**Mobile-specific notes:**
- **iOS Safari** - Auto-play restrictions require tapping play after joining
- **Android Chrome** - Background tabs may be suspended by the OS
- **Data saver modes** - May interfere with WebSocket connections

### Media Types
| Type | Status |
|------|--------|
| Movies | Supported |
| TV Episodes | Supported |
| HLS streams | Supported |
| Direct play | Supported |
| Live TV | Not supported |

## Known Limitations

1. **Single media** - One media item per room (by design)
2. **Ephemeral rooms** - Rooms are closed when the host leaves (with no other participants remaining) and doesn't reconnect within 90 seconds, or when the server restarts (by design); if other participants remain, host duties transfer automatically instead — see Automatic host transfer below
3. **Guests need a browser or Jellyfin Media Player client** - The Watch Party UI (joining, chat, the room list) only exists in the injected web client and Jellyfin Desktop. Hosting is broader: a [Host Bridge](../technical/host-bridge.md) lets any native/TV client (e.g. Fladder on Android TV) act as room host even though it can't run the UI itself — but someone still has to join as a guest from a supported client to actually watch.
4. **Chat history is capped and in-memory** - The last 50 messages are replayed to joining/reattaching clients, but history is lost when a room closes (rooms are ephemeral by design)

## Roadmap

### Planned Features

| Feature | Priority | Status |
|---------|----------|--------|
| Text chat | High | Done |
| Message history for late joiners | Medium | Done |
| Democratic mode | Medium | Done |
| Automatic host transfer | Medium | Done |
| Room passwords | Low | Done |

### Feature Descriptions

- **Message history for late joiners** - The last 50 chat messages are replayed to clients joining or reattaching to a room
- **Democratic mode** - Host-toggleable per-room setting that lets all participants control playback, not just the host
- **Automatic host transfer** - When the host leaves (or disconnects past the 90s reconnect grace period) with other participants still in the room, the earliest-joined remaining participant is promoted to host instead of closing the room
- **Room passwords** - Optional password set at room creation; required to join

### Long-term Goals

- **Official Jellyfin plugin repository** - Publish to the [official Jellyfin plugin repository](https://jellyfin.org/docs/general/server/plugins/#official-plugins) for native discoverability and installation

