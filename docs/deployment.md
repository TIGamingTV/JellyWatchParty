---
title: Deployment
nav_order: 8
---

# Production Deployment

## Architecture Overview

```
Internet
    │
    ▼
┌─────────────────────────────────────┐
│         Reverse Proxy               │
│    (nginx/Caddy/Traefik)           │
│    SSL termination, routing         │
└───────────┬─────────────┬───────────┘
            │             │
    ┌───────▼───────┐ ┌───▼───────────┐
    │   Jellyfin    │ │ Session Server│
    │   :8096       │ │    :3000      │
    └───────────────┘ └───────────────┘
```

## Docker Compose Deployment

```yaml
# docker-compose.prod.yml
version: '3.8'

services:
  jellyfin:
    image: jellyfin/jellyfin:latest
    container_name: jellyfin
    volumes:
      - ./config:/config
      - ./cache:/cache
      - /path/to/media:/media:ro
      - ./plugins/JellyWatchParty.dll:/config/plugins/JellyWatchParty/JellyWatchParty.dll:ro
    environment:
      - JELLYFIN_PublishedServerUrl=https://jellyfin.example.com
    restart: unless-stopped
    networks:
      - internal

  session-server:
    image: jwp-session-server
    container_name: jwp-session
    environment:
      - ALLOWED_ORIGINS=https://jellyfin.example.com
      - JWT_SECRET=${JWT_SECRET}
      - LOG_LEVEL=warn
    restart: unless-stopped
    networks:
      - internal

  caddy:
    image: caddy:2-alpine
    container_name: caddy
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    restart: unless-stopped
    networks:
      - internal

networks:
  internal:

volumes:
  caddy_data:
  caddy_config:
```

```bash
# .env
JWT_SECRET=your-very-secure-32-character-secret-key
```

## Reverse Proxy Configuration

### Caddy (Recommended)

```caddyfile
jellyfin.example.com {
    reverse_proxy jellyfin:8096

    handle /ws {
        reverse_proxy session-server:3000
    }
}
```

Caddy automatically provisions Let's Encrypt certificates for the block above.

### nginx

```nginx
upstream jellyfin {
    server jellyfin:8096;
}

upstream session-server {
    server session-server:3000;
}

server {
    listen 443 ssl http2;
    server_name jellyfin.example.com;

    ssl_certificate /etc/ssl/certs/jellyfin.crt;
    ssl_certificate_key /etc/ssl/private/jellyfin.key;

    location / {
        proxy_pass http://jellyfin;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }

    location /ws {
        proxy_pass http://session-server;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_read_timeout 86400;
    }
}
```

For Let's Encrypt with Certbot: `sudo apt install certbot python3-certbot-nginx && sudo certbot --nginx -d jellyfin.example.com` (auto-renewal is usually configured automatically via `certbot.timer`).

### Traefik

```yaml
services:
  jellyfin:
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.jellyfin.rule=Host(`jellyfin.example.com`)"
      - "traefik.http.routers.jellyfin.tls.certresolver=letsencrypt"
      - "traefik.http.services.jellyfin.loadbalancer.server.port=8096"

  session-server:
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.jwp-ws.rule=Host(`jellyfin.example.com`) && PathPrefix(`/ws`)"
      - "traefik.http.routers.jwp-ws.tls.certresolver=letsencrypt"
      - "traefik.http.services.jwp-ws.loadbalancer.server.port=3000"
```

### Cloudflare Tunnel

Cloudflare Tunnel (`cloudflared`) creates an outbound-only connection to Cloudflare's edge, so no ports need to be opened. **Important:** in the tunnel config, use the Docker service name or host IP to reach the session server — never `localhost`, which resolves to the tunnel container's own loopback.

```yaml
# ~/.cloudflared/config.yml
tunnel: your-tunnel-id
credentials-file: /path/to/credentials.json

ingress:
  - hostname: jellyfin.example.com
    service: http://jellyfin:8096
  - hostname: jwp.example.com
    service: http://session-server:3000
  - service: http_status:404
```

Set plugin **Session Server URL** to `wss://jwp.example.com/ws`. If `cloudflared` runs on the Docker host (not in a container), use the host IP or `host.docker.internal` instead of the service name.

## Deployment Hardening

- **Use internal networks** — don't expose the session server port externally; put it on the same Docker network as the reverse proxy and `expose` (not `ports`) it
- **Set the Session Server URL** in plugin settings to the reverse-proxy hostname (e.g. `wss://jellyfin.example.com/ws`) — without this, the client defaults to `ws://<current-host>:3000/ws`, which isn't reachable through a proxy
- **Use read-only volumes** for the plugin DLL and media directory
- **Drop capabilities:**
  ```yaml
  services:
    session-server:
      cap_drop: [ALL]
      read_only: true
      security_opt: [no-new-privileges:true]
  ```

See [Security](security) for authentication, CORS, and the full threat model.

## Health Checks

```yaml
services:
  session-server:
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 5s
```

{% raw %}
```bash
docker inspect --format='{{.State.Health.Status}}' session-server
```
{% endraw %}

## Logging

| Level | Use Case |
|-------|----------|
| `error` | Production (minimal) |
| `warn` | Production (recommended) |
| `info` | Debugging |
| `debug` | Development |
| `trace` | Deep debugging |

```yaml
services:
  session-server:
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
```

Forward to syslog or Loki instead of local files if you're aggregating logs centrally:

```yaml
services:
  session-server:
    logging:
      driver: loki
      options:
        loki-url: "http://loki:3100/loki/api/v1/push"
        labels: "service=jwp-session"
```

## Alerting

Simple uptime check via cron:

```bash
#!/bin/bash
if ! curl -sf http://localhost:3000/health > /dev/null; then
    echo "JellyWatchParty session server is DOWN" | mail -s "ALERT: JWP Down" admin@example.com
fi
```

```cron
*/5 * * * * /usr/local/bin/check-jwp.sh
```

For fleet-style monitoring, point [Uptime Kuma](https://github.com/louislam/uptime-kuma) at `http://session-server:3000/health`, or wire an Alertmanager rule against container `up` metrics if you already run Prometheus:

```yaml
groups:
  - name: jwp
    rules:
      - alert: JWPSessionServerDown
        expr: up{job="session-server"} == 0
        for: 1m
        labels:
          severity: critical
```

The session server doesn't currently expose Prometheus metrics itself — monitor container resource usage (`docker stats session-server`, or cAdvisor) and connection/room counts from logs in the meantime.

## Capacity Planning

| Metric | Per Client | Per Room |
|--------|------------|----------|
| Memory | ~1 KB | ~5 KB |
| CPU | Minimal | Minimal |
| Bandwidth | ~1 KB/s | ~10 KB/s |

**Current limitations:** single instance (stateful, in-memory) — see [Configuration: Multi-Instance Setup](configuration#multi-instance-setup) for why you shouldn't run more than one.

**Connection limits:** WebSocket connections are bounded by OS file-descriptor limits (default 1024) — raise with `ulimit -n 65535`, or in Docker:

```yaml
services:
  session-server:
    ulimits:
      nofile:
        soft: 65535
        hard: 65535
```

## Backup Strategy

**Back up:** Jellyfin `/config` directory (includes plugin config), Docker Compose files, `.env` files with secrets.

**Don't back up:** session server state (ephemeral, in-memory) or cache directories.

```bash
#!/bin/bash
BACKUP_DIR=/backup/jellyfin
DATE=$(date +%Y%m%d)
docker compose stop jellyfin
tar -czf $BACKUP_DIR/config-$DATE.tar.gz ./config
docker compose start jellyfin
```

## Upgrade Procedure

1. **Backup first:** `docker compose stop && tar -czf backup-before-upgrade.tar.gz ./config`
2. **Pull new images:** `docker compose pull`
3. **Update plugin:** download the new `.dll` from [Releases](https://github.com/TIGamingTV/JellyWatchParty/releases) and replace it in your plugins volume
4. **Restart:** `docker compose up -d`
5. **Verify:** check Dashboard for plugin status, test Watch Party functionality, check logs for errors

## Troubleshooting a Deployment

If the health check is failing: confirm the container is running (`docker ps`), check its logs (`docker logs session-server`), and test from inside the container (`docker exec session-server curl localhost:3000/health`). For everything else, see [Troubleshooting & FAQ](troubleshooting).
