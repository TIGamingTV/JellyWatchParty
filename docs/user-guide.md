---
title: User Guide
nav_order: 5
---

# User Guide

How to watch together once your administrator has installed JellyWatchParty
(see [Installation]({{ '/installation/' | relative_url }})). Use Jellyfin in a
browser, Jellyfin Desktop, or the web-based Jellyfin mobile apps. For TV apps,
see [Watching on a TV app](#watching-on-a-tv-app).

## The Watch Party Panel

Click the **group icon**:

- in Jellyfin's header, next to your user avatar, or
- in the video player's controls (**Watch Party**).

The panel opens at the bottom right. Close it with its **X**, with
**Escape**, or by clicking outside it; you stay in your room.

**Lobby** (not in a room):

- **Online** / **Offline**: whether you are connected to the session server.
- **Available Rooms**: active rooms with a lock if they have a password, a
  vote icon if they are democratic, and the number of people. **Join** joins.
- **Create Room**.
- **Server**: the session server you are connected to.

**Room view** (in a room): the room name, **Participants** (the host has a
star; each person shows a status such as *In sync*, *Catching up*,
*Buffering* or *Not watching*), your sync status, **Chat**, your latency
(**RTT**) and the **Leave** button (**Close room** for the host).

## Create a Room

1. Start playing the movie or episode you want to watch.
2. Open the panel and click **Create Room**.
3. Enter a password, or leave it empty for an open room, and click
   **Create Room**.

You are now the host. The room is named after you (for example
"Room de Alice"). If you create a room before playing anything, guests wait
until you start something.

## Join a Room

**From the panel:** open the panel, then click **Join** (or the room's row)
in **Available Rooms**. For a room with a password, enter it and click
**Join**. Your player opens the host's item and jumps to the host's
position.

**From the home screen:** active rooms appear in a **Watch Parties** row at
the top of the Jellyfin home screen (refreshed every few seconds). Each card
shows the item, the room name and how many people are watching. Click the
**play button** on a card to open the item and join; click the card itself
to open the item's details page.

## Watching Together

| You are | What happens |
|---------|--------------|
| Host | Your play, pause and seeks are sent to everyone. Your first play waits until everyone has the video loaded (at most 10 seconds) and then starts for everyone after a 3-2-1 countdown. Switching to another item switches everyone ("Host switched media"). |
| Guest | Your player follows the host. Small differences are corrected by speeding up or slowing down slightly; differences of 2 seconds or more with a seek. If you pause while the room is playing, playback resumes within about a second and you see "Only the host controls playback" (unless the room is democratic). |

Your sync status (guests):

| Status | Meaning |
|--------|---------|
| In sync | You match the host |
| Waiting for sync... | A synchronized play is about to start |
| Out of sync | Catching up by adjusting playback speed |
| Loading host's media… | Opening the host's item |

Audio and subtitle tracks are your own choice; changing them doesn't affect
anyone else.

**Leaving:** click **Leave**, or close the player as a guest. When the host
clicks **Close room** or leaves, the person who joined earliest becomes the
new host; the room ends when the last person leaves. If your connection
drops, you keep your place for 90 seconds.

## Chat

In a room, type in the **Chat** box and press **Enter** or click **Send**
(500 characters at most). When you join, you see the last 50 messages.
While the panel is closed, new messages show as notifications at the top
right and as an unread counter next to **Chat**.

## Democratic Mode

In a democratic room, everyone with the Watch Party panel can play, pause and
seek for the whole room once it has started. The first play, switching items
and the position everyone follows stay with the host.

- **Host:** tick **Everyone can control playback** in the room view. The
  switch only appears if the administrator turned on **Allow democratic
  mode** in the plugin settings (or the room is already democratic). Admins
  can also turn it on for any room.
- **Everyone:** the panel shows *Everyone can control playback* (with
  *(TV devices too)* if an admin let devices control the room), and
  notifications name who paused or resumed.

## Watching on a TV App

Apps without the Watch Party panel (the Android TV app, Fladder, Swiftfin,
...) can join as a **Jellyfin device**. Open the app, sign in as yourself and
start something, then:

- **Ask an admin** to add the device to a room in the
  [admin panel]({{ '/admin-panel/' | relative_url }}#jellyfin-devices), as
  host (everyone follows the device) or receiver (the device follows the
  host and is started on the room's item automatically).
- **Use the chat bot**, if your server has one: `/jwp device add` on
  [Discord]({{ '/discord-bot/' | relative_url }}), or **Add my device** on a
  room's panel in [Telegram]({{ '/telegram-bot/' | relative_url }}).
- **Use the panel**, if the administrator enabled panel bridging: in the
  lobby, **Host From Another Device** makes a playing device host a new room;
  in a room, **Add a Device to This Room** attaches a device that is playing
  the room's item as a receiver. Only your own devices are listed.

Fladder and some other apps can only be host, because they don't accept
remote control.

## Something Not Working?

See [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }}).
