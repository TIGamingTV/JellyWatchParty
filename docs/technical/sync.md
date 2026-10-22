---
title: Sync Algorithms
parent: Technical Reference
nav_order: 5
---

# Synchronization Algorithms

## Overview

JellyWatchParty keeps clients in sync with a shared server clock, commands
scheduled at a server time, continuous drift correction on guests, and
filters on both sides against the false events of HLS/transcoded streaming.
Client code is in `src/clients/jellyfin-web` (constants in `state.js`),
server code in `src/server/src/ws`.

## Clock Synchronization

### Problem
Clients have different system clocks. To act at the same moment, each
client needs the offset between its clock and the server's.

### Algorithm

```
Client                          Server
   │                              │
   ├─── ping { client_ts: T1 } ──►│
   │                              │
   │◄── pong { client_ts: T1,     │
   │           server_ts: T2 } ───┤
   │                              │
   T3 (reception)                 │
```

Each `pong` gives a sample (`ws/handlers/clock.js`):

```javascript
rtt = T3 - T1;
sampleOffset = T2 + rtt / 2 - T3;   // server clock minus client clock
```

The last `TIME_SYNC_MAX_SAMPLES` (8) samples are kept; the one with the
lowest RTT (least queuing delay) is blended in with an EMA:

```javascript
best = sample with the smallest rtt among the last 8;
serverOffsetMs = hasTimeSync
    ? (1 - TIME_SYNC_EMA_ALPHA) * serverOffsetMs + TIME_SYNC_EMA_ALPHA * best.offset  // alpha 0.4
    : best.offset;
```

Until the first `pong`, the offset is seeded from `room_state`'s `server_ts`.
The client pings every `PING_INIT_MS` (2 s) until `PING_STABLE_AFTER` (5)
pongs, then every `PING_STABLE_MS` (30 s); if the offset later moves by
more than 50 ms in one step it goes back to the fast interval. The samples
are cleared when the socket closes.

### Usage (`utils/time.js`)
```javascript
const getServerNow = () => Date.now() + (state.serverOffsetMs || 0);
```

## Synchronized Action Scheduling

### Problem
When the host presses play, every client should start at the same instant
despite different network latency.

### Solution: Target Server Timestamp

The server stamps each relayed `player_event` with `target_server_ts`
(also in `server_ts`) and clients wait for that server time:

```
Host                  Server                    Client B
  │                      │                          │
  ├─ play @ pos 120s ───►│                          │
  │                      ├── target_server_ts ─────►│
  │                      │   = now + 1000ms         │
  │                      │                    scheduleAt(target_ts)
  │                      │                          │
  │                      │                    [T = target_server_ts]
  │                      │                    video.play()
```

The host's own player has already started when it sends the event; the
1 s delay covers delivery and the guest's buffering. Plays that the server
releases itself (pending play, start countdown) go to everyone, host
included, so all start at `target_server_ts`.

### Client-Side Implementation (`utils/time.js`)

```javascript
const scheduleAt = (serverTs, fn) => {
  if (state.pendingActionTimer) clearTimeout(state.pendingActionTimer); // only one pending action
  const serverNow = getServerNow();
  const delay = Math.max(0, serverTs - serverNow);
  if (delay === 0) { fn(); return; }
  state.pendingActionTimer = setTimeout(fn, delay);
};
```

### Configured Delays (server, `ws/constants.rs`)

| Action | Delay (ms) | Client handling |
|--------|------------|-----------------|
| `play` | 1000 (`PLAY_SCHEDULE_MS`) | Scheduled at `target_server_ts` |
| `play`, room's first, with others in the room | 3000 (`START_COUNTDOWN_MS`) | Countdown, then scheduled |
| `pause`, `seek`, `buffering` | 300 (`CONTROL_SCHEDULE_MS`) | Applied on receipt; `target_server_ts` / `server_ts` only feed the position math |

## Position Correction with Lead Time

### Problem
Messages take time to arrive. When a guest receives "position = 120 s" for a
playing room, the room is already further along.

### Solution: Lead Time Compensation (`utils/time.js`)

```javascript
const adjustedPosition = (position, serverTs) => {
  const serverNow = getServerNow();
  const ts = typeof serverTs === 'number' ? serverTs : serverNow;
  const elapsed = Math.max(0, serverNow - ts) + SYNC_LEAD_MS;  // 300 ms
  return position + elapsed / 1000;
};
```

Used for `play`/`pause` events and for the room position in `room_state`
and `media_changed`. `seek` and `buffering` positions are applied as is.
For a scheduled play, `server_ts` is the target time, so `elapsed` is 0
until it passes and the result is `position + 0.3 s`.

### Example

```
Server time:    1000ms                        1100ms
Host state:     pos=120s ─────────────────────► received
                                              elapsed = 100ms
                                              lead    = 300ms
                                              adjusted = 120.4s
```

A guest whose video is more than `SEEK_THRESHOLD` (1 s) from the adjusted
position pauses and seeks there before applying the action.

## Continuous Drift Correction

### Problem
Even after a synchronized start, guests drift (slightly different playback
speeds, buffering, decoding).

### Algorithm: `syncLoop` (guests only, every `SYNC_LOOP_MS` = 500 ms)

The loop (`playback/sync.js`) compares the video with where the room
should be: `expected = lastSyncPosition + (getServerNow() - lastSyncServerTs) / 1000`,
using the last baseline from `room_state`, `player_event` or an accepted
`state_update`. It does nothing (and resets the rate to 1x) for the host,
outside a room, when not on the room's media (status `loading`), when the
room isn't playing, or when the video is paused; it skips the tick while
buffering or below `readyState` 3.

Correction uses **hysteresis** (a Schmitt trigger): a burst only *starts*
once drift exceeds `DRIFT_CORRECTION_ENTER_SEC` (0.3 s) and, once started,
only *stops* when drift falls under `DRIFT_CORRECTION_EXIT_SEC` (0.1 s).
Between bursts playback stays at exactly 1x. With a single threshold, the
correction itself would push drift back and forth across it on jitter,
causing constant small speed changes.

```javascript
const drift = expected - video.currentTime;   // > 0: behind, < 0: ahead
const abs = Math.abs(drift);

if (!state.isDriftCorrecting) {
  if (abs < DRIFT_CORRECTION_ENTER_SEC) { goQuiet(video); return; }  // 1x, status "synced"
  state.isDriftCorrecting = true;                                    // status "syncing"
} else if (abs < DRIFT_CORRECTION_EXIT_SEC) {
  goQuiet(video); return;
}

if (abs >= DRIFT_SOFT_MAX_SEC) {                       // 2.0 s
  if (!state.isInitialSync && !inSyncCooldown) {
    // HARD_SEEK
    utils.suppress();
    video.currentTime = expected;
    state.isDriftCorrecting = false;
    video.playbackRate = 1;
    return;
  }
  // during initial sync / cooldown: fall through to rate correction
}

const sign = drift > 0 ? 1 : -1;
const rate = clamp(1 + sign * Math.sqrt(abs) * DRIFT_GAIN, PLAYBACK_RATE_MIN, PLAYBACK_RATE_MAX);
video.playbackRate = rate;
```

`syncCooldownUntil` is set 2 s ahead by a received `play` and by a
`state_update` that resumes a paused guest, so a just-started stream isn't
hard-seeked while it settles.

### Initial Sync (after joining)

When a guest gets `room_state` for a playing room, it seeks to the adjusted
position (if more than 1 s off) and enters initial sync: no hard seek for
`INITIAL_SYNC_COOLDOWN_MS` (8 s), and the phase ends when drift falls under
`INITIAL_SYNC_DRIFT_THRESHOLD` (0.5 s) or after `INITIAL_SYNC_MAX_MS` (30 s).
During it, the first tick with drift above 0.5 s seeks once more (to catch
Jellyfin's resume-position jump), and drift above `INITIAL_SYNC_MAX_DRIFT`
(10 s) always seeks; otherwise drift is corrected by rate only.

### Visualization

```
                         drift (expected - actual)
    ◄──────────────────────────┼──────────────────────────►
    │          │               │               │          │
  SEEK       SLOW         QUIET ZONE          FAST       SEEK
 (≤ -2.0s)  (rate < 1,   (hysteresis,      (rate > 1,  (≥ +2.0s)
            min 0.85)     ±0.1-0.3s)        max 2.0)
```

A burst starts when |drift| crosses 0.3 s and keeps adjusting the rate
until |drift| falls under 0.1 s.

### Rate Formula (Progressive Sqrt Curve)

```
rate = 1 + sign(drift) * sqrt(|drift|) * DRIFT_GAIN     (DRIFT_GAIN = 0.50)
clamped to [0.85, 2.0]

Examples (during a burst):
- drift = +0.25s → 1 + 0.50 * 0.50 = 1.25x
- drift = +1.0s  → 1 + 1.00 * 0.50 = 1.50x
- drift = +1.9s  → 1 + 1.38 * 0.50 = 1.69x
- drift = -0.5s  → 1 - 0.71 * 0.50 = 0.65x → clamped to 0.85x
- |drift| ≥ 2.0s → hard seek (rate correction only during initial sync / cooldown,
                    where e.g. +4.0s gives 2.00x)
```

The sqrt curve corrects larger drifts harder while staying smooth. Browser
pitch correction keeps audio natural at higher rates.

## HLS Handling and Feedback Loop Prevention

### The HLS Problem

HLS splits video into segments, which causes misleading player behavior:

1. **False states**: while buffering, `video.paused` may be `true` without a user pause
2. **Unstable position**: `currentTime` may jump or go backward while segments load
3. **Variable latency**: every seek loads new segments

### Feedback Loop Scenario

```
                    WITHOUT PROTECTION

Host ──► Server ──► Client
  │                   │
  │  "play @ 10:00"   │
  │                   │
  │            HLS buffering...
  │            video.paused = true (false!)
  │            video.currentTime = 9:58 (behind)
  │                   │
  │◄─ "pause @ 9:58" ─┤  ← ERROR!
  │                   │
Server broadcasts "pause" to all
  │                   │
Everyone stops!
```

### Client-Side Protection

#### A. Sync Lock (`isSyncing`) and suppression

Whenever the client moves the video itself (applying a command, a
`state_update` play/pause, joining), it calls `startSyncing()`
(`utils/time.js`), which sets `isSyncing` for `SUPPRESS_MS` (2000 ms) by
default. Video event handlers send nothing while it is set. A second,
time-based flag, `suppress()` / `shouldSend()` (`utils/misc.js`, also
2000 ms), covers the client's own corrections (hard seeks, start requests).

```javascript
// playback/bind.js
const onHostEvent = (action, video) => {
  if (!utils.shouldSend()) return;
  if (state.isSyncing) return;
  // ...
};
```

##### Per-user audio/subtitle tracks

Audio and subtitle tracks are **not** synced; each participant picks their
own, since only position and play state travel over the room's channel.
But switching a track can make Jellyfin reload the stream (most audio
switches under transcoding do), which fires the same
`play`/`pause`/`seeked`/`waiting` events used for sync.

`playback/tracks.js` wraps `playbackManager.setAudioStreamIndex` /
`setSubtitleStreamIndex`: when a client that sends playback events (the
host, or any member in democratic mode) switches a track, the sync lock is
engaged for `TRACK_SWITCH_SUPPRESS_MS` (8000 ms) to cover a transcode
restart. One-shot `canplay`/`playing` listeners collapse it back to the
normal `SUPPRESS_MS` window as soon as the reload finishes. On web builds
without a global `playbackManager`, the same is done on the video's
`loadstart` event.

#### B. Buffering Detection (`playback/bind.js`)

```javascript
waiting:  () => { state.isBuffering = true;  /* host: send player_event "buffering" */ },
canplay:  () => { state.isBuffering = false; },
playing:  () => { state.isBuffering = false; /* host, after buffering: send "play" */ },

// A host pause while buffering is not sent
if (action === 'pause' && state.isBuffering) return;
```

#### C. ReadyState Check

```javascript
const isVideoReady = () => video && video.readyState >= 3;  // HAVE_FUTURE_DATA
```

The host sends no `state_update` and no `seek` while not ready; guests skip
drift correction.

#### D. Seeking Check

The host sends no `state_update`, `play` or `pause` while `video.seeking`.
Seeks are also debounced (250 ms) and only sent if the position moved at
least `SEEK_THRESHOLD` (1 s) from the last sent one.

### Server-Side Protection

#### Cooldown After Command

```rust
const COMMAND_COOLDOWN_MS: u64 = 2000;

// On every relayed player_event
room.last_command_ts = now;
room.last_command_by = sender;

// On state_update
let in_command_cooldown =
    room.last_command_ts > 0 && now - room.last_command_ts < COMMAND_COOLDOWN_MS;
if in_command_cooldown && room.last_command_by != sender {
    return false; // someone else's command: drop everything
}
if new_play_state != room.state.play_state {
    return true;  // the host's own play-state change passes
}
// ... otherwise dropped during the cooldown (see below)
```

When the last command came from someone other than the host (a member of
a democratic room, or a chat bot user), even a play-state change is
dropped: the host's player hasn't carried the command out yet, and its
update would undo it.

#### Position Jitter Filtering

```rust
let pos_diff = new_pos - room.state.position;
let too_frequent = now - room.last_state_ts < MIN_STATE_UPDATE_INTERVAL_MS;   // 500 ms
let small_backward_jitter = (-2.0..-POSITION_JITTER_THRESHOLD).contains(&pos_diff); // HLS noise
let small_forward_jitter = (0.0..POSITION_JITTER_THRESHOLD).contains(&pos_diff);    // insignificant

!(in_command_cooldown || too_frequent || small_backward_jitter || small_forward_jitter)
```

### Democratic Mode

In a [democratic room]({{ '/technical/protocol/' | relative_url }}#democratic-mode) any
member may send `play`, `pause` and `seek`, but the host stays the only
source of `state_update`, so there is still one position everyone follows.
Extra guards against commands bouncing back and forth:

| Where | Guard |
|-------|-------|
| Server | The cooldown above blocks the host's stale updates after someone else's command. |
| Client (sender) | `mirrorRoomState` takes its own command as the new sync baseline (the server doesn't echo it), so `syncLoop` doesn't drift-correct back. |
| Client (all) | The command guard (`COMMAND_GUARD_MS` 3 s) ignores `state_update`s that disagree with the last sent or applied command by more than `COMMAND_GUARD_TOLERANCE_SEC` (2 s) or in play state; the first that agrees ends it. |
| Client (all) | `isEchoOfRoomState`: a play/pause/seek event that only brings the video in line with the room (e.g. a slow seek finishing after the 2 s `isSyncing` lock) is not sent. |
| Client (guest) | No commands before the room has started, while the stream is loading (`FOLLOWER_LOAD_MAX_MS`, cut to `SUPPRESS_MS` once it can play), before the first sync baseline, or while not on the room's item. |
| Client (host) | Applies other members' commands (not its own, not during a media switch, not a `play` while already playing). |
| Device bridges | Host devices are sent others' commands and stay quiet until they report again; receiver devices' own actions count only 3 s after the last command or room change (see [Server: `jellyfin/`]({{ '/technical/server/' | relative_url }}#module-jellyfin)). |

### Buffering and HLS Edge Cases

| Scenario | Behavior |
|----------|----------|
| Segment loading | `isBuffering = true`; guest drift correction skipped; host sends `buffering` (room paused) |
| Host seeks while not ready | Not sent (`readyState < 3`) |
| False pause (HLS artifact) | Host pause not sent while buffering |
| Backward position jump of 0.5-2 s | Dropped by the server as HLS noise |

## Ready/Pending Play Mechanism

### Problem
A member that just joined (or a guest after a media change) must load the
media first. If the host plays before everyone is ready, some would miss
the start.

### Solution

A member counts as ready once it has sent `ready`; the web client does so
when the room's item is loaded (`readyState >= 2`) and is the room's media.
Joining removes a member from `ready_clients`; the room creator starts
ready. A host `play` while someone isn't ready creates a `pending_play`:

```
Host                     Server                   Client B
  │                         │                         │
  │                         │◄── join_room ───────────┤
  │                         │  B not in ready_clients │
  │                         │                         │
  ├── player_event: play ──►│                         │
  │                    all_ready() = false            │
  │                    pending_play = {               │
  │                      position: 120,               │
  │                      created_at: now              │
  │                    }                              │
  │                    schedule_pending_play(2s)      │
  │                         │                         │
  │                         │◄── ready ───────────────┤
  │                    all_ready() = true             │
  │                    start_scheduled_play           │
  │                         │                         │
  │◄── player_event: play ──┼── player_event: play ──►│
  │    target_ts = T+1.0s   │   target_ts = T+1.0s    │
  ▼                         │                         ▼
video.play() @ T+1.0s       │              video.play() @ T+1.0s
```

While the play is pending, the host's `seek` / `state_update` only move its
position; a `pause` cancels it. Joins are processed one at a time under the
room lock; each new member resets the gate for itself.

### Safety Timeout

If a member never becomes ready, the play goes out anyway after the wait
(`MAX_READY_WAIT_MS`, 2 s; `START_READY_WAIT_MS`, 10 s, for the room's
first play):

```rust
fn schedule_pending_play(room_id, created_at, wait_ms, clients, rooms) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(wait_ms)).await;
        if room.pending_play.created_at == created_at {
            // still the same pending play: start it for everyone
            start_scheduled_play(room, clients, pending.position, now); // now + 1000 (or countdown)
        }
    });
}
```

### Start Countdown

A room created with `started: false` holds its first play: the server waits
up to 10 s for everyone (`start_pending`), then sends `play` with
`countdown: true` to everyone, host included, 3 s ahead. Details in
[Protocol: Start countdown]({{ '/technical/protocol/' | relative_url }}#start-countdown).

### Media Changes Re-Arm the Ready Gate

`set_media` (see [Protocol]({{ '/technical/protocol/' | relative_url }}#set_media))
resets `ready_clients` to just the host and clears `pending_play`, so the
mechanism above runs again for the new item: the host's next `play` waits
(up to `MAX_READY_WAIT_MS`) for guests to load it. Guests reset their own
`readyRoomId` on `media_changed` so they send `ready` again once the new
item is loaded, and ignore `player_event`/`state_update`/position
corrections until they are on it (`utils.isOnRoomMedia`), so a stale
correction can't seek the old stream to the new item's position.

## Threshold and Timing Summary

| Parameter | Value | Location | Description |
|-----------|-------|----------|-------------|
| `SUPPRESS_MS` | 2000ms | Client | Anti-feedback lock duration |
| `TRACK_SWITCH_SUPPRESS_MS` | 8000ms | Client | Lock for audio/subtitle track switches (collapses early once the reload settles) |
| `SEEK_THRESHOLD` | 1.0s | Client | Min position change for a host seek to be sent; min gap before a guest seeks |
| `STATE_UPDATE_MS` | 1000ms | Client | Host state send interval |
| `SYNC_LEAD_MS` | 300ms | Client | Lead added to adjusted positions |
| `DRIFT_CORRECTION_ENTER_SEC` | 0.3s | Client | Drift needed to start a correction burst |
| `DRIFT_CORRECTION_EXIT_SEC` | 0.1s | Client | Drift must fall under this to stop correcting |
| `DRIFT_SOFT_MAX_SEC` | 2.0s | Client | Hard seek threshold |
| `PLAYBACK_RATE_MIN` | 0.85 | Client | Min catch-up rate |
| `PLAYBACK_RATE_MAX` | 2.0 | Client | Max catch-up rate |
| `DRIFT_GAIN` | 0.50 | Client | Gain of the sqrt curve |
| `INITIAL_SYNC_COOLDOWN_MS` | 8000ms | Client | No hard seek after join |
| `INITIAL_SYNC_MAX_MS` | 30000ms | Client | Max initial sync phase duration |
| `INITIAL_SYNC_DRIFT_THRESHOLD` | 0.5s | Client | Ends initial sync when caught up |
| `INITIAL_SYNC_MAX_DRIFT` | 10s | Client | Drift that always seeks during initial sync |
| `SYNC_LOOP_MS` | 500ms | Client | Sync loop interval |
| `TIME_SYNC_MAX_SAMPLES` | 8 | Client | Clock samples kept (lowest RTT used) |
| `TIME_SYNC_EMA_ALPHA` | 0.4 | Client | Clock offset smoothing |
| `PING_INIT_MS` / `PING_STABLE_MS` | 2000ms / 30000ms | Client | Ping interval before / after `PING_STABLE_AFTER` (5) pongs |
| `START_SAFETY_MS` | 15000ms | Client | Host plays anyway if the start countdown never comes |
| `COMMAND_GUARD_MS` | 3000ms | Client | Democratic mode: disagreeing `state_update`s ignored after a command |
| `COMMAND_GUARD_TOLERANCE_SEC` | 2.0s | Client | Democratic mode: how far an update may be from the command and still agree |
| `FOLLOWER_LOAD_MAX_MS` | 15000ms | Client | Democratic mode: a loading guest stream's events aren't commands (at most) |
| `PLAY_SCHEDULE_MS` | 1000ms | Server | Delay before play |
| `CONTROL_SCHEDULE_MS` | 300ms | Server | Delay stamped on pause/seek/buffering |
| `START_COUNTDOWN_MS` | 3000ms | Server | First-play countdown |
| `MAX_READY_WAIT_MS` | 2000ms | Server | Ready timeout |
| `START_READY_WAIT_MS` | 10000ms | Server | Ready timeout for the room's first play |
| `MIN_STATE_UPDATE_INTERVAL_MS` | 500ms | Server | State rate limit |
| `POSITION_JITTER_THRESHOLD` | 0.5s | Server | Position noise threshold |
| `COMMAND_COOLDOWN_MS` | 2000ms | Server | State-update cooldown after a command |
| `USER_ACTION_GRACE_MS` | 3000ms | Server (bridge) | Device control: report must be this long after the last command / room change |
| `USER_SEEK_THRESHOLD_SECS` | 8.0s | Server (bridge) | Device control: jump that counts as a seek on the device |
