---
title: User Guide
nav_order: 5
---

# User Guide

Before using JellyWatchParty, ensure your Jellyfin administrator has
installed the plugin, started the session server, and enabled the client
script — see [Installation](installation) if not.

## Creating a Watch Party

1. **Start playing a video** - Open any movie or TV episode in Jellyfin
2. **Find the Watch Party button** - Look for the group icon in the top header bar (right side)
3. **Click to open the panel** - A slide-out panel appears
4. **Enter a room name** - Give your party a descriptive name
5. **Click "Start Room"** - You are now the host

As the host, you control playback for everyone. When you play, pause, or seek, all participants follow.

### Watching on a TV App

If you're watching on a native/TV client that can't run the Watch Party
UI at all (e.g. the official Android TV app or Fladder), someone with
browser access to the same server can bridge your session into a room —
see [Host Bridge](technical/host-bridge) — in one of two ways:

- **Host**: your TV session becomes the room's host. Guests join normally
  from their own room list; nothing changes on their end.
- **Receiver**: start playing the same item on the TV, then, from a browser
  that's already in the room, add your TV session as a *receiver* — it will
  follow the room's play, pause, and seek. (Works for rooms without a
  password.)

> **Both roles are opt-in.** An administrator must enable them from the
> plugin configuration page (**Client Bridging** section): *Allow
> third-party clients to host* enables the Host role, and *Allow supported
> clients as receivers* enables the Receiver role. Until enabled, the
> corresponding picker does not appear in the Watch Party panel.

## Joining a Watch Party

### From the Player
1. **Open any video** - The same video the host is watching
2. **Click the Watch Party button** - Opens the panel
3. **Find the room** - Rooms appear in the list with participant counts
4. **Click "Join"** - You'll automatically sync to the host's position

### From the Homepage

The Jellyfin homepage displays active watch parties in a dedicated "Watch Parties" section, making it easy to discover and join ongoing sessions.

1. **Go to Jellyfin home** - Active watch parties appear in a dedicated section below your media libraries
2. **Browse party cards** - Each card shows the media cover, room name, participant count, and a play button overlay
3. **Join options:**
   - **Click the card** - Navigates to the video player and joins the room
   - **Click the play button** - Starts playback immediately and auto-joins

**Notes:**
- The Watch Parties section only appears when there are active rooms
- Cards refresh automatically every 5 seconds
- If a room closes while you're viewing the homepage, the card disappears
- You must be logged into Jellyfin to see and join watch parties

## Host Controls

| Action | Effect |
|--------|--------|
| Play | All clients start playing |
| Pause | All clients pause |
| Seek | All clients jump to that position |
| Close panel | Room stays active |
| Leave room | Room closes, all participants disconnected (unless another participant remains — see [Features: Automatic host transfer](features#current-features)) |

## Participant Experience

| What Happens | What You See |
|--------------|--------------|
| Host plays | Video starts automatically |
| Host pauses | Video pauses automatically |
| Host seeks | Video jumps to new position |
| Host loses connection briefly | Nothing visible — the server holds the room open for 90 seconds waiting for the host to reconnect |
| Host leaves (or doesn't reconnect within 90s) | "Room closed" notification, unless another participant is promoted to host |
| Drift detected | Playback speed adjusts (0.85x-2.0x) to catch up, only kicking in once drift passes 0.3s and staying quiet again once it drops back under 0.1s |

## The Panel Interface

### Lobby View (Not in a room)
- **Room list** - Active watch parties with names and participant counts
- **Create room** - Input for room name and "Start Room" button
- **Connection status** - Online/Offline indicator

### In-Room View
- **Room name** and **participants** count
- **Sync indicator** - Shows sync status (participants only)
- **Chat** - Text messaging with other participants
- **RTT** - Round-trip time to server (latency indicator)
- **Leave button** - Exit the watch party

## Using Chat

1. Type your message in the chat input field
2. Press **Enter** or click **Send**
3. Your message appears for all participants

- **Username display** and **timestamps** on every message
- **Unread badge** appears when new messages arrive while the panel is closed
- **Message limit** - 500 characters
- **Message history** - The last 50 messages are replayed to anyone joining or reattaching to a room

## Sync Indicator

| Status | Indicator | Meaning |
|--------|-----------|---------|
| In sync | Green dot | Your playback matches the host |
| Out of sync | Yellow pulsing dot | Catching up via playback speed adjustment |
| Waiting for sync | Spinner | Synchronized play is being scheduled |

The "Out of sync" state is normal for a few seconds after joining or after the host seeks — playback speed automatically adjusts to catch up.

## Notifications

**System notifications (center)** — brief toasts for host resumed/paused playback, participant joined/left, and room closed.

**Chat notifications (top-right)** — when the chat panel is closed, incoming messages appear as stacked toasts (up to 5), showing sender and message; click to dismiss, or they fade after 5 seconds.

## Tips for Best Experience

**For Hosts:** wait for everyone to join before starting; announce pauses via external chat; avoid rapid seeking so clients have time to sync.

**For Participants:** make sure you're watching the same title; use a stable connection; give it a few seconds to sync after joining before judging sync quality.

**Network:** port 3000 (session server default) must be reachable; some firewalls block WebSocket connections; use WSS (secure WebSocket) in production.

## Something Not Working?

See [Troubleshooting & FAQ](troubleshooting) for common issues like the
Watch Party button not appearing, sync problems, or rooms closing
unexpectedly.
