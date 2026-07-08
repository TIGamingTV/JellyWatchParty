---
title: Troubleshooting & FAQ
nav_order: 7
---

# Troubleshooting & FAQ

## Quick Diagnostic Checklist

1. [ ] Session server running? (`curl http://localhost:3000/health`)
2. [ ] Plugin installed? (Check Dashboard > Plugins)
3. [ ] Script tag in Custom HTML? (Dashboard > General) — or is [file-transformation](installation#enable-the-client-script) installed?
4. [ ] Browser cache cleared? (Ctrl+F5)
5. [ ] Correct WebSocket URL configured?
6. [ ] Firewall allowing the session server port (default 3000)?

Related: [Core Structure](core-structure) for expected system behavior,
[Security](security) for authentication/CORS issues, [Configuration](configuration)
for server and plugin settings.

## Common Issues

### Watch Party Button Not Visible

1. **Check Custom HTML configuration** — Dashboard > General > Branding should have:
   ```html
   <script src="../JellyWatchParty/ClientScript"></script>
   ```
2. **Hard refresh the browser** (Ctrl+F5 / Cmd+Shift+R), or clear the cache completely
3. **Check browser console** (F12) — a `404` means the script wasn't found (plugin not installed); a `CORS` error means the WebSocket was blocked (check CORS config)
4. **Verify plugin is installed** — Dashboard > Plugins should show "JellyWatchParty"; check Jellyfin logs for plugin load errors

### Cannot Connect to Session Server

1. **Check server is running:** `docker ps | grep session` and `curl http://localhost:3000/health`
2. **Check firewall:** `nc -zv localhost 3000`, open the port if needed (`sudo ufw allow 3000/tcp`)
3. **Check the WebSocket URL** — browser console shows the attempted URL; should be `ws://host:3000/ws` or `wss://host:3000/ws`
4. **Check CORS** — session server logs show CORS errors; set `ALLOWED_ORIGINS` to include your Jellyfin URL

### Sync Issues

Participants out of sync, playback drifting apart, or frequent jumping/stuttering:

1. **Wait a few seconds** — initial sync takes 2-3 seconds and drift correction is gradual by design
2. **Host: pause and play again** to re-sync everyone
3. **Check network quality** — high latency causes sync issues; check RTT in the panel (ideal < 100ms)
4. **HLS/transcoding** — transcoded streams have higher latency; try direct play or reduce quality if network is slow
5. **Check everyone has the same media** — different versions may have different durations

Technical details: [Sync Algorithms](technical/sync).

### Room Closes Unexpectedly

Causes: the host disconnected (network dropped, browser closed, computer slept) and didn't reconnect within 90 seconds and no other participant remained to be promoted; or the server restarted (rooms are in-memory/ephemeral). Check server logs for errors if this happens with a stable host connection.

Technical details: [Core Structure: Host Disconnect](core-structure#host-disconnect-grace-period-then-closure).

### Authentication Errors

1. **Check JWT configuration matches** — plugin JWT Secret must match server `JWT_SECRET`; both must be configured or both must be empty
2. **Check token expiration** — default 1 hour; refresh the page for a new token
3. **Rate limiting** — max 10 token requests per minute; wait and try again

### HLS Streaming Issues

Sync works initially but drifts during playback, frequent buffering interrupts sync, position jumps backward, or "false pauses" trigger unwanted sync events. HLS chunks video into small segments, causing buffering gaps, position reporting delays, and `readyState` changes during segment loads. JellyWatchParty filters most of this automatically (see [Sync Algorithms: HLS Handling](technical/sync#5-hls-handling-and-feedback-loop-prevention)); if issues persist:

- Reduce bitrate/quality in Jellyfin playback settings
- Prefer Direct Play or Direct Stream over full transcoding (Dashboard > Playback > Active Devices shows which is in use)
- Safari uses native HLS (not hls.js) and may report `readyState` differently during buffering — keep the tab in focus, or use Chrome/Firefox

### Rate Limiting Issues

| Limit | Value | Applies To |
|-------|-------|------------|
| Token requests | 10/min | Per user, plugin endpoint |
| WebSocket messages | 30/sec | Per client connection |
| Message size | 64 KB | Per message |

Usually caused by rapid page refreshes/reconnection attempts (token limit) or spamming play/pause/seek (message limit) — normal usage won't hit either. Check server logs: `docker logs session-server 2>&1 | grep -i "rate"`.

### Panel Opens But Empty

Wait for the WebSocket to finish connecting ("Connecting..." status), check the browser console (F12) for JavaScript/script-loading errors, and clear the browser cache in case an old script version is cached.

## Log Analysis

**Session server:** `docker logs session-server` (or `-f` to follow). Look for `Client connected`/`disconnected`, `Room created`/`closed`, `SECURITY: Wildcard origin` (CORS warning), `Message too large`, `Invalid token`.

**Jellyfin:** Docker (`docker logs jellyfin`), Linux (`/var/log/jellyfin/`), Windows (`%ProgramData%\Jellyfin\Server\log\`). Look for `[JellyWatchParty] JWT authentication is enabled.` or `[JellyWatchParty] JwtSecret is not configured.`

**Browser console:** F12 > Console, filter by "JWP". `[JWP] Loaded`, `[JWP] Connected`, `[JWP] Disconnected`, `[JWP] Room joined`.

## Network Debugging

```javascript
// In browser console — check WebSocket state
console.log(JWP.state.ws?.readyState);
// 0 = CONNECTING, 1 = OPEN, 2 = CLOSING, 3 = CLOSED
```

```bash
curl http://localhost:3000/health
npm install -g wscat && wscat -c ws://localhost:3000/ws
```

Or open Developer Tools (F12) > Network tab > filter "WS" to inspect WebSocket messages directly.

## Performance Issues

**High CPU (server):** check number of rooms/clients. **High CPU (client):** reduce video quality, close other tabs, check for JS errors.

**Memory (server):** rooms are in-memory (~1KB/client, ~5KB/room); restart to clear if needed.

**Slow sync:** reduce `SYNC_LEAD_MS` if latency is low, increase if latency is high (see [Sync Algorithms](technical/sync)); check client hardware.

## Reset Procedures

**Client:** clear browser cache/cookies, hard refresh, or in console: `localStorage.clear(); location.reload();`

**Server:** `docker restart session-server` (clears all rooms)

**Plugin:** Dashboard > Plugins > JellyWatchParty > clear all fields > Save > restart Jellyfin

## Frequently Asked Questions

### General

**What is JellyWatchParty?** A Jellyfin plugin that lets multiple users watch the same video in sync — when the host plays, pauses, or seeks, everyone follows automatically.

**Is it free?** Yes, open source and free to use.

**Does it work with Plex or Emby?** No — it uses Jellyfin's plugin API and web interface specifically.

**Do all participants need Jellyfin accounts?** Yes, everyone needs access to the same Jellyfin server and media library.

### Setup

**Why do I need a separate session server?** Jellyfin's plugin architecture doesn't support WebSocket endpoints, so a separate lightweight server handles real-time communication between clients.

**Can I run everything on one machine?** Yes — Jellyfin and the session server use different ports (8096 and 3000 by default).

**Is Docker required?** No, but recommended. See [Installation](installation) for native build options.

**Why do I need to add a script tag manually?** Since Jellyfin 10.9, plugins can't automatically inject scripts for security reasons — the manual step (or the file-transformation plugin) ensures administrators explicitly approve script injection.

### Usage

**Who controls playback?** The host (room creator) — their play/pause/seek actions are mirrored to all participants. Democratic mode (all participants controlling playback) is planned.

**What happens if the host leaves?** A brief disconnect (network blip, backgrounded app) is invisible to participants for 90 seconds while the server waits for reconnection. If the host doesn't return and other participants remain, the earliest-joined one is automatically promoted to host and the room stays open. It only closes if no one is left.

**Can I host from Fladder or another Android TV app?** Yes, via [Host Bridge](technical/host-bridge) — any logged-in user with browser access can bridge a native session in as host.

**Can I chat with other viewers?** Yes — real-time text chat in the panel, with the last 50 messages replayed to late joiners/reconnecting clients.

**Can I make a room private?** Yes, with an optional password set at room creation.

**Does everyone need the same video quality?** No — each client transcodes independently; sync is based on playback position, not video quality.

### Sync Quality

**How accurate is the sync?** Typically within 100-200ms, using clock synchronization and drift correction.

**Why do I see slight speed changes?** Playback speed is adjusted (0.85x-2.0x) to gradually correct drift without jarring seeks — imperceptible in most cases. See [Sync Algorithms](technical/sync).

**What if I'm several seconds behind?** If drift exceeds 2.0 seconds, the client seeks directly instead of adjusting speed.

**Does buffering affect sync?** Yes, temporarily — the system waits for all clients to be "ready" before starting playback and continuously corrects drift afterward.

### Technical

**What ports are used?** 8096 (Jellyfin) and 3000 (session server) by default.

**What protocol is used?** WebSocket with JSON messages — see [Protocol](technical/protocol).

**Is the connection encrypted?** It can be — use WSS with HTTPS. See [Security](security).

**How is authentication handled?** Optional JWT tokens — see [Security](security).

## Getting Help

When reporting issues, include: your Jellyfin version, browser and OS, Docker version (if applicable), session server + plugin configuration (without secrets), reverse proxy setup, relevant logs (server/Jellyfin/browser console), and steps to reproduce.

- [GitHub Issues](https://github.com/TIGamingTV/JellyWatchParty/issues) - Bug reports
- [GitHub Discussions](https://github.com/TIGamingTV/JellyWatchParty/discussions) - Questions
- [Jellyfin Forums](https://forum.jellyfin.org/) - Community help
