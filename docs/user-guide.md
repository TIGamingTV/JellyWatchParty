---
title: User Guide
nav_order: 5
---

# User Guide

Before using JellyWatchParty, ensure your Jellyfin administrator has
installed the plugin, started the session server, and enabled the client
script — see [Installation]({{ '/installation/' | relative_url }}) if not.

## Creating a Watch Party

1. **Start playing a video** - Open any movie or TV episode in Jellyfin
2. **Find the Watch Party button** - Look for the group icon in the top header bar (right side)
3. **Click to open the panel** - A slide-out panel appears
4. **Enter a room name** - Give your party a descriptive name
5. **Click "Start Room"** - You are now the host

As the host, you control playback for everyone. When you play, pause, or seek, all participants follow.
To share control, turn on [democratic mode](#democratic-mode).

### Watching on a TV App

If you're watching on a native/TV client that can't run the Watch Party
UI at all (e.g. the official Android TV app or Fladder), you can put your
device into a room yourself if your server has the
[Discord bot]({{ '/discord-bot/' | relative_url }}): in Discord, run
`/jwp device add` while the app is open and signed in as you. Otherwise, ask
an admin: they can put your device into a room from the session server's
[admin panel]({{ '/admin-panel/' | relative_url }}#jellyfin-devices). Either way, the device joins as host or
receiver, and a receiver TV is started on the room's item automatically.

On servers where the admin allows it, you can also bridge your own
device yourself from a browser signed in to your account — see
[Host Bridge]({{ '/technical/host-bridge/' | relative_url }}) — in one of two ways:

- **Host**: your TV session becomes the room's host. Guests join normally
  from their own room list; nothing changes on their end.
- **Receiver**: start playing the same item on the TV, then, from a browser
  that's already in the room, add your TV session as a *receiver* — it will
  follow the room's play, pause, and seek. (Works for rooms without a
  password.)

> **Both roles are opt-in, for trusted servers.** An administrator must
> turn on *Let users bridge their devices from the Watch Party panel* in
> the plugin configuration page (**Watch Party Panel Bridging** section),
> then *Allow third-party clients to host* for the Host role and *Allow
> supported clients as receivers* for the Receiver role. Until enabled,
> the corresponding picker does not appear in the Watch Party panel. Only
> your own devices are listed.

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
| Play (first time in the room) | Everyone sees "Waiting for everyone to be ready..." until all videos are loaded (10 s at most), then a 3, 2, 1 countdown, and all start together |
| Play | All clients start playing |
| Pause | All clients pause |
| Seek | All clients jump to that position |
| Close panel (X button, Escape, or click outside it) | Room stays active |
| Leave room | Room closes, all participants disconnected (unless another participant remains — see [Features: Automatic host transfer]({{ '/features/' | relative_url }}#current-features)) |

## Participant Experience

| What Happens | What You See |
|--------------|--------------|
| Host plays | Video starts automatically |
| Host pauses | Video pauses automatically |
| Host seeks | Video jumps to new position |
| Host loses connection briefly | Nothing visible — the server holds the room open for 90 seconds waiting for the host to reconnect |
| Host leaves (or doesn't reconnect within 90s) | "Room closed" notification, unless another participant is promoted to host |
| Drift detected | Playback speed adjusts (0.85x-2.0x) to catch up, only kicking in once drift passes 0.3s and staying quiet again once it drops back under 0.1s |
| You pause on your own | The video resumes within about a second ("Only the host controls playback"), unless the room is in democratic mode |

## Democratic Mode

In democratic mode everyone in the room can play, pause and seek, not just
the host. Rooms with it on are marked with an icon in the room list.

**Turning it on (host):** in the Watch Party panel, tick **Everyone can
control playback**. The switch only appears if your administrator turned on
*Allow democratic mode* in the plugin settings (or the room already is
democratic, so you can turn it off). An admin can also turn it on for any
room from the [admin panel]({{ '/admin-panel/' | relative_url }}#democratic-mode), and in a room created from
Discord or Telegram the owner can turn it on from the bot.

**What everyone sees:** a notification such as "Alice turned on democratic
mode: everyone can control playback", and a line in the panel saying
*Everyone can control playback* (with *(TV devices too)* when an admin also
let devices control the room). Pause and resume toasts name who did it,
e.g. "Bob paused playback".

**What you can do as a participant** (Jellyfin in a browser or Jellyfin
Desktop): play, pause and seek with the player's normal controls once the
room has started. Everyone, host included, follows. These stay with the
host:

- the room's first play (the start countdown);
- switching to another item;
- the position everyone follows between commands.

**Chat users:** if the room was created from Discord or Telegram and the
admin turned on the bot's *Democratic mode commands*, everyone who joined
the room in the chat gets play, pause and seek: `/jwp playback` and panel
buttons in [Discord]({{ '/discord-bot/' | relative_url }}#democratic-mode), panel buttons in
[Telegram]({{ '/telegram-bot/' | relative_url }}#democratic-mode).

**TV apps** put into the room as receivers only follow, unless an admin
turned on *Devices control too* for the room: then pausing or seeking on
the TV itself is applied for everyone after a few seconds.

## The Panel Interface

### Lobby View (Not in a room)
- **Room list** - Active watch parties with names and participant counts
- **Create room** - Input for room name and "Start Room" button
- **Connection status** - Online/Offline indicator

### In-Room View
- **Room name** and **participant list**: each person's name (the host has a star) and status: In sync, Catching up, Buffering, Loading, Not watching, or Playing / Paused for the host
- **Sync indicator** - Shows sync status (participants only)
- **Democratic mode** - The host's *Everyone can control playback* switch (when allowed), and for everyone a note while the mode is on
- **Chat** - Text messaging with other participants
- **RTT** - Round-trip time to server (latency indicator)
- **Leave button** - Exit the watch party (for the host: **Close room**)

The panel closes with the **X** in its header, with **Escape**, or by clicking anywhere outside it; the room stays active either way.

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

**System notifications (center)** — brief toasts for resumed/paused playback (naming who did it), democratic mode turned on or off, participant joined/left, and room closed.

**Chat notifications (top-right)** — when the chat panel is closed, incoming messages appear as stacked toasts (up to 5), showing sender and message; click to dismiss, or they fade after 5 seconds.

## Tips for Best Experience

**For Hosts:** wait for everyone to join before starting; announce pauses via external chat; avoid rapid seeking so clients have time to sync.

**For Participants:** make sure you're watching the same title; use a stable connection; give it a few seconds to sync after joining before judging sync quality.

**Network:** port 3000 (session server default) must be reachable; some firewalls block WebSocket connections; use WSS (secure WebSocket) in production.

## Something Not Working?

See [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }}) for common issues like the
Watch Party button not appearing, sync problems, or rooms closing
unexpectedly.
