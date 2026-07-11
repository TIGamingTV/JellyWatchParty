---
title: Host Bridge
parent: Technical Reference
nav_order: 6
---

# Host Bridge

## Overview

Normally, hosting a watch party requires running the injected web
client — the browser is what talks to the session server. That's a
problem for native/TV clients that can't run injected JavaScript at
all (e.g. [Fladder](https://github.com/jyc-oss/fladder) on Android TV,
Swiftfin, Infuse, official mobile/TV apps). **Host Bridge** solves this
on the *host* side only: any logged-in user with browser access to the
same Jellyfin server can bridge a currently-playing native session in
as a room host. Guests are completely unaffected — they still join the
resulting room from their own room list exactly as they would any
other room, and the room is indistinguishable from a browser-hosted one
to them.

The bridge also supports the opposite direction — a **receiver**: a
native session can be attached to a room this browser is already in and
kept in sync with the host, driven by Jellyfin's generic remote-control
playstate commands (which official clients such as the Android TV app
honour: pause / unpause / absolute seek). This is how an official client
that can't run the injected UI can still *follow* a party. The session
must already be playing the room's item; the receiver keeps play/pause
and position aligned but does not start playback remotely.

So a native client can still participate as a guest via the receiver
role. Running the injected UI directly (a browser, or Jellyfin Desktop
via its native player adapter — see
[Client: `utils/video.js`](client.md#module-utilsvideojs)) remains the
way to join and drive a room from the client itself.

## How It Works

```
Native session (e.g. Fladder on Android TV)
      │ (Jellyfin server-side playback events only —
      │  the native client itself does nothing JWP-specific)
      ▼
ISessionManager (Jellyfin server)
      │ PlaybackStart / PlaybackProgress / PlaybackStopped
      ▼
HostBridgeManager (hosted service, subscribed for the plugin's lifetime)
      │ routes events to the matching SessionHostBridge, if any
      ▼
SessionHostBridge (one per bridged session)
      │ owns a ClientWebSocket to the session server
      │ speaks the normal client protocol: auth → create_room →
      │ player_event / state_update
      ▼
Session Server (Rust) — room created, this session is the host
      ▲
      │ guests join normally, from their own room list
Browser guests (unaffected, unaware a bridge is involved)
```

## Components

### `Services/HostBridgeManager.cs`

A hosted service (`IHostedService`) that subscribes to Jellyfin's
`ISessionManager.PlaybackStart`/`PlaybackProgress`/`PlaybackStopped`
events for the plugin's entire lifetime and owns all currently-active
`SessionHostBridge` instances, keyed by Jellyfin session ID.

**Eligibility filter** (`GetEligibleSessions()`): only sessions that are
currently playing something, aren't already bridged, and whose `Client`
does **not** start with `"Jellyfin Web"`, `"Jellyfin Desktop"`, or
`"Jellyfin Media Player"` (prefix match, since `Client` includes a
trailing version — e.g. `"Jellyfin Web 10.11.11"`,
`"Jellyfin Desktop 3.0.0-dev"`). Those three already run the injected
script and can host normally via "Create Room", so they're excluded
from the bridge picker to avoid clutter.

`StartBridgeAsync(sessionId)` creates a `SessionHostBridge` for the
session and starts it; `StopBridgeAsync(sessionId)` tears one down.
Playback events for an already-bridged session are routed to that
bridge's `OnPlaybackProgressAsync`; a `PlaybackStopped` event removes
and disposes the bridge automatically.

### `Services/SessionHostBridge.cs`

One instance per bridged session. Owns a `ClientWebSocket` connection to
the session server and translates Jellyfin session events into the
exact same protocol messages a browser host would send
(see [Protocol](protocol.md)):

| Jellyfin event | → | Protocol message |
|---|---|---|
| Bridge start | → | `auth` (JWT if configured, else `user_id`/`user_name`), then `create_room` (with `start_pos` and `media_id` from the session's current playback) |
| Play/pause state changes | → | `player_event` (`action: "play"` or `"pause"`) |
| Position updates (no play-state change) | → | `state_update` |

It tracks its own `RoomId` by watching for the `room_state` message the
server sends back after `create_room`, and clears it on `room_closed`.

### `Services/SessionFollowerBridge.cs`

One instance per *receiver* session — the receive-only counterpart to
`SessionHostBridge`. Owns a `ClientWebSocket` that `auth`s and **joins**
an existing room (rather than creating one), then consumes the host's
broadcast messages and translates them into remote-control commands:

| Room message | → | Remote-control command |
|---|---|---|
| `room_state` (on join) / `state_update` — `play_state` + `position` | → | initial/ongoing `Pause`/`Unpause` and drift-correcting `Seek` |
| `player_event` — `action: "play"`/`"pause"` + `position` | → | `Unpause`/`Pause`, plus `Seek` on large drift |

Commands are sent via
`ISessionManager.SendPlaystateCommand(controllingSessionId: "", sessionId, …)`
— an empty controlling-session id skips Jellyfin's control-permission
path and relays the command straight to the target session's socket.
Play/pause is only re-sent on a state change, and `Seek` only fires when
the session has drifted past a threshold (~2 s) and no seek has been
issued within a short cooldown, so drift correction doesn't spam the
client. `HostBridgeManager` owns follower bridges alongside host bridges
(a session is one role or the other) and disposes a follower when its
session stops playing.

`RoomId` is set only when the server confirms the join with a `room_state`
message — not optimistically at connect time — so a rejected join does not
show up as a phantom connected bridge. On that same `room_state` the follower
sends a `ready` message: a headless bridge has no video to buffer, and the
server's play gate (`all_ready` / `pending_play`) would otherwise hold back
every host `play` for the whole room until it timed out waiting for the
follower to ready. `ready` persists for the room's lifetime, so it is sent
once, on join.

**Known limitations (receiver):**
- **Password-protected rooms are not supported.** The follower's `join_room`
  does not carry the room password (the web client doesn't retain it), so the
  server rejects the join; the receiver simply never syncs.
- **No reconnect.** Like `SessionHostBridge`, a follower does not re-establish
  its WebSocket after a drop — it must be re-attached. It also does not tear
  itself down on `room_closed` (it just stops following); use Stop, or it is
  cleaned up when the session stops playing.

### `Services/SessionServerAuth.cs`

Shared JWT-minting logic used by both the bridge (minting a token for
the *bridged session's* owner, not the HTTP caller) and the normal
`/JellyWatchParty/Token` endpoint.

## REST Endpoints

See [Plugin: REST API Reference](plugin.md#rest-api-reference) for the
full endpoint table (`Bridge/Sessions`, `Bridge/Status`,
`Bridge/{sessionId}/Start`, `Bridge/{sessionId}/Stop`,
`Bridge/{sessionId}/Follow?roomId=…`) and auth gating. `Follow` attaches
the session to the given room as a receiver (the room-id comes from the
room the calling browser is currently in).

`Bridge/Sessions` returns `sessionId`, `userName`, `deviceName`, `client`,
`nowPlayingItemName` per eligible session; `Bridge/Status` and the
`Start`/`Follow`/`Stop` responses return `sessionId`, `userName`, `roomId`,
`connected`, `role` (`"host"` or `"receiver"`). Response fields are camelCase — Jellyfin's controllers don't
auto-camelCase JSON output, so the controller projects onto anonymous
objects with the exact keys the UI expects, matching the existing
`/Token` endpoint's `user_id`/`auth_enabled` convention of spelling
field names out explicitly rather than relying on a naming policy.

## UI

`src/clients/jellyfin-web/ui/bridge.js` renders a "Host From Another
Device" section directly inside the normal lobby panel
(`ui/render.js`'s `renderLobby`) — not the Jellyfin admin config page.
It lists eligible sessions and active bridges (polling the two `GET`
endpoints), and is rendered in **both** panel contexts: the pre-room
lobby (`renderLobby`) and the in-room view (`renderRoom`). Each eligible
session shows a single action matching the context: **Host** in the
lobby (start a new room from that session) and **Receiver** while in a
room (attach that session to the current room, passing `state.roomId`
to `Follow`). Active bridges show their role and can be stopped with a
button. All calls use the same `ApiClient.accessToken()` pattern the
rest of the client uses to call plugin endpoints.

## Related

- [Features: Known Limitations](../features.md#known-limitations) — how this changes the "web only" caveat
- [Troubleshooting & FAQ: Can I host from Fladder or another Android TV app?](../troubleshooting.md#usage)
- [User Guide: Hosting from a TV App](../user-guide.md#hosting-from-a-tv-app)
