---
title: Sync Algorithms
parent: Technical Reference
nav_order: 5
---

# Synchronization Algorithms

## Overview

JellyWatchParty uses multiple algorithms to maintain playback synchronization between clients, addressing the specific challenges of HLS/transcoded streaming.

## 1. Clock Synchronization (Simplified NTP)

### Problem
Clients have different system clocks. To synchronize actions, we need to know the offset between client and server clocks.

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

**Calculation:**
```javascript
rtt = T3 - T1;                           // Round-trip time
serverTimeAtT3 = T2 + (rtt / 2);         // Estimated current server time
serverOffsetMs = serverTimeAtT3 - T3;    // Client/server offset
```

**EMA Smoothing (Exponential Moving Average):**
```javascript
// Prevents sudden jumps from latency variations
serverOffsetMs = hasTimeSync
    ? (0.6 * serverOffsetMs + 0.4 * newOffset)
    : newOffset;
```

### Usage
```javascript
function getServerNow() {
    return Date.now() + serverOffsetMs;
}
```

### Clock Skew Tolerance

The system tolerates significant clock differences between clients:

| Skew Level | Behavior |
|------------|----------|
| < 100ms | Ideal - no noticeable drift |
| 100ms - 500ms | Good - corrected by playback rate adjustment |
| 500ms - 2000ms | Acceptable - noticeable catch-up but functional |
| > 2000ms | Poor - may trigger hard seek, visible jumps |

## 2. Synchronized Action Scheduling

### Problem
When the host clicks "Play", all clients must start playback at the same instant, despite variable network latency.

### Solution: Target Server Timestamp

```
Host                  Server                    Client B
  │                      │                          │
  ├─ play @ pos 120s ───►│                          │
  │                      │                          │
  │                      ├── target_server_ts ─────►│
  │                      │   = now + 1000ms         │
  │                      │                          │
  │                      │                    scheduleAt(target_ts)
  │                      │                          │
  │                      │                          ▼
  │                      │                    [Wait...]
  │                      │                          │
  ◄──────────────────────┼──────────────────────────┤
                    [T = target_server_ts]          │
                                              video.play()
```

### Client-Side Implementation

```javascript
function scheduleAt(serverTs, fn) {
    const serverNow = getServerNow();
    const delay = Math.max(0, serverTs - serverNow);

    if (delay === 0) {
        fn();  // Immediate execution
    } else {
        setTimeout(fn, delay);
    }
}
```

### Configured Delays

| Action | Delay (ms) | Reason |
|--------|------------|--------|
| `play` | 1000 | Allow buffering sync (reduced from 1500ms) |
| `pause` | 300 | Shorter, no buffering needed |
| `seek` | 300 | Shorter, direct position |

## 3. Position Correction with Lead Time

### Problem
Messages take time to arrive. When client receives "position = 120s", the host is already further ahead.

### Solution: Lead Time Compensation

```javascript
function adjustedPosition(position, serverTs) {
    const serverNow = getServerNow();
    const elapsed = Math.max(0, serverNow - serverTs);  // Time since send
    const lead = SYNC_LEAD_MS;  // 300ms margin

    return position + (elapsed + lead) / 1000;
}
```

### Example

```
Server time:    1000ms         1050ms         1100ms
                  │               │               │
Host sends:     pos=120s        ─────────────────►│
                  │                               │
Client receives: ──────────────────────────────────│
                                               pos=120s
                                               elapsed=100ms
                                               lead=120ms
                                               adjusted=120.22s
```

## 4. Continuous Drift Correction

### Problem
Even with perfect initial synchronization, clients drift over time (slightly different playback speeds, buffers, etc.).

### Algorithm: syncLoop (non-hosts only)

Drift correction uses **hysteresis** (a Schmitt trigger): a correction burst only
*starts* once drift exceeds `DRIFT_CORRECTION_ENTER_SEC`, and once started, it only
*stops* once drift falls back under the tighter `DRIFT_CORRECTION_EXIT_SEC`. Between
bursts, playback sits untouched at exactly 1x. This is deliberate: a single static
threshold re-evaluated every tick means the very act of correcting (which shrinks
drift) can push drift back under the threshold and out again on jitter, causing
constant tiny, visible speed flicker. Two thresholds with a gap between them mean the
controller only reacts to real, sustained drift and stays quiet the rest of the time.

```javascript
function syncLoop() {
    // Calculate expected position
    const elapsed = (getServerNow() - lastSyncServerTs) / 1000;
    const expected = lastSyncPosition + elapsed;

    // Measure drift
    const drift = expected - video.currentTime;
    const absDrift = Math.abs(drift);

    if (!isDriftCorrecting) {
        // Not currently correcting: ignore anything below the enter threshold
        if (absDrift < DRIFT_CORRECTION_ENTER_SEC) {  // 0.3s
            video.playbackRate = 1;
            return;
        }
        isDriftCorrecting = true;  // crossed into correction territory
    } else if (absDrift < DRIFT_CORRECTION_EXIT_SEC) {  // 0.1s
        // Already correcting and caught back up: stop and go quiet
        isDriftCorrecting = false;
        video.playbackRate = 1;
        return;
    }

    // Excessive drift: forced seek
    if (absDrift >= DRIFT_SOFT_MAX_SEC) {  // 2.0s
        video.currentTime = expected;
        video.playbackRate = 1;
        isDriftCorrecting = false;
        return;
    }

    // Soft correction zone: progressive sqrt-based speed adjustment
    // drift > 0 = behind = speed up
    // drift < 0 = ahead = slow down
    const sign = drift > 0 ? 1 : -1;
    const correction = sign * Math.sqrt(absDrift) * DRIFT_GAIN;
    const rate = clamp(1 + correction, 0.85, 2.0);
    video.playbackRate = rate;
}
```

### Visualization

```
                    DRIFT_SOFT_MAX_SEC = 2.0s
                           │
    ◄─────────────────────┼────────────────────►
    │         │           │           │        │
  SEEK     SLOW       QUIET ZONE     FAST     SEEK
 (<−2.0s) (−2.0s      (hysteresis)  (+0.3s   (>+2.0s)
           to −0.3s)   ±0.1-0.3s     to +2.0s)
    │         │                         │        │
    │    rate = 0.85               rate = 2.0     │
    │     (min)                       (max)       │
    └─────────┴──────────┬──────────────┴─────────┘
                         │
                    rate = 1.0

Once a burst starts (|drift| crosses ±0.3s), it holds the rate-adjustment path
until |drift| falls back under ±0.1s — not just until it re-crosses ±0.3s.
```

### Rate Formula (Progressive Sqrt Curve)

```
rate = 1 + sign(drift) * sqrt(|drift|) * DRIFT_GAIN
     = 1 + sign(drift) * sqrt(|drift|) * 0.50

Examples:
- drift = +0.25s → rate = 1 + sqrt(0.25) * 0.50 = 1.25x
- drift = +1.0s  → rate = 1 + sqrt(1.0) * 0.50 = 1.50x
- drift = +2.0s  → rate = 1 + sqrt(2.0) * 0.50 = 1.71x
- drift = +4.0s  → rate = 1 + sqrt(4.0) * 0.50 = 2.00x (capped)
- drift = -0.5s  → rate = 1 - sqrt(0.5) * 0.50 = 0.65x (clamped to 0.85x)
```

The sqrt curve provides stronger correction for larger drifts while staying smooth. Browser pitch correction (`preservesPitch`) keeps audio natural even at 2.0x.

## 5. HLS Handling and Feedback Loop Prevention

### The HLS Problem

HLS (HTTP Live Streaming) is an adaptive streaming protocol that chunks video into segments. This creates problematic behaviors:

1. **False states**: During buffering, `video.paused` may be `true` even without user pause
2. **Unstable position**: `currentTime` may jump or go backward while loading segments
3. **Variable latency**: Each seek triggers new segment loading

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

### Implemented Solutions

#### A. Sync Lock (`isSyncing`)

```javascript
// When receiving server command
function onServerCommand() {
    isSyncing = true;

    // ... apply command ...

    // Release after 2 seconds
    setTimeout(() => { isSyncing = false; }, 2000);
}

// Before sending to server
function onEvent() {
    if (isSyncing) return;  // Blocked!
    // ...
}
```

#### B. Buffering Detection

```javascript
// Track video events
video.addEventListener('waiting', () => { isBuffering = true; });
video.addEventListener('canplay', () => { isBuffering = false; });
video.addEventListener('playing', () => { isBuffering = false; });

// Filtering
function onPauseEvent() {
    if (isBuffering) return;  // False pause, ignore
    // ...
}
```

#### C. ReadyState Check

```javascript
function isVideoReady() {
    return video.readyState >= 3;  // HAVE_FUTURE_DATA
}

function sendStateUpdate() {
    if (!isVideoReady()) return;  // Not enough data
    // ...
}
```

#### D. Seeking Check

```javascript
function onEvent() {
    if (video.seeking) return;  // Currently seeking
    // ...
}
```

### Server-Side Protection

#### Cooldown After Command

```rust
const COMMAND_COOLDOWN_MS: u64 = 2000;

// After broadcasting player_event
room.last_command_ts = now_ms();

// On receiving state_update
if now_ms() - room.last_command_ts < COMMAND_COOLDOWN_MS {
    return;  // Ignore during cooldown
}
```

#### Position Jitter Filtering

```rust
const POSITION_JITTER_THRESHOLD: f64 = 0.5;

let pos_diff = new_pos - room.state.position;

// Small backward jump = HLS noise
if pos_diff < -0.5 && pos_diff > -2.0 {
    return;  // Ignore
}

// Micro-advance = insignificant
if pos_diff >= 0.0 && pos_diff < 0.5 {
    return;  // Ignore
}
```

### Buffering and HLS Edge Cases

| Scenario | Behavior |
|----------|----------|
| Segment loading | `isBuffering=true`, sync paused |
| Seek during buffer | Queued until ready |
| False pause (HLS artifact) | Filtered by buffering check |
| Backward position jump | Ignored if < 2s (HLS noise) |

Protection mechanisms: the `isSyncing` lock (2s) prevents feedback loops,
`readyState >= 3` is required before sending updates, and the server
applies a 2s cooldown after commands.

## 6. Ready/Pending Play Mechanism

### Problem
When a new participant joins, they must load the media before they can play. If the host clicks Play before everyone is ready, some will miss the start.

### Multiple Clients Joining Rapidly

When several clients join a room in quick succession, each join is
processed sequentially under the room lock, and participant updates are
batched within 100ms to avoid a message flood. The host's play command
waits up to 2s for all clients to be ready (see below); if clients aren't
ready within that timeout, play proceeds anyway. Allow 2-3 seconds between
mass joins for optimal sync.

### Solution

```
Host                     Server                   Client B
  │                         │                         │
  │                         │◄── join_room ───────────┤
  │                         │                         │
  │                         │  B not in ready_clients │
  │                         │                         │
  ├── player_event: play ──►│                         │
  │                         │                         │
  │                    all_ready() = false            │
  │                         │                         │
  │                    pending_play = {               │
  │                      position: 120,               │
  │                      created_at: now              │
  │                    }                              │
  │                         │                         │
  │                    schedule_timeout(2s)           │
  │                         │                         │
  │                         │◄── ready ───────────────┤
  │                         │                         │
  │                    all_ready() = true             │
  │                    pending_play = None            │
  │                         │                         │
  │◄── player_event: play ─┼── player_event: play ──►│
  │    target_ts = T+1.0s   │   target_ts = T+1.0s    │
  │                         │                         │
  ▼                         │                         ▼
video.play() @ T+1.0s       │              video.play() @ T+1.0s
```

### Safety Timeout

If a client never becomes ready (network issue, etc.), play is forced after 2 seconds:

```rust
fn schedule_pending_play(room_id, created_at, rooms, clients) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(2000)).await;

        if room.pending_play.created_at == created_at {
            // Timeout: force play
            broadcast_scheduled_play(room, clients, position, now + 1000);
            room.pending_play = None;
        }
    });
}
```

## Threshold and Timing Summary

| Parameter | Value | Location | Description |
|-----------|-------|----------|-------------|
| `SUPPRESS_MS` | 2000ms | Client | Anti-feedback lock duration |
| `SEEK_THRESHOLD` | 1.0s | Client | Min difference for seek broadcast |
| `STATE_UPDATE_MS` | 1000ms | Client | State send interval |
| `SYNC_LEAD_MS` | 300ms | Client | Compensation advance |
| `DRIFT_CORRECTION_ENTER_SEC` | 0.3s | Client | Drift needed to start a correction burst |
| `DRIFT_CORRECTION_EXIT_SEC` | 0.1s | Client | Drift must fall under this to stop correcting |
| `DRIFT_SOFT_MAX_SEC` | 2.0s | Client | Forced seek threshold |
| `PLAYBACK_RATE_MIN` | 0.85 | Client | Min catchup speed |
| `PLAYBACK_RATE_MAX` | 2.0 | Client | Max catchup speed |
| `DRIFT_GAIN` | 0.50 | Client | Proportional gain (sqrt curve) |
| `INITIAL_SYNC_COOLDOWN_MS` | 8000ms | Client | Cooldown after join (no HARD_SEEK) |
| `INITIAL_SYNC_MAX_MS` | 30000ms | Client | Max initial sync phase duration |
| `INITIAL_SYNC_DRIFT_THRESHOLD` | 0.5s | Client | Exit initial sync when caught up |
| `SYNC_LOOP_MS` | 500ms | Client | Sync loop interval |
| `PLAY_SCHEDULE_MS` | 1000ms | Server | Delay before play |
| `CONTROL_SCHEDULE_MS` | 300ms | Server | Delay before pause/seek |
| `MAX_READY_WAIT_MS` | 2000ms | Server | Ready timeout |
| `MIN_STATE_UPDATE_INTERVAL_MS` | 500ms | Server | State rate limit |
| `POSITION_JITTER_THRESHOLD` | 0.5s | Server | Position noise threshold |
| `COMMAND_COOLDOWN_MS` | 2000ms | Server | Cooldown after command |
