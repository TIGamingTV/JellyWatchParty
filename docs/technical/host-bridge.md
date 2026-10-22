---
title: Host Bridge
parent: Technical Reference
nav_order: 6
---

# Host Bridge

{: .note }
This page describes the **plugin's** bridge, used from the in-player
Watch Party panel. It is off unless an admin turns on **Let users bridge
their devices from the Watch Party panel** (meant for small servers with
trusted users), and users can only bridge their own sessions. The normal
way is the session server's
[admin panel]({{ '/admin-panel/' | relative_url }}#jellyfin-devices): it drives devices over the
Jellyfin API, can put any device into any room as host or receiver, and
starts playback on receivers itself.

## Overview

Hosting a watch party normally requires the injected web client, since
the browser is what talks to the session server. Native/TV clients can't
run injected JavaScript (e.g. [Fladder](https://github.com/jyc-oss/fladder),
Swiftfin, Infuse, official mobile/TV apps). The bridge lets the Jellyfin
server itself connect to the session server on behalf of such a session,
in one of two roles:

- **Host:** a session that is playing something drives a new room. Guests
  join it from their room list like any other room; to them it is
  indistinguishable from a browser-hosted one.
- **Receiver:** a session is attached to the room the calling browser is
  in and kept in sync with it through Jellyfin's generic remote-control
  playstate commands (Pause / Unpause / absolute Seek), which official
  clients such as the Android TV app honour. The session must already be
  playing the room's item; the receiver keeps play/pause and position
  aligned but doesn't start playback.

Both roles are off by default behind the master switch
`PluginConfiguration.EnablePanelBridging`; with it on, the host role is
gated by `AllowThirdPartyClientHost` and the receiver role by
`AllowSupportedClientReceiver` (`PanelHostAllowed` /
`PanelReceiverAllowed` combine them). The combined values are sent to the
client as `allow_third_party_host` / `allow_supported_receiver` on
`/JellyWatchParty/Token`, so it only renders the matching picker, and are
enforced server-side: `Bridge/{sessionId}/Start` and
`Bridge/{sessionId}/Follow` answer `400` when their role is off, and
`Bridge/Sessions` returns an empty list when neither role is allowed.
Turning the switch (or a role) off stops the matching running bridges
(`HostBridgeManager.ApplyConfigurationAsync`, on
`Plugin.ConfigurationChanged`). Bridging also needs
`SessionServerUrl` to be set: the bridge connects from the Jellyfin
server, so there is no page host to fall back to.

**Ownership:** Sessions, Status, Start, Follow and Stop only cover the
caller's own sessions (session `UserId` == the caller's `Jellyfin-UserId`
claim). Callers in the `Administrator` role may bridge and stop any
session.

Both bridges send the bridged session's `DeviceId` as `bridge_device_id`
in `create_room` / `join_room`. The session server records it and shows
the connection as a *Plugin bridge* in its admin panel. One device is
never driven from two places: the admin panel refuses a device a plugin
bridge drives, and the server refuses a plugin bridge's create/join
(`reason: "device_already_bridged"`) for a device that is already in a
room. Start and Follow wait up to 5 s (`StartAnswerTimeout`) for the
server's answer (`room_state` or `error`), so a refusal (or a wrong room
password) reaches the user as an error instead of a bridge without a
room.

Stopping a bridge sends `leave_room` before closing, so it leaves its room
at once instead of after the server's 90 s reconnect grace. A bridge whose
room closes, or that an admin removes (`room_closed`), drops itself.

Running the injected UI directly (a browser, or Jellyfin Desktop via its
native player adapter — see
[Client: `utils/video.js`]({{ '/technical/client/' | relative_url }}#module-utilsvideojs)) remains the
way to join and drive a room from the device itself.

## How It Works

```
Native session (e.g. Fladder on Android TV)
      │ (Jellyfin server-side playback events only —
      │  the native client itself does nothing JWP-specific)
      ▼
ISessionManager (Jellyfin server)
      │ PlaybackStart / PlaybackProgress / PlaybackStopped
      ▼
HostBridgeManager (hosted service, subscribed for the server's lifetime)
      │ routes events to the matching SessionHostBridge, if any
      ▼
SessionHostBridge (one per bridged session)
      │ owns a ClientWebSocket to the session server
      │ speaks the normal client protocol: auth → create_room →
      │ player_event / state_update (+ ping every 20 s)
      ▼
Session Server (Rust) — room created, this session is the host
      ▲
      │ guests join normally, from their own room list
Browser guests (unaffected, unaware a bridge is involved)
```

## Components

### `Services/HostBridgeManager.cs`

A singleton `IHostedService` (registered in `ServiceRegistrator`) that
subscribes to `ISessionManager.PlaybackStart` / `PlaybackProgress` /
`PlaybackStopped` and to `Plugin.ConfigurationChanged`, and owns all
active bridges, keyed by Jellyfin session id: `SessionHostBridge`s and
`SessionFollowerBridge`s (a session is one or the other).

**Eligibility** (`GetEligibleSessions(onlyUserId)`): sessions that are
playing something, aren't bridged in either role, belong to the caller
(unless admin), and whose `Client` does **not** start with `"Jellyfin
Web"`, `"Jellyfin Desktop"` or `"Jellyfin Media Player"` (case-insensitive
prefix match, since `Client` carries a version). Those already run the
injected script and can create a room themselves.

`StartBridgeAsync(sessionId)` / `StartFollowerAsync(sessionId, roomId)`
check the session (exists, not already bridged, `SessionServerUrl` set),
start the bridge, and re-check that the role is still allowed;
`StopBridgeAsync(sessionId)` disposes either kind. `PlaybackStart` and
`PlaybackProgress` for a host-bridged session go to its
`OnPlaybackProgressAsync`; `PlaybackStopped` disposes the session's
bridge in either role.

### `Services/SessionHostBridge.cs`

One instance per host session. Opens a `ClientWebSocket` to
`SessionServerUrl` and sends the same protocol messages a browser host
would (see [Protocol]({{ '/technical/protocol/' | relative_url }})):

| Jellyfin event | → | Protocol message |
|---|---|---|
| Bridge start | → | `auth` (JWT for the session's owner when `HasUsableJwtSecret`, else `user_id`/`user_name`), then `create_room` (`user_name` "User (Device)", `start_pos` and `media_id` from the session's current playback, `bridge_device_id`) |
| Play/pause state change | → | `player_event` (`action: "play"` or `"pause"`, `position`) |
| Progress without a play-state change | → | `state_update` (`position`, `play_state`) |
| Every 20 s | → | `ping` (keeps the server's 60 s zombie reaper away from an idle bridge) |

It takes its `RoomId` from the `room_state` the server sends after
`create_room`, and clears it on `room_closed`. Sends are serialized
(`ClientWebSocket` forbids concurrent sends).

**Known limitation: no `set_media`.** The web host detects an item change
and sends `set_media` (see
[Protocol: `set_media`]({{ '/technical/protocol/' | relative_url }}#set_media)); a bridged native host
doesn't. Jellyfin fires `PlaybackStopped` on every item change, and
`HostBridgeManager` disposes the bridge on it, so starting another item
ends the bridge (and its room) instead of switching the room's media.

**Known limitation: doesn't follow others in democratic mode.** In a
[democratic room]({{ '/technical/protocol/' | relative_url }}#democratic-mode) other members may play,
pause and seek, but `SessionHostBridge` ignores incoming `player_event`s,
so the native host keeps playing as before. The server drops its
`state_update`s for 2 s after someone else's command; after that the
host's own state goes out again and undoes the command. Admin-panel and
chat-bot device hosts do carry such commands out (see
[Server: `jellyfin/`]({{ '/technical/server/' | relative_url }}#module-jellyfin)). Plugin bridges never
announce `capabilities: ["democratic"]` and can't control a democratic
room themselves, in either role.

**No reconnect:** a bridge doesn't re-establish a dropped WebSocket; stop
and start it again.

### `Services/SessionFollowerBridge.cs`

One instance per *receiver* session. Its `ClientWebSocket` sends `auth`
and `join_room` (`user_name` "User (Device)", `bridge_device_id`) for the
given room, pings every 20 s, and turns room messages into remote-control
commands:

| Room message | → | Remote-control command |
|---|---|---|
| `room_state` (on join): `state.play_state` + `state.position` | → | `Seek` if drifted, then `Pause`/`Unpause` |
| `state_update`: `play_state` + `position` | → | same |
| `player_event`: `action` (`play`; `pause` or `buffering` = paused) + `position` | → | same |

Commands go through
`ISessionManager.SendPlaystateCommand(controllingSessionId: "", sessionId,
…)`; an empty controlling-session id skips Jellyfin's control-permission
check and relays the command straight to the target session. `Pause` /
`Unpause` is only sent when the commanded state changes; `Seek` only when
the session's reported position is more than 2 s
(`DriftThresholdSeconds`) from the room's and the last seek is at least
3 s (`SeekCooldown`) old.

`RoomId` is set only when the server confirms the join with `room_state`,
so a rejected join doesn't show up as a connected bridge. On that
`room_state` the follower also sends `ready`: it has no video to buffer,
and the server's play gate (`all_ready` / `pending_play`) would otherwise
hold back every `play` for the whole room until it timed out waiting for
it. `ready` lasts for the room's lifetime, so it is sent once.

A follower applies every relayed `player_event`, so in a democratic room
it also follows other members' commands; what someone does on the
receiver device itself is not sent to the room (only admin-panel device
bridges with *device control* do that).

**Known limitations (receiver):**
- **Doesn't follow `media_changed`:** it is ignored, and since the
  follower is disposed on its session's `PlaybackStopped`, an item switch
  just ends the bridge.
- **No password-protected rooms:** `join_room` carries no password (the
  web client doesn't keep it), so the server refuses the join and Follow
  fails with the server's error.
- **No reconnect**, like the host bridge.

### `Services/SessionServerAuth.cs`

JWT minting shared by the bridges (token for the *bridged session's*
owner, not the HTTP caller) and `/JellyWatchParty/Token`; see
[Plugin: JWT generation]({{ '/technical/plugin/' | relative_url }}#jwt-generation).

## REST Endpoints

See [Plugin: REST API Reference]({{ '/technical/plugin/' | relative_url }}#rest-api-reference) for the
endpoint table, auth and error answers: `GET Bridge/Sessions`, `GET
Bridge/Status`, `POST Bridge/{sessionId}/Start`, `POST
Bridge/{sessionId}/Follow?roomId=…` (the web client passes the room it is
in) and `POST Bridge/{sessionId}/Stop`.

`Bridge/Sessions` returns `sessionId`, `userName`, `deviceName`, `client`,
`nowPlayingItemName` per eligible session. `Bridge/Status` and the
`Start`/`Follow` responses return `sessionId`, `userName` ("User
(Device)"), `roomId`, `connected`, `role` (`"host"` or `"receiver"`);
`Stop` returns an empty `200`. The controller spells these camelCase keys
out in anonymous objects, the same way `/Token` spells out
`user_id`/`auth_enabled`.

## UI

`src/clients/jellyfin-web/ui/bridge.js` fills two optional sections of the
Watch Party panel (`ui/render.js`), not the Jellyfin admin page:

- In the lobby, *Host From Another Device (e.g. Fladder)*, rendered only
  when `allow_third_party_host` is true: each eligible session gets a
  **Host** button (`Start`).
- In a room, *Add a Device to This Room*, rendered only when
  `allow_supported_receiver` is true: each eligible session gets a
  **Receiver** button (`Follow` with `state.roomId`).

Both list the active bridges with their role and room and a **Stop**
button, and are refreshed with the panel. Calls use `utils.apiFetch`
(the `Authorization: MediaBrowser …` header); errors are shown as toasts.

## Related

- [Features: Known Limitations]({{ '/features/' | relative_url }}#known-limitations)
- [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }}#frequently-asked-questions)
- [User Guide: Watching on a TV App]({{ '/user-guide/' | relative_url }}#watching-on-a-tv-app)
