---
title: Configuration
nav_order: 6
---

# Configuration

## Plugin Configuration

Access the plugin configuration page at **Dashboard** > **Plugins** > **JellyWatchParty**.

| Setting | Default | Description |
|---------|---------|-------------|
| JWT Secret | (empty) | Secret key for signing tokens. Leave empty to disable authentication. **Must be at least 32 characters** — shorter non-empty values are treated as if authentication were disabled (see below), not just "less secure." |
| JWT Audience | `JellyWatchParty` | Audience claim in generated tokens |
| JWT Issuer | `Jellyfin` | Issuer claim in generated tokens |
| Token TTL | `3600` | Token lifetime in seconds (1 hour default) |
| Invite TTL | `3600` | Invite link lifetime in seconds (reserved for future use) |
| Session Server URL | (empty) | Custom WebSocket server URL. If empty, uses `ws(s)://[host]:3000/ws`. Invalid or suspicious values (wrong scheme, malformed URL, bare internal hostname) show a non-blocking warning in the config page and browser console/toast — the value is still saved and used as entered. |
| Hide native SyncPlay button | `false` | Hides Jellyfin's built-in SyncPlay button in the web client, since JellyWatchParty replaces that feature. |
| Allow third-party clients to host | `false` | Opt-in. Lets a native/third-party client that can't run the injected script (Fladder, Swiftfin, Infuse, official mobile apps, …) be bridged in as a room **host**. While off, the "Host From Another Device" picker is hidden and the server rejects host-bridge requests. See [Host Bridge](technical/host-bridge). |
| Allow supported clients as receivers | `false` | Opt-in. Lets a supported native client (such as the official Jellyfin Android TV app) be attached to a room as a **receiver** that follows the host via remote-control commands. While off, the "Add a Device to This Room" picker is hidden and the server rejects receiver requests. See [Host Bridge](technical/host-bridge). |

### JWT Secret Guidelines

For production use: minimum 32 characters, cryptographically random, never reused across environments.

```bash
openssl rand -base64 32
```

**A secret shorter than 32 characters disables authentication entirely** —
`/JellyWatchParty/Token` responds as if `JwtSecret` were empty (no token,
`auth_enabled: false`), it does not attempt to sign with a weak key. This
still includes `session_server_url` in the response, so the widget
connects normally without authentication. (Before this was fixed, a
non-empty secret under 128 bits — e.g. accidentally pasting a 10-character
value — made every call to `/JellyWatchParty/Token` fail with a 500 error,
which meant the client never learned the configured Session Server URL and
the widget got stuck showing "Offline" no matter what URL was set. If you
hit that symptom on an older version, check your Jellyfin server logs for
`IDX10653`/`ArgumentOutOfRangeException` on `GET /JellyWatchParty/Token`.)

## Session Server Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3000` | Port to listen on |
| `HOST` | `0.0.0.0` | Address to bind to |
| `ALLOWED_ORIGINS` | `http://localhost:8096,https://localhost:8096` | CORS allowed origins (comma-separated) |
| `JWT_SECRET` | (empty) | Secret for validating tokens |
| `LOG_LEVEL` | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |

```yaml
services:
  session-server:
    image: jwp-session-server
    ports:
      - "3000:3000"
    environment:
      - ALLOWED_ORIGINS=https://jellyfin.example.com
      - JWT_SECRET=${JWT_SECRET}
      - LOG_LEVEL=info
    restart: unless-stopped
```

### CORS Configuration

Specify allowed origins instead of using a wildcard (`*`) — using `*` logs a
security warning and is not recommended for production. See
[Security: CORS](security#cors-cross-origin-resource-sharing) for details.

```bash
# Single origin
ALLOWED_ORIGINS=https://jellyfin.example.com

# Multiple origins
ALLOWED_ORIGINS=https://jellyfin.example.com,http://localhost:8096
```

## Client Configuration

The client gets its configuration from the plugin automatically. If the
session server runs on a different host or port, set a custom WebSocket URL:

1. Go to **Dashboard** > **Plugins** > **JellyWatchParty**
2. Set **Session Server URL**, e.g. `wss://session.example.com/ws`
3. Save and refresh

| Scheme | When to Use |
|--------|-------------|
| `ws://` | HTTP/unencrypted (development only) |
| `wss://` | HTTPS/encrypted (production) |

The config page warns (but does not block saving) if the URL looks wrong —
e.g. `ws://` used while the value doesn't parse as a scheme it recognizes,
or a bare hostname with no dot that isn't `localhost` (a common sign of an
internal Docker/Compose service name that isn't reachable from a browser).
The same checks run again client-side on every connection attempt and are
logged to the browser console and the plugin's server log.

## Sync Tuning

Client and server sync-timing constants (drift correction thresholds,
clock-sync smoothing, scheduling delays) are not configurable at runtime,
but can be modified in source. See [Sync Algorithms: Threshold and Timing
Summary](technical/sync#threshold-and-timing-summary) for the full,
authoritative list of constants rather than repeating it here.

## Configuration Examples

### Minimal Setup (Development)

```yaml
services:
  session-server:
    image: jwp-session-server
    ports:
      - "3000:3000"
```

Plugin settings: JWT Secret empty, Session Server URL empty.

### Production Setup

```yaml
services:
  session-server:
    image: jwp-session-server
    ports:
      - "127.0.0.1:3000:3000"  # Only localhost
    environment:
      - ALLOWED_ORIGINS=https://jellyfin.example.com
      - JWT_SECRET=${JWT_SECRET}
      - LOG_LEVEL=warn
    restart: unless-stopped
```

Plugin settings: JWT Secret set to a 32+ char secret, Session Server URL set
to `wss://jellyfin.example.com/ws` (via reverse proxy).

### Multi-Instance Setup

The session server is stateful (in-memory rooms), so only **one instance**
should run per deployment — running multiple instances behind a load
balancer will split rooms across them unpredictably. For high availability
across multiple Jellyfin instances pointed at the same server, just set
`ALLOWED_ORIGINS` to include all of them:

```yaml
services:
  session-server:
    image: jwp-session-server
    environment:
      - ALLOWED_ORIGINS=https://jellyfin1.example.com,https://jellyfin2.example.com
```

## Validating Configuration

```bash
# Plugin config (requires admin API key)
curl -H "X-Emby-Token: YOUR_API_KEY" \
  "http://localhost:8096/System/Configuration/Plugin/0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d"

# Server health
curl http://localhost:3000/health

# JWT token generation
curl -H "X-Emby-Token: YOUR_API_KEY" \
  "http://localhost:8096/JellyWatchParty/Token"
```

## Next Steps

- [Security](security) - Security hardening
- [Deployment](deployment) - Production deployment
- [Troubleshooting & FAQ](troubleshooting) - Common issues
