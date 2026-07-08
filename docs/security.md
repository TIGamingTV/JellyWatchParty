---
title: Security
nav_order: 9
---

# Security

JellyWatchParty includes several security features: JWT authentication,
CORS protection, rate limiting, and input validation. This page covers how
they work, their limits, and hardening recommendations. For firewall/network
topology, see [Deployment](deployment).

## Authentication

1. User authenticates with Jellyfin
2. Client requests a JWT token from the plugin
3. Client sends the token to the session server
4. Server validates the token before allowing actions

### Enabling Authentication

```bash
openssl rand -base64 32
```

Set the result as **JWT Secret** in **Dashboard > Plugins > JellyWatchParty**,
and as `JWT_SECRET` in the session server's environment — both must use the
**same secret**:

```yaml
services:
  session-server:
    environment:
      - JWT_SECRET=K7xR9mPqN2wLhVbE4cT8fY0jU5sA3dG1
```

### Token Structure

| Claim | Description |
|-------|-------------|
| `sub` | Jellyfin user ID |
| `name` | User display name |
| `aud` | Audience (configurable) |
| `iss` | Issuer (configurable) |
| `iat` | Issued at timestamp |
| `exp` | Expiration timestamp |

Default lifetime: 1 hour (3600s), configurable 60s-86400s (24h) in plugin settings.

## CORS (Cross-Origin Resource Sharing)

CORS prevents unauthorized websites from connecting to your session server.

```yaml
environment:
  - ALLOWED_ORIGINS=https://jellyfin.example.com          # recommended: single origin
  # - ALLOWED_ORIGINS=https://a.example.com,https://b.example.com  # multiple origins
  # - ALLOWED_ORIGINS=*                                     # NOT recommended for production
```

Using `*` logs `SECURITY: Wildcard origin (*) configured - ALL origins allowed!` — any website could then connect to your session server.

## Rate Limiting

| Limit | Value | Scope |
|-------|-------|-------|
| Token requests | 10/min | Per authenticated Jellyfin user (plugin endpoint) |
| WebSocket messages | 30/sec | Per WebSocket connection (client UUID) |
| Message size | 64 KB | Per message |

**Important — rate limiting is per-client, not per-IP.** The session server limits by WebSocket client UUID, not IP address, so an attacker could open multiple connections to bypass it. This is because the server often sits behind a reverse proxy without direct IP visibility, and JWT authentication covers most abuse cases. For production, add IP-based rate limiting at the reverse proxy:

```nginx
limit_req_zone $binary_remote_addr zone=ws_limit:10m rate=10r/s;

location /ws {
    limit_req zone=ws_limit burst=20 nodelay;
    proxy_pass http://session-server:3000;
}
```

## HTTPS/WSS

Encrypted connections protect JWT tokens from interception and prevent
man-in-the-middle attacks — required for production. Configure a reverse
proxy with SSL (see [Deployment](deployment)) and set the plugin's Session
Server URL to `wss://jellyfin.example.com/ws`.

The session server validates certificates by default; for self-signed
certificates (development only) you may need to disable validation in the
client or add the CA to the trust store.

## Input Validation

Image URLs are restricted to `http(s)` schemes to block `javascript:`/`data:`
XSS vectors. The server also validates message type, room existence, host
permissions, and payload structure on every incoming message.

## Security Best Practices

**Production checklist:**
- [ ] JWT authentication enabled with a strong secret (32+ characters)
- [ ] CORS restricted to specific origins
- [ ] HTTPS enabled (via reverse proxy)
- [ ] Session server not directly exposed to the internet
- [ ] Regular updates applied, logs monitored for suspicious activity

**Secrets:** use environment variables and `.env` files (not committed to git), rotate periodically, never share across environments or hardcode in config files.

## Threat Model

| Threat | Mitigation |
|--------|------------|
| Unauthorized access | JWT authentication |
| Token theft | Short expiration, HTTPS |
| Cross-site attacks | CORS validation, URL sanitization |
| Denial of service | Rate limiting, message size limits |
| Man-in-the-middle | HTTPS/WSS encryption |

**Known, by-design limitations:**

| Limitation | Notes |
|------------|-------|
| Room passwords are a fast salted SHA-256 hash, not a slow KDF | Rooms are in-memory/ephemeral, so there's no persisted hash database to protect against offline cracking (see `src/server/src/password.rs`) |
| Ephemeral sessions | No persistence by design |
| Single secret for all users | No per-user JWT secrets |
| Rate limiting per client, not IP | Use reverse-proxy rate limiting |
| No token revocation | Short TTL + secret rotation instead |

## What JWT Authentication Does NOT Protect

| Scenario | Current Behavior | Mitigation |
|----------|------------------|------------|
| Room creation | Any authenticated user can create rooms, optionally with a password | By design — all Jellyfin users are trusted |
| Room joining | Any authenticated user can join any room; password-protected rooms require the correct password | Set a room password for private rooms |
| Room enumeration | All users see all active rooms (and whether each is password-protected) | By design — rooms are public within your Jellyfin instance |
| Token revocation | Tokens valid until expiration | Rotate the JWT secret to invalidate all tokens at once |

JWT authentication operates on a trust boundary at the **Jellyfin level**: if
a user can log into Jellyfin, they can use JellyWatchParty. There's no
additional access-control layer within JellyWatchParty itself — restrict
Jellyfin access (or set room passwords) to control who can use watch parties,
and use Jellyfin's library permissions to control media access for sensitive
content.

## Incident Response

**If the JWT secret is compromised:** change it immediately on both the
plugin and server, restart all services — all existing tokens become
invalid and users must re-authenticate.

**If suspicious activity is detected:** check logs, consider temporarily
disabling the service, review CORS/authentication settings, and update to
the latest version.

## Container Security

The session server uses **Alpine Linux** (~26MB, ~6 low-severity CVEs) rather
than a Debian base (~100MB, 30+ CVEs) for a minimal attack surface. Images
are scanned on every push with **Trivy** (CVEs, uploaded to the GitHub
Security tab) — remaining known CVEs (e.g. BusyBox `netstat`/`tar`) affect
tools the application doesn't use and require local access to exploit.

Runtime hardening already in place: non-root user (`appuser`, UID 1000),
health checks with automatic restart. For maximum hardening in sensitive
environments, consider a distroless or scratch-based image (requires a
custom healthcheck binary / static binary compilation).

```yaml
services:
  session-server:
    user: "1000:1000"
    read_only: true
    security_opt:
      - no-new-privileges:true
    deploy:
      resources:
        limits:
          memory: 256M
          cpus: '0.5'
```

## Reporting Security Issues

If you discover a security vulnerability, **do not** open a public issue —
email the maintainer directly with a description, reproduction steps, and
potential impact.

## Next Steps

- [Deployment](deployment) - Production deployment
- [Configuration](configuration) - Server and plugin settings
- [Troubleshooting & FAQ](troubleshooting) - Common problems
