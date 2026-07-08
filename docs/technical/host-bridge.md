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

This only makes hosting possible from native clients. Joining a room as
a guest still requires a client that can run the injected UI (a
browser, or Jellyfin Desktop via its native player adapter — see
[Client: `utils/video.js`](client.md#module-utilsvideojs)).

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

### `Services/SessionServerAuth.cs`

Shared JWT-minting logic used by both the bridge (minting a token for
the *bridged session's* owner, not the HTTP caller) and the normal
`/JellyWatchParty/Token` endpoint.

## REST Endpoints

See [Plugin: REST API Reference](plugin.md#rest-api-reference) for the
full endpoint table (`Bridge/Sessions`, `Bridge/Status`,
`Bridge/{sessionId}/Start`, `Bridge/{sessionId}/Stop`) and auth gating.

`Bridge/Sessions` returns `sessionId`, `userName`, `deviceName`, `client`,
`nowPlayingItemName` per eligible session; `Bridge/Status` and the
`Start`/`Stop` responses return `sessionId`, `userName`, `roomId`,
`connected`. Response fields are camelCase — Jellyfin's controllers don't
auto-camelCase JSON output, so the controller projects onto anonymous
objects with the exact keys the UI expects, matching the existing
`/Token` endpoint's `user_id`/`auth_enabled` convention of spelling
field names out explicitly rather than relying on a naming policy.

## UI

`src/clients/jellyfin-web/ui/bridge.js` renders a "Host From Another
Device" section directly inside the normal lobby panel
(`ui/render.js`'s `renderLobby`) — not the Jellyfin admin config page.
It lists eligible sessions and active bridges (polling the two `GET`
endpoints) and lets the user Start/Stop a bridge with a button, using
the same `ApiClient.accessToken()` pattern the rest of the client uses
to call plugin endpoints.

## Related

- [Features: Known Limitations](../features.md#known-limitations) — how this changes the "web only" caveat
- [Troubleshooting & FAQ: Can I host from Fladder or another Android TV app?](../troubleshooting.md#usage)
- [User Guide: Hosting from a TV App](../user-guide.md#hosting-from-a-tv-app)
