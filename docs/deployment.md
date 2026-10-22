---
title: Deployment
nav_order: 8
---

# Deployment and Operations

This page covers running JellyWatchParty in production: HTTPS through a
reverse proxy, hardening, and day-to-day operations (logs, updates,
backups). Start with [Installation]({{ '/installation/' | relative_url }}).

## Behind a Reverse Proxy

When Jellyfin is served over HTTPS, browsers only allow secure WebSocket
(`wss://`) connections, so the session server must be behind the same kind
of proxy. The usual setup routes `/ws` on the Jellyfin host name to the
session server:

```
Browser ──https/wss──> reverse proxy ──> Jellyfin        :8096   (everything else)
                                    └──> session server  :3000   (/ws only)
```

Then:

1. Set the plugin's **Session Server URL** to `wss://jellyfin.example.com/ws`.
2. Set `ALLOWED_ORIGINS=https://jellyfin.example.com` on the session server.
3. Stop publishing port 3000 on the host if only the proxy needs it: put the
   proxy and the session server on the same Docker network and remove the
   `3000:3000` mapping.

The examples assume the proxy reaches the containers as `jellyfin` and
`session-server` on a shared Docker network. Never route `/ws` (or anything
public) to port 3001 or 3002.

### Caddy

```caddyfile
jellyfin.example.com {
    handle /ws {
        reverse_proxy session-server:3000
    }
    handle {
        reverse_proxy jellyfin:8096
    }
}
```

### nginx

Inside the `server` block of your Jellyfin site:

```nginx
location = /ws {
    proxy_pass http://session-server:3000;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_read_timeout 1h;
}
```

### Traefik

```yaml
services:
  session-server:
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.jwp-ws.rule=Host(`jellyfin.example.com`) && Path(`/ws`)"
      - "traefik.http.routers.jwp-ws.tls.certresolver=letsencrypt"
      - "traefik.http.services.jwp-ws.loadbalancer.server.port=3000"
```

### Separate host name

You can also give the session server its own host name (for example
`jwp.example.com` proxied to port 3000, or a Cloudflare Tunnel ingress to
`http://session-server:3000`). Set the Session Server URL to
`wss://jwp.example.com/ws`. `ALLOWED_ORIGINS` stays the address of
**Jellyfin**, since that is the page the browser connects from.

### Admin panel through a proxy

Give the admin panel its own host name and HTTPS, and set
`ADMIN_COOKIE_SECURE=true`; see
[Admin Panel: Behind a reverse proxy]({{ '/admin-panel/' | relative_url }}#behind-a-reverse-proxy).

## Hardening

- Enable authentication: the same 32+ character secret in `JWT_SECRET` and
  the plugin's **JWT Secret**.
- List only your Jellyfin address(es) in `ALLOWED_ORIGINS`; don't use `*`.
- Keep port 3001 (admin panel) on `127.0.0.1`, the LAN, or behind HTTPS, or
  set `ADMIN_ENABLED=false` if you don't use it. Keep port 3002 unpublished.
- The images run as an unprivileged user (UID 1000). The session server only
  writes to `DATA_DIR`, so you can also drop capabilities and make the root
  filesystem read-only:

  ```yaml
  services:
    session-server:
      cap_drop: [ALL]
      security_opt: ["no-new-privileges:true"]
      read_only: true
  ```

- The session server limits each connection to 30 messages per second, but
  not per IP address. Add rate limiting at the proxy if it is exposed to the
  internet.

See [Security]({{ '/security/' | relative_url }}) for the security model.

## Operations

Run these in the folder that contains `compose.yaml`.

| Task | Command |
|------|---------|
| Start (in the background) | `docker compose up -d` (add `--profile discord` to include the chat bot) |
| Follow logs | `docker compose logs -f session-server` (bot: `discord-bot`) |
| Status and health | `docker compose ps` and `curl http://localhost:3000/health` |
| Restart | `docker compose restart session-server` |
| Apply changes to `.env` or `compose.yaml` | `docker compose up -d` (a restart keeps the old environment) |
| Stop | `docker compose down` (the `jwp-data` volume is kept) |

The images have a built-in health check (`/health` every 30 seconds), shown
as `healthy` in `docker compose ps`.

Restarting the session server ends every room and signs admins out of the
admin panel; participants drop back to the lobby and have to create a new
room. Plan restarts and updates outside watch parties.

### What is persisted

| Data | Where | Survives a restart |
|------|-------|--------------------|
| Rooms, chat, admin sessions, bot activity log | Session server memory | No |
| Chat bot settings, link codes, linked accounts | `DATA_DIR` (`jwp-data` volume): `integrations.json`, `secret.key` | Yes |
| Plugin settings | Jellyfin's configuration directory | Yes |

### Updating

The `latest` image tag always points to the latest release. Update the
plugin and the images together.

1. Back up the `jwp-data` volume if you use the chat bots (below).
2. Update the session server and bot:
   ```bash
   docker compose pull
   docker compose up -d          # add --profile discord if you use the bot
   ```
3. Update the plugin in Jellyfin (**Dashboard > Plugins**, install the
   offered update), then restart Jellyfin.
4. Reload Jellyfin in the browser (Ctrl+F5) and check that the panel shows
   **Online**.

**Pinning a version:** from v4.1.5.0 on, each release is also tagged with
its version (`4.1.5.0`) and its minor version (`4.1`). Set `JWP_TAG=4.1.5.0`
in `.env` to stay on that release, and keep the plugin on the same version;
change the tag to update. Older releases only have `latest`; to stay on one
of those, pin the image digest, which you can look up with:

{% raw %}
```bash
docker inspect --format '{{index .RepoDigests 0}}' ghcr.io/tigamingtv/jwp-session-server:latest
```
{% endraw %}

**Windows binary:** stop `session-server.exe`, replace it with the one from
the new release, and start it again.

### Backing up

Only the `jwp-data` volume needs a backup (and your `compose.yaml` and
`.env`). Keep `secret.key` with `integrations.json`: without it, every link
code stops matching and you have to assign new codes.

```bash
docker volume ls | grep jwp-data     # the volume is named <folder>_jwp-data
docker run --rm -v <folder>_jwp-data:/data -v "$PWD":/backup alpine \
  tar czf /backup/jwp-data.tgz -C /data .
```

### Removing

1. In Jellyfin, uninstall **JellyWatchParty** under **Dashboard > Plugins**
   and restart Jellyfin. The plugin removes the script it added to the web
   page.
2. Run `docker compose down`. Add `-v` only if you also want to delete the
   `jwp-data` volume (chat bot settings and links).
