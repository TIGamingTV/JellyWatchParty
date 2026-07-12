---
title: Features
nav_order: 3
---

# Features

## What is JellyWatchParty?

JellyWatchParty is a plugin for [Jellyfin](https://jellyfin.org/) that enables
synchronized media playback across multiple clients. It allows users to watch
movies, TV shows, and other media together in real-time, regardless of their
physical location.

## The Problem

Watching media together remotely is challenging:
- Video players drift out of sync over time
- Pausing, seeking, and resuming must be coordinated manually
- Network latency makes coordination difficult
- Different streaming qualities cause timing differences

## The Solution

- **Real-time synchronization** - All participants see the same content at the same time
- **Host-controlled playback** - One person controls play/pause/seek for everyone
- **Automatic drift correction** - Playback speed adjusts to keep clients in sync
- **HLS/transcoding support** - Works with Jellyfin's adaptive streaming

## How It Works

1. **Host creates a room** - Starts a watch party from the Jellyfin player
2. **Guests join** - Enter the room ID to join the session
3. **Synchronized playback** - Everyone sees the same frame at the same time
4. **Continuous sync** - Background algorithms keep everyone aligned

See [Core Structure](core-structure) for how the three components (plugin,
session server, web client) work together to make this happen.

## Comparison with Alternatives

| Feature | JellyWatchParty | SyncPlay | Teleparty |
|---------|---------------|----------|-----------|
| Self-hosted | Yes | Yes | No |
| Jellyfin native | Yes | Yes | No |
| Lightweight | Yes | Moderate | Heavy |
| Browser-based | Yes | No | Yes |
| Open source | Yes | Yes | No |

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
- **Seek sync** - Jumping to a position syncs everyone
- **Position sync** - Continuous updates keep clients aligned
- **Drift correction** - Automatic playback speed adjustment (0.85x-2.0x), using hysteresis so it only kicks in once drift exceeds 0.3s and stays quiet until it falls back under 0.1s (see [Sync Algorithms](technical/sync))
- **HLS support** - Works with Jellyfin's adaptive streaming
- **Per-user audio/subtitle tracks** - Each participant picks their own audio and subtitle track using Jellyfin's normal player controls; only play/pause/seek/position are synced, so track choice never affects (or is affected by) anyone else in the room

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

See [Security](security) for the full security model.

### Native Client Bridge

Any logged-in user with browser access to the server can bridge a currently
playing native/TV session (e.g. the official Android TV app or Fladder, which
can't run the injected UI) into a room — in either role — see
[Host Bridge](technical/host-bridge):

- **Host**: bring the native session in as the room's host. Guests still join
  normally; nothing changes on their end.
- **Receiver**: while you're in a room, attach a native session that's already
  playing the same item so it follows the room's play/pause/seek. (Rooms
  without a password only.)

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
| Jellyfin Desktop | Any (CEF+mpv) | Supported | Uses a native player adapter (see [Client](technical/client#module-utilsvideojs)) instead of an HTML5 `<video>` element |

#### Safari Known Issues

Safari uses its native HLS implementation, which behaves differently:

- **Buffering state reporting** - Safari may report incorrect `readyState` during HLS segment loading, causing brief sync hiccups
- **Playback rate limits** - Safari may clamp playback rates more aggressively than other browsers
- **Background tab throttling** - Aggressive throttling can affect sync when tab is not focused

**Workarounds:** keep the Safari tab in focus during watch parties; if sync issues persist, try leaving and rejoining the room.

#### Mobile Browser Limitations

| Feature | Desktop | Mobile |
|---------|---------|--------|
| Background playback | Yes | Limited (OS may pause) |
| Playback rate adjustment | Full range | May be restricted |
| Auto-play | Yes | Requires user interaction |
| Picture-in-picture sync | Yes | Not supported |

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

1. **Host-only control** - Only the host can control playback (democratic mode planned)
2. **Single media** - One media item per room (by design)
3. **Ephemeral rooms** - Rooms are closed when the host leaves (with no other participants remaining) and doesn't reconnect within 90 seconds, or when the server restarts (by design); if other participants remain, host duties transfer automatically instead — see Automatic host transfer above
4. **Guests need a browser or Jellyfin Media Player client** - The Watch Party UI (joining, chat, the room list) only exists in the injected web client and Jellyfin Desktop. Native/TV clients are broader via the [Native Client Bridge](technical/host-bridge): any such client can be bridged in as a room **host**, or attached to a room as a **receiver** that follows playback — but neither role gives it the interactive UI (chat, room list), and someone still drives the session from a supported client.
5. **Chat history is capped and in-memory** - The last 50 messages are replayed to joining/reattaching clients, but history is lost when a room closes (rooms are ephemeral by design)

## Roadmap

| Feature | Priority | Status |
|---------|----------|--------|
| Text chat | High | Done |
| Message history for late joiners | Medium | Done |
| Democratic mode | Medium | Planned |
| Automatic host transfer | Medium | Done |
| Room passwords | Low | Done |

- **Democratic mode** - Allow all participants to control playback, not just the host
- **Official Jellyfin plugin repository** - Publish to the [official Jellyfin plugin repository](https://jellyfin.org/docs/general/server/plugins/#official-plugins) for native discoverability and installation

## Next Steps

- [Installation](installation) - Set up on your server
- [User Guide](user-guide) - How to use JellyWatchParty
- [Core Structure](core-structure) - How it's built
