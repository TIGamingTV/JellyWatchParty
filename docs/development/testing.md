---
title: Testing
parent: Development
nav_order: 3
---

# Testing Guide

## Automated Tests

`just test` runs all four suites. CI runs the same ones (see
[CI/CD]({{ '/development/ci/' | relative_url }})).

| Suite | Location | Command |
|-------|----------|---------|
| Session server (Rust) | inline `#[cfg(test)]` modules in `src/server/src/**` | `just test server` |
| Chat bot (Rust) | inline modules in `src/integrations/discord-bot/src/**` | `just test discord` |
| Plugin (C#, xUnit + Moq) | `src/plugins/jellyfin/JellyWatchParty.Tests/` | `just test plugin` |
| Client (JavaScript, `node:test`) | `src/clients/jellyfin-web/tests/*.test.js` | `just test client` |

### Session server

```bash
cd src/server
cargo test                         # all tests
cargo test all_ready               # tests whose name contains "all_ready"
cargo test -- --nocapture          # show output
RUST_BACKTRACE=1 cargo test
```

There is no `tests/` directory: tests live next to the code.
`src/server/src/test_helpers.rs` builds clients, rooms and receivers, and the
route tests drive the real router (including a real WebSocket upgrade via
`tokio-tungstenite`). Linking needs mold, see
[Setup]({{ '/development/setup/' | relative_url }}#cargo-configuration).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use std::collections::HashSet;

    #[test]
    fn all_ready_false() {
        let mut room = test_helpers::create_room("r1", "host");
        room.clients = vec!["host".to_string(), "guest".to_string()];
        room.ready_clients = HashSet::from(["host".to_string()]);
        assert!(!all_ready(&room));
    }
}
```

### Chat bot

```bash
cd src/integrations/discord-bot
cargo test --locked
```

The API tests use a raw socket server in place of the session server; no
Discord or Telegram connection is needed.

### Plugin

`just test plugin` runs `dotnet test JellyWatchParty.Tests/ -f net10.0` in
`mcr.microsoft.com/dotnet/sdk:10.0`, so no local SDK is needed. To run it
locally with the .NET 10 SDK:

```bash
just build plugin   # copies the client JS into JellyWatchParty/Web/ (embedded resources)
cd src/plugins/jellyfin
dotnet test JellyWatchParty.Tests/JellyWatchParty.Tests.csproj
dotnet test JellyWatchParty.Tests/JellyWatchParty.Tests.csproj --collect:"XPlat Code Coverage"
```

### Client

```bash
node --test src/clients/jellyfin-web/tests/*.test.js
```

`tests/setup.js` fakes the minimal browser globals and loads `state.js`,
`utils/time.js`, `utils/misc.js` and `utils/validation.js`; test files
`require` it (and any other module they exercise) and use
`window.JellyWatchParty`.

### Naming

- Rust: descriptive snake_case without a `test_` prefix
  (`decide_attach_requires_the_resume_secret`).
- C#: `Subject_Condition_Expectation` or `Subject_Expectation`
  (`PluginGuid_IsValidGuid`).
- JS: `describe('<function>')` + `it('<behaviour>')`.

## Manual Testing

### Environment

1. `just up`
2. Open Jellyfin in two browsers (or a normal and a private window; each tab
   is its own client) and log in.

### Room management

| Test | Steps | Expected |
|------|-------|----------|
| Create room | Watch Party > start a room | Room created, you are host |
| Join room | Other window: Watch Party > select room > Join | Joined, synced to host |
| Leave room | In room > Leave | Back to the room list |
| Host leaves | Host leaves the room | Earliest remaining participant becomes host |
| Last member leaves | Everyone leaves | Room closes |
| Password | Create with password, join with a wrong one | "Incorrect password"; locked out after 5 tries for 60 s |

### Playback sync

| Test | Steps | Expected |
|------|-------|----------|
| First play | Host presses play with a guest in the room | Waits for everyone to be ready (max 10 s), then 3 s countdown |
| Play / pause | Host plays or pauses | All clients follow |
| Seek | Host seeks | All clients jump to the same position |
| Drift | Play for 10+ minutes | Rate correction starts above 0.3 s drift; hard seek above 2 s |

### Edge cases

| Test | Steps | Expected |
|------|-------|----------|
| Rapid seeks | Seek several times quickly | Clients end on the last position |
| Buffering | Throttle the network mid-play | No false pause broadcast |
| Disconnect | Drop the network briefly | Reconnects (backoff 1 s up to 30 s) and resumes the room; membership is kept for 90 s |
| Media switch | Host starts another item | Guests load it (`media_changed`) |
| Democratic mode | Host enables it, guest pauses | Everyone pauses |

Test at least Chrome and Firefox; Safari and mobile browsers when touching
playback or UI code.

### Network conditions

```bash
# Linux: add 200 ms latency (needs root and iproute2)
sudo tc qdisc add dev eth0 root netem delay 200ms
# Reset
sudo tc qdisc del dev eth0 root
```

## Load Testing

Each connection is rate-limited to 30 messages per second and rooms are
capped at 20 clients. The script below opens 50 connections that each create
a room; it only works with JWT disabled (`JWT_SECRET` unset), because
`create_room` requires an authenticated client.

```python
#!/usr/bin/env python3
import asyncio, json, time
import websockets  # pip install websockets

async def client(n):
    async with websockets.connect('ws://localhost:3000/ws') as ws:
        await ws.recv()  # client_hello
        await ws.recv()  # room_list
        await ws.send(json.dumps({
            'type': 'create_room',
            'payload': {'user_name': f'load-{n}', 'start_pos': 0},
            'ts': int(time.time() * 1000),
        }))
        await asyncio.sleep(60)

async def main():
    await asyncio.gather(*(client(i) for i in range(50)))

asyncio.run(main())
```

### Message latency

```javascript
// In the browser console, while connected
window.JellyWatchParty.actions.send('ping', { client_ts: Date.now() });
// The pong handler updates the latency shown in the panel (.jwp-latency)
```

### Connectivity checks

```bash
curl http://localhost:3000/health   # {"status":"ok","auth_enabled":...}
wscat -c ws://localhost:3000/ws     # npm i -g wscat
```

## Next Steps

- [Contributing]({{ '/development/contributing/' | relative_url }})
- [Setup]({{ '/development/setup/' | relative_url }})
- [Release]({{ '/development/release/' | relative_url }})
