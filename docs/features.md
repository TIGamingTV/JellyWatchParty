---
title: Features
nav_order: 3
---

# Features

JellyWatchParty lets people on the same Jellyfin server watch a movie or
episode together: one person hosts a room, the others join it, and
everyone's player follows the host's play, pause and seeks, wherever they
are.

## Overview

| Feature | What it does | How to use it |
|---------|--------------|---------------|
| Watch Party panel | A group icon in the Jellyfin header and player opens a panel to create, join and leave rooms | [User Guide]({{ '/user-guide/' | relative_url }}#the-watch-party-panel) |
| Synchronized playback | Guests load the host's item and follow play, pause and seeks; small drift is corrected by adjusting playback speed, large drift with a seek | [User Guide]({{ '/user-guide/' | relative_url }}#watching-together) |
| Start countdown | The room's first play waits (up to 10 s) until everyone has the video loaded, then everyone starts after a 3-2-1 countdown | Automatic |
| Media follows the host | When the host switches to another item, guests switch too | Automatic |
| Room passwords | Optional password when creating a room | [User Guide]({{ '/user-guide/' | relative_url }}#create-a-room) |
| Chat | Text chat inside the room; the last 50 messages are shown to people who join later | [User Guide]({{ '/user-guide/' | relative_url }}#chat) |
| Participant status | Who is in the room, who hosts, and whether each person is in sync, catching up, buffering or not watching | Room view of the panel |
| Home screen row | A **Watch Parties** row on the Jellyfin home screen lists active rooms | [User Guide]({{ '/user-guide/' | relative_url }}#join-a-room) |
| Automatic host handover | If the host leaves, the person who joined earliest becomes host instead of the room closing | Automatic |
| Reconnect | A dropped connection keeps its place in the room for 90 seconds | Automatic |
| Democratic mode | Optionally, everyone in a room can play, pause and seek | [Democratic mode](#democratic-mode) |
| Own audio and subtitles | Each person picks their own audio and subtitle tracks; only playback position and state are shared | Jellyfin's normal player controls |
| Admin panel | Web UI for admins: see all rooms, create password-protected groups, move people, change hosts, close rooms | [Admin Panel]({{ '/admin-panel/' | relative_url }}) |
| Jellyfin devices | Admins put TV apps and other clients without the panel into rooms, as host or receiver | [Admin Panel: Jellyfin devices]({{ '/admin-panel/' | relative_url }}#jellyfin-devices) |
| Discord and Telegram bots | Linked users create rooms and add their own devices from a chat, without an admin | [Discord Bot]({{ '/discord-bot/' | relative_url }}), [Telegram Bot]({{ '/telegram-bot/' | relative_url }}) |
| Panel bridging (opt-in) | On trusted servers, users bridge their own playing devices into rooms from the panel | [Panel bridging](#panel-bridging) |
| Hide SyncPlay | Optionally hides Jellyfin's built-in SyncPlay button | [Plugin settings]({{ '/configuration/' | relative_url }}#plugin-settings) |
| Authentication | The plugin signs users in to the session server with their Jellyfin identity (JWT) | [Configuration]({{ '/configuration/' | relative_url }}) |

## Supported Clients

The Watch Party panel is part of Jellyfin's web interface. Apps that don't
use it can still take part as **Jellyfin devices**, controlled by the
session server through Jellyfin's remote control.

| Client | Watch Party panel | As host | As receiver |
|--------|-------------------|---------|-------------|
| Jellyfin in a browser, Jellyfin Desktop / Media Player | Yes | Yes | Yes |
| Jellyfin Android / iOS app (web-based) | Yes; with its native player turned on, use it as a Jellyfin device instead | Yes | Yes |
| Official Jellyfin Android TV app | No: add it as a Jellyfin device | Yes | Yes (started and kept in sync by remote control) |
| Fladder | No: add it as a Jellyfin device | Yes | No (doesn't accept remote control) |
| Other apps (Swiftfin, Infuse, Kodi, ...) | No: add it as a Jellyfin device | Yes | Only if the app accepts remote control |

Devices are added by an admin in the admin panel, by the users themselves
through a chat bot, or (if enabled) from the panel. Receivers stay within
about 2 seconds of the host; browsers are kept closer.

**Jellyfin versions:** current releases support Jellyfin 12.x only. For
10.11.x, use an older release (`targetAbi` `10.11.11.0`).

## Democratic Mode

By default only the host controls playback. In a democratic room, other
participants can play, pause and seek for everyone too.

- **Who can control:** the host, plus participants using the Watch Party
  panel (Jellyfin Web, Jellyfin Desktop), plus chat bot users in the room
  when the bot's **Democratic mode commands** setting is on. Jellyfin
  devices only when an admin also turns on **Devices control too** for the
  room.
- **What stays with the host:** the room's first play (the countdown),
  switching to another item, and the position everyone follows between
  commands. Others can control playback once the room has started.
- **Who turns it on:**
  - the host, with **Everyone can control playback** in the panel, if the
    plugin setting **Allow democratic mode** is on;
  - an admin, for any room, in the [admin panel]({{ '/admin-panel/' | relative_url }}#democratic-mode);
  - a chat room's owner or an admin, from the bot, if **Democratic mode
    commands** is on.
- **Device control:** with **Devices control too**, pausing, resuming or
  seeking (more than about 8 seconds) on a receiver TV app is applied to
  the room. It reacts only as fast as the device reports to Jellyfin (often
  several seconds), and a device that pauses to buffer can pause the room.

Everyone in the room is notified when the mode changes, and pause and resume
notifications name who did it. Democratic rooms are marked in the room list.

## Panel Bridging

Off by default, for small servers with trusted users. An admin enables
**Let users bridge their devices from the Watch Party panel** plus one or
both roles in the [plugin settings]({{ '/configuration/' | relative_url }}#plugin-settings).
Users can only bridge their own sessions (Jellyfin administrators: any).

- **Host:** in the lobby, under **Host From Another Device**, pick one of
  your devices that is playing something. It becomes the host of a new room
  and the others follow it.
- **Receiver:** while in a room, under **Add a Device to This Room**, pick a
  device that is playing the room's item. It follows the room's play, pause
  and seeks. Not available for password-protected rooms.

Admins don't need this to put devices into rooms; the admin panel can do it
for any room. Technical details: [Host Bridge]({{ '/technical/host-bridge/' | relative_url }}).

## Limits

| What | Limit |
|------|-------|
| People and devices per room | 20 |
| Rooms you host at a time (Watch Party panel) | 1: creating a new room closes the one you host |
| Chat message | 500 characters |
| Chat history shown to newcomers | 50 messages |
| Wrong room passwords | 5 per room per minute, then a short lockout |
| Messages per connection | 30 per second |

## Known Limitations

- **Rooms are temporary.** They exist in the session server's memory: a
  restart ends them. A room closes when its last person leaves; rooms
  created from a chat bot stay open while empty until their timeout.
- **One instance only.** Run a single session server per Jellyfin
  installation; several instances don't share rooms. One session server can
  serve several Jellyfin addresses (list them all in `ALLOWED_ORIGINS`).
- **Everyone needs access to the item.** Guests play the host's item from
  the same Jellyfin server, so they need permission for that library.
- **Devices are less precise.** Jellyfin devices only report their position
  every few seconds, so they are kept within about 2 seconds, not frame
  accurate.
- **No panel in native apps.** Apps that don't use Jellyfin's web interface
  (TV apps, Fladder, Swiftfin, ...) have no chat or room list; they join as
  devices.
