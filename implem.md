# Third-Party Client Integration Guide

Instructions for integrating a third-party Jellyfin client (e.g. Fladder,
Swiftfin, Streamyfin, Infuse, a custom Android TV app, etc.) with
JellyWatchParty. "Third-party client" here means anything that is **not**
Jellyfin Web / Jellyfin Desktop — i.e. a client that cannot run the injected
`src/clients/jellyfin-web/` JavaScript at all, and must talk to JellyWatchParty
over the network directly instead.

Split into two independent steps:

- **Step 1 (mandatory)** — what the client must implement to be *driven* by
  the synchronization: join a room and have its native player actually follow
  another participant's play/pause/seek/position, the same way a browser
  guest does today. Without this, the client can never be a real guest —
  it can, at best, only be bridged in as a host (see Host Bridge,
  `docs/technical/host-bridge.md`), which is a one-way, host-only path.
- **Step 2 (optional)** — REST/WebSocket calls a native in-player "Watch
  Party" widget can make to reach self-service parity with the web
  client's lobby panel (create/join/leave rooms, chat, browse the room
  list, start/stop a Host Bridge for *another* session). None of this is
  required for Step 1 to work — a Step-1-only client can still be joined
  into a room created elsewhere, or driven by a Host Bridge.

Reference implementation for everything below: `src/clients/jellyfin-web/`
(especially `ws/`, `playback/`, `state.js`) and the protocol spec at
`docs/technical/protocol.md`. Constants referenced below are the current
values from `docs/technical/sync.md` — check that page for the
up-to-date numbers before implementing, since they're occasionally tuned.

---

## Step 1 — Mandatory: being actively controlled by the synchronization

This is "build a minimal guest" — reimplementing the relevant slice of
`ws/connection.js` + `ws/handlers/*.js` + `playback/sync.js` against your
own client's native player API instead of an HTML5 `<video>` element. This
is the same gap tracked in `docs/PROGRESS.md` Round 15 as "Fladder-as-guest",
still open at the time of writing.

### 1.1 Obtain a token

```
GET /JellyWatchParty/Token
Header: X-Emby-Token: <jellyfin access token>
```

Returns:
```json
{
  "token": "eyJ...",
  "auth_enabled": true,
  "expires_in": 3600,
  "user_id": "abc123",
  "user_name": "John"
}
```

If `auth_enabled` is `false`, `token` is `null` — connect anonymously (skip
`auth` in 1.3). Also read `SessionServerUrl` from the plugin's exposed
config (or hardcode it per-deployment) — it is used verbatim as the
WebSocket URL, already `ws://` or `wss://`.

### 1.2 Open the WebSocket, with a persistent client ID

```
ws(s)://<session-server-host>/ws?client_id=<persistent-uuid>
```

- Generate a UUIDv4 once per install and persist it locally (equivalent of
  the web client's `localStorage['owp_persistent_client_id']`). Send it as
  `client_id` on **every** connection attempt, including reconnects.
- This is what lets the server reattach a dropped connection to its
  existing room membership (grace period, currently 90s — see
  `RECONNECT_GRACE_SECS` in `src/server/src/room/reconnect.rs`) instead of
  treating a brief network blip as a new stranger. Skipping this means
  any WiFi hiccup, app backgrounding, or device sleep silently ejects the
  client from its room.
- On connect, expect `client_hello` then `room_list` unsolicited from the
  server.

### 1.3 Authenticate (if `auth_enabled`)

Send immediately after connecting:
```json
{"type": "auth", "payload": {"token": "eyJ..."}, "ts": <now_ms>}
```

### 1.4 Join a room

```json
{"type": "join_room", "room": "<room-id>", "payload": {"password": "..."}, "ts": <now_ms>}
```
`password` only if the room has one (rooms in `room_list` carry
`has_password`). Handle the `error` response with `payload.reason ==
"wrong_password"` explicitly — that's the only machine-readable error
code the protocol currently defines.

On success you get `room_state`:
```json
{
  "payload": {
    "name": "Movie Night",
    "host_id": "...",
    "participant_count": 3,
    "media_id": "...",
    "state": {"position": 120.5, "play_state": "playing"},
    "chat_history": [...]
  }
}
```
Apply `state.position`/`state.play_state` to your native player
immediately (seek + play/pause) so a mid-episode join starts roughly
aligned — the drift-correction loop (1.7) takes over from there. You do
**not** need to do anything with `chat_history` for Step 1 — that's a
Step 2 concern.

### 1.5 Clock synchronization (required before you can honor `target_server_ts`)

Every server-driven playback command carries a `target_server_ts`, not a
"do it now" instruction — the server intentionally schedules commands
slightly in the future so all clients execute at (approximately) the same
wall-clock moment despite differing network latency. Without clock sync
you cannot correctly interpret this field.

Send periodically (recommended: every ~10s, matching the web client):
```json
{"type": "ping", "payload": {"client_ts": <now_ms>}, "ts": <now_ms>}
```
On `pong`:
```js
const rtt = Date.now() - payload.client_ts;
const serverOffset = server_ts + (rtt / 2) - Date.now();
```
Use `serverOffset` to compute `getServerNow() = Date.now() +
serverOffset` everywhere below. Smoothing this offset with an EMA
(α≈0.4, matching `playback/sync.js`) rather than trusting a single sample
is recommended but not strictly mandatory for a first implementation.

The ping cadence also doubles as your connection heartbeat — see the
zombie-timeout note in 1.9.

### 1.6 Signal readiness (`ready`)

```json
{"type": "ready", "room": "<room-id>", "payload": {"media_id": "..."}, "ts": <now_ms>}
```
Send this once your player has the correct media loaded and is prepared
to actually execute a scheduled play. The server holds a fresh `play`
command as `pending_play` until **all** participants in the room have
sent `ready`, then broadcasts it — skipping this means either the room
never starts (waiting on you forever, capped at `MAX_READY_WAIT_MS` =
2000ms server-side) or you receive `player_event`s scheduled for a time
before your player is actually prepared to hit them.

### 1.7 Receive and apply playback commands — the core of Step 1

**`player_event`** (host's discrete play/pause/seek/buffering actions):
```json
{
  "payload": {"action": "play", "position": 120.5, "target_server_ts": 1678900001000}
}
```
On receipt:
1. Compute `delay = target_server_ts - getServerNow()` (clamp to ≥0).
2. Schedule the action after `delay` ms (a plain timer is fine —
   accuracy within ~50-100ms is what matters, not sub-ms precision).
3. At execution time: `action == "play"` → seek to the adjusted position
   (`position + max(0, getServerNow() - target_server_ts)/1000` to
   account for any scheduling slop) and start playback; `"pause"` or
   `"buffering"` → seek to `position` and pause; `"seek"` → seek to
   `position`, preserving current play/pause state.
4. Suppress your own outgoing event listeners for a short window
   (~2000ms, the web client's `SUPPRESS_MS`/"Sync Lock") while performing
   this programmatic change — otherwise your player's own
   play/pause/seek event handlers (1.8) will misinterpret your own
   sync-driven action as a fresh user action and re-broadcast it,
   creating a feedback loop. This is the single most important
   anti-footgun in the whole protocol; see `docs/technical/sync.md` §5A.

**`state_update`** (host's periodic position/play-state, used for
continuous drift correction rather than discrete events):
```json
{"payload": {"position": 125.3, "play_state": "playing"}}
```
Minimum viable handling: track `(position, play_state, server_ts)` as a
reference point, and on a fixed interval (e.g. every 500ms,
`SYNC_LOOP_MS`) compare your player's actual position against
`expected = lastSyncPosition + (getServerNow() - lastSyncServerTs)/1000`.
If `|drift| >= 2.0s` (`DRIFT_SOFT_MAX_SEC`), hard-seek to `expected`.
That alone is enough to be a *correct* (if visibly jumpy) guest.

**Recommended, not strictly mandatory**: smooth sub-threshold drift via
playback-rate nudging instead of only hard-seeking, using the same
hysteresis shape as `playback/sync.js` — only *start* correcting once
drift exceeds `DRIFT_CORRECTION_ENTER_SEC` (0.3s), only *stop* once it
falls back under the tighter `DRIFT_CORRECTION_EXIT_SEC` (0.1s), rate =
`clamp(1 + sign(drift) * sqrt(|drift|) * DRIFT_GAIN, 0.85, 2.0)`. Skip
this if your native player API doesn't expose a playback-rate control
(many mpv-style native players do, via something like `setPlaybackRate`)
— falling back to hard-seek-only at the 2.0s threshold is a legitimate,
simpler Step 1 implementation, just less smooth to watch.

### 1.8 Report your own player's events (only relevant if this client can also act as host)

If you only ever intend this client to act as a **guest**, you can skip
this subsection — it exists purely so the same client code can also host.
A host sends its own play/pause/seek as `player_event`, and periodic
position as `state_update`, with the same message shapes as 1.7 (client →
server direction, no `target_server_ts`, added by the server). Do **not**
send anything if you are not the current host — `state.host_id` from
`room_state`/`host_changed`, compared to your own `client_id` from
`client_hello`, tells you whether you're allowed to. If you don't need
host capability from within this client at all, you can rely entirely on
Host Bridge (`docs/technical/host-bridge.md`) for hosting and implement
only the guest half here.

### 1.9 Handle room-lifecycle messages

At minimum:
- `room_closed` — stop applying commands, return to a "no room" state.
- `client_left` / `participants_update` — update participant count if
  you display one; not required for sync correctness.
- `host_changed` — cosmetic only for a guest (you follow whatever
  `player_event`/`state_update` arrives regardless of sender identity);
  update any "who's hosting" UI if you show one.
- Keep sending `ping` on a steady interval even when idle. The server's
  zombie-connection reaper (`ZOMBIE_TIMEOUT_MS`, currently 60s) will drop
  a connection with no traffic, and if your platform can suspend JS/app
  timers in the background for longer than the reconnect grace period
  (90s), a room rejoin will fail and you'll be treated as a new client —
  this exact failure mode is documented in `docs/PROGRESS.md` Rounds
  12-13 for Jellyfin Desktop's embedded web engine. Test specifically
  with your app backgrounded/minimized, not just foregrounded.

### 1.10 Summary checklist

| # | Requirement | Why mandatory |
|---|---|---|
| 1 | `GET /JellyWatchParty/Token` | Auth + know whether `auth_enabled` |
| 2 | Persistent `client_id` on every WS connect | Reconnect reattachment; without it, any disconnect = ejected from room |
| 3 | `auth` message (if enabled) | Server requires it before other messages |
| 4 | `join_room` (+ password handling) | Entry point for becoming a guest |
| 5 | `ping`/`pong` clock sync | Required to interpret `target_server_ts` correctly |
| 6 | `ready` | Required for host's first `play` to ever fire |
| 7 | Apply `player_event` (play/pause/seek/buffering) to native player | This *is* being "actively controlled" |
| 8 | Apply `state_update` drift correction (hard-seek minimum) | Keeps a long-running session from silently drifting apart |
| 9 | Suppress own event listeners during programmatic sync actions | Prevents feedback loops (Sync Lock) |
| 10 | Handle `room_closed` | Correctness — stop acting on a dead room |

---

## Step 2 — Optional: widget-triggered API calls for self-service parity

Everything here is for an **in-player widget** (a native button/panel your
client draws itself, equivalent to the web client's OSD button + lobby
panel from `ui/render.js`) that lets a user manage rooms from directly
inside this third-party client, instead of relying on a room created
elsewhere or a Host Bridge started from a browser. None of this is needed
for Step 1's guest behavior to work.

All REST calls use the user's normal Jellyfin access token:
`X-Emby-Token: <jellyfin access token>`. Base URL:
`http(s)://<jellyfin-host>:<port>/JellyWatchParty`.

### 2.1 Room list widget

WebSocket `list_rooms` → `room_list` response (also pushed unsolicited on
most room-lifecycle changes — no need to poll if you're already
connected per Step 1). Render as a "Join a Watch Party" list: `name`,
`count`, `has_password` (show a lock icon, prompt for password on join).

### 2.2 Create-room button

```json
{"type": "create_room", "payload": {"name": "Movie Night", "start_pos": 0.0, "media_id": "<jellyfin-item-id>", "password": "optional"}, "ts": <now_ms>}
```
On the `room_state` response, this client becomes host — see Step 1.8 if
you've implemented host-sending; otherwise treat this as "create the
room so others can join, but this client won't itself broadcast
playback" (less useful, so host-sending is worth pairing with this
button if you build it at all).

### 2.3 Leave-room button

```json
{"type": "leave_room", "room": "<room-id>", "ts": <now_ms>}
```

### 2.4 Chat widget

Send:
```json
{"type": "chat_message", "room": "<room-id>", "payload": {"text": "..."}, "ts": <now_ms>}
```
Receive `chat_message` broadcasts and the `chat_history` array already
delivered in `room_state` (1.4) for late-join/reconnect context (last 50
messages, capped in-memory, lost when the room closes). Purely additive
UI — has zero effect on sync.

### 2.5 Host Bridge controls (bridging a *different* session in as host)

Lets this client's widget start/stop a Host Bridge for some other
currently-playing Jellyfin session (e.g. a TV in another room), mirroring
`ui/bridge.js`'s "Host From Another Device" section. Useful if your
client is used as a remote/companion, less useful if it's only ever used
to watch on the device it's running on.

```
GET /JellyWatchParty/Bridge/Sessions
```
```json
[{"sessionId": "...", "userName": "...", "deviceName": "...", "client": "...", "nowPlayingItemName": "..."}]
```
Already excludes sessions running `"Jellyfin Web"`/`"Jellyfin
Desktop"`/`"Jellyfin Media Player"` clients (those can host themselves
directly, no bridge needed).

```
GET /JellyWatchParty/Bridge/Status
```
```json
[{"sessionId": "...", "userName": "...", "roomId": "...", "connected": true}]
```

```
POST /JellyWatchParty/Bridge/{sessionId}/Start
POST /JellyWatchParty/Bridge/{sessionId}/Stop
```
No request body. Poll `Bridge/Status` after `Start` to discover the
resulting `roomId` and surface a "join the room you just started" shortcut
in your widget.

All four Bridge endpoints are `[Authorize]` (any logged-in user, not
admin-gated) — response keys are camelCase; note in
`docs/PROGRESS.md` Round 17 that Jellyfin controllers do **not**
auto-camelCase, so match the field names exactly as shown above, not a
PascalCase guess.

### 2.6 Password-prompt UI note

If your platform has an equivalent gap to `window.prompt()` being a
silent no-op in CEF-based clients (see `docs/PROGRESS.md` Round 19), use
your platform's native modal/dialog API for the password prompts in 2.2
and the join flow in 2.1 — don't assume a browser-style blocking prompt
exists or works.

### 2.7 Summary checklist

| # | Feature | Endpoint/message |
|---|---|---|
| 1 | Browse rooms | WS `list_rooms` / `room_list` |
| 2 | Create room | WS `create_room` |
| 3 | Leave room | WS `leave_room` |
| 4 | Chat | WS `chat_message`, `room_state.chat_history` |
| 5 | List bridgeable sessions | `GET /JellyWatchParty/Bridge/Sessions` |
| 6 | List active bridges | `GET /JellyWatchParty/Bridge/Status` |
| 7 | Start a bridge | `POST /JellyWatchParty/Bridge/{sessionId}/Start` |
| 8 | Stop a bridge | `POST /JellyWatchParty/Bridge/{sessionId}/Stop` |

---

## Further reading

- `docs/technical/protocol.md` — full WebSocket message reference
- `docs/technical/sync.md` — all drift-correction/timing constants
- `docs/technical/host-bridge.md` — the existing host-only bridge that
  Step 2.5 exposes controls for
- `docs/PROGRESS.md` — Rounds 14/15/23 in particular cover real-world
  native-player integration pitfalls (no DOM `<video>` element, polling
  vs. event-based adapters, feedback-loop suppression during host-side
  track switches) worth reading before starting a native player adapter
