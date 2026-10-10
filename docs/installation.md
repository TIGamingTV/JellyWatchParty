---
title: Installation
nav_order: 2
---

# Installation

JellyWatchParty has two parts to install: the **session server** (a small
standalone process that manages rooms) and the **Jellyfin plugin** (which
serves the UI and talks to the session server). Both are required.

## Prerequisites

- **Jellyfin Server** 12.x (10.11.x is no longer supported — install an older
  JellyWatchParty release if you're still on it)
- **Port 3000** available for the session server (or any port you choose)
- Admin access to Jellyfin

## Quick Start (Docker Compose)

The fastest way to get running. The [admin panel]({{ '/admin-panel/' | relative_url }})
and the [Discord bot]({{ '/discord-bot/' | relative_url }}) are optional:
the panel only starts when `ADMIN_PASSWORD` is set, and the bot only with the
`discord` profile.

```yaml
# docker-compose.yml
services:
  jwp-session:
    image: ghcr.io/tigamingtv/jwp-session-server:${JWP_TAG:-latest}
    container_name: jwp-session
    restart: unless-stopped
    ports:
      - "3000:3000"
      # Admin panel; only starts when ADMIN_PASSWORD is set.
      - "127.0.0.1:3001:3001"
      # Never publish 3002 (integration API for the chat bots).
    environment:
      - ALLOWED_ORIGINS=http://your-jellyfin:8096
      - JWT_SECRET=${JWT_SECRET:-}            # same value as in the plugin settings
      - ADMIN_PASSWORD=${ADMIN_PASSWORD:-}
      # Jellyfin devices (TV apps, Fladder, ...) in the admin panel and the bot
      - JELLYFIN_URL=${JELLYFIN_URL:-}        # as seen from this container
      - JELLYFIN_API_KEY=${JELLYFIN_API_KEY:-}
      # Chat bots: settings and link codes are kept in /data
      - DATA_DIR=/data
      - INTEGRATION_HOST=0.0.0.0
      - DISCORD_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN:-}
      - TELEGRAM_INTEGRATION_TOKEN=${TELEGRAM_INTEGRATION_TOKEN:-}
    volumes:
      - jwp-data:/data

  # Optional chat bots (Discord and/or Telegram, one container):
  # docker compose --profile discord up -d   (or --profile telegram)
  jwp-discord-bot:
    image: ghcr.io/tigamingtv/jwp-discord-bot:${JWP_TAG:-latest}
    container_name: jwp-discord-bot
    restart: unless-stopped
    profiles: [discord, telegram]
    depends_on: [jwp-session]
    environment:
      - JWP_INTEGRATION_URL=http://jwp-session:3002
      - DISCORD_BOT_TOKEN=${DISCORD_BOT_TOKEN:-}
      - DISCORD_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN:-}
      - TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN:-}
      - TELEGRAM_INTEGRATION_TOKEN=${TELEGRAM_INTEGRATION_TOKEN:-}

volumes:
  jwp-data:
```

```bash
# .env next to docker-compose.yml
JWT_SECRET=<openssl rand -base64 32>
ADMIN_PASSWORD=<openssl rand -base64 18>
JELLYFIN_URL=http://your-jellyfin:8096
JELLYFIN_API_KEY=<Dashboard > API Keys>
# Only for the Discord bot:
DISCORD_INTEGRATION_TOKEN=<openssl rand -hex 32>
DISCORD_BOT_TOKEN=<Discord Developer Portal > Bot > Reset Token>
# Only for the Telegram bot (a different random value):
TELEGRAM_INTEGRATION_TOKEN=<openssl rand -hex 32>
TELEGRAM_BOT_TOKEN=<@BotFather > /newbot>
# Image channel for both containers: latest (default), dev, beta or 1.2.3
# JWP_TAG=latest
```

```bash
docker compose up -d                     # session server only
docker compose --profile discord up -d   # session server + chat bot(s)
```

Then install the plugin via the [repository method](#plugin-install) below,
and enable the [client script](#enable-the-client-script).

## Session Server Install Options

Pick whichever fits your environment. All options end up running the same
server; only how you get it running differs.

### Docker: Pre-built Image (Recommended)

```bash
# Latest stable release
docker run -d \
  --name jwp-session \
  -p 3000:3000 \
  -e ALLOWED_ORIGINS="http://localhost:8096" \
  ghcr.io/tigamingtv/jwp-session-server:latest

# Or a specific version (release v0.1.0)
docker run -d --name jwp-session -p 3000:3000 \
  ghcr.io/tigamingtv/jwp-session-server:0.1.0

# Or the beta channel (latest build from main)
docker run -d --name jwp-session -p 3000:3000 \
  ghcr.io/tigamingtv/jwp-session-server:beta

# Or the develop channel (latest build from develop, for testing)
docker run -d --name jwp-session -p 3000:3000 \
  ghcr.io/tigamingtv/jwp-session-server:dev
```

The Discord bot is published the same way as
`ghcr.io/tigamingtv/jwp-discord-bot`, with the same tags. Run it on the same
tag as the server.

### Docker: Build from Source

```bash
docker build -f infra/docker/server.Dockerfile --build-arg BUILD_MODE=release \
  -t jwp-session-server ./src/server

docker run -d \
  --name jwp-session \
  -p 3000:3000 \
  -e ALLOWED_ORIGINS="http://localhost:8096" \
  jwp-session-server
```

### Native Linux (Build from Source)

Requires Rust 1.83+:

```bash
cd src/server
cargo build --release
./target/release/session-server
```

### Windows Server (Prebuilt Binary, No Install Required)

No Docker, no Rust, nothing to install — download a ready-to-run binary:

1. Go to [Releases](https://github.com/TIGamingTV/JellyWatchParty/releases)
   and download `jwp-session-server-windows-vX.Y.Z.zip` from the latest release
2. Extract it anywhere on the Windows Server host
3. (Optional) Set configuration via environment variables before launching,
   e.g. in PowerShell:
   ```powershell
   $env:PORT = "3000"
   $env:ALLOWED_ORIGINS = "http://your-jellyfin:8096"
   $env:JWT_SECRET = "<32+ char secret, must match the Jellyfin plugin config>"
   ```
4. Run `session-server.exe`
5. Allow it through Windows Firewall when prompted so Jellyfin and clients
   can reach it on the configured port (default `3000`)

To keep it running in the background as a Windows service, wrap it with
[NSSM](https://nssm.cc/) or Task Scheduler pointed at `session-server.exe`.

## Enable the Client Script

The session server alone doesn't do anything — Jellyfin's web UI needs a
small script injected so the Watch Party button and panel appear.

### Option A: Automatic Injection (Recommended)

Install [jellyfin-plugin-file-transformation](https://github.com/IAmParadox27/jellyfin-plugin-file-transformation)
and restart Jellyfin. JellyWatchParty automatically registers a transformation
that injects the client script into `index.html` — no configuration needed.

### Option B: Manual (Custom HTML)

1. Log in to Jellyfin as an administrator
2. Go to **Dashboard** > **General**
3. Scroll to **Custom HTML** (Branding section)
4. Add this line to the "Custom HTML body" field:
   ```html
   <script src="../JellyWatchParty/ClientScript"></script>
   ```
5. Click **Save**
6. Hard refresh your browser (Ctrl+F5)

## Plugin Install

### Option A: Via Jellyfin Plugin Repository (Recommended) {#plugin-install}

1. Go to **Dashboard** > **Plugins** > **Repositories**
2. Click **Add** and enter:
   ```
   https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json
   ```
3. Go to the **Catalog** tab
4. Find **JellyWatchParty** and click **Install**
5. Restart Jellyfin
6. Enable the [client script](#enable-the-client-script) if you haven't already

This method provides automatic update notifications when new versions are
released. Testers who want the develop/beta channel instead can use
`manifest-dev.json` in the same way — see [Release: Develop Plugin
Channel]({{ '/development/release/' | relative_url }}#develop-plugin-channel).

### Option B: Manual Download

1. Download the latest release zip (`JellyWatchParty-vX.Y.Z.zip`) from the
   [releases page](https://github.com/TIGamingTV/JellyWatchParty/releases)
2. Extract it to your Jellyfin plugins directory:
   ```bash
   # Linux (Docker)
   unzip JellyWatchParty-v0.1.0.zip -d /tmp/jwp
   docker cp /tmp/jwp/. jellyfin:/config/plugins/JellyWatchParty/

   # Linux (native)
   sudo unzip JellyWatchParty-v0.1.0.zip -d /var/lib/jellyfin/plugins/JellyWatchParty/

   # Windows
   # Extract to: C:\ProgramData\Jellyfin\Server\plugins\JellyWatchParty\
   ```
3. Restart Jellyfin (`docker restart jellyfin` or `sudo systemctl restart jellyfin`)
4. Enable the [client script](#enable-the-client-script)

### Configure the Plugin (Optional)

1. Go to **Dashboard** > **Plugins** > **JellyWatchParty**
2. Set a JWT Secret (min 32 characters) for authentication
3. Click **Save**

See [Configuration]({{ '/configuration/' | relative_url }}) for the full settings reference.

## Verification

**Check the session server:**

```bash
curl http://localhost:3000/health
# Expected: 200 OK with "OK"
```

**Check the plugin:**

1. Go to **Dashboard** > **Plugins** — "JellyWatchParty" should appear in the list
2. Check the logs for a startup message:
   ```
   [JellyWatchParty] JWT authentication is enabled.
   ```
   or
   ```
   [JellyWatchParty] JwtSecret is not configured. Authentication is DISABLED.
   ```

**Test the UI:**

1. Open any video in Jellyfin
2. Look for the Watch Party button (group icon) in the top header
3. Click it to open the panel

## Environment Variables

This is the core list. The admin panel and Jellyfin device variables are in
the [Admin Panel]({{ '/admin-panel/' | relative_url }}#environment-variables) reference, the Discord bot
variables are in the [Discord Bot]({{ '/discord-bot/' | relative_url }}#2-configure-the-containers) guide, and
[`.env.example`](https://github.com/TIGamingTV/JellyWatchParty/blob/main/.env.example) lists all of them.

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3000` | Server port |
| `HOST` | `0.0.0.0` | Bind address |
| `ALLOWED_ORIGINS` | `*` | CORS allowed origins (comma-separated) |
| `JWT_SECRET` | (none) | JWT secret for authentication |
| `RUST_LOG` | `info` | Logging level |

```bash
docker run -d \
  -p 3000:3000 \
  -e ALLOWED_ORIGINS="https://jellyfin.example.com" \
  -e JWT_SECRET="your-32-character-secret-key-here" \
  -e RUST_LOG="debug" \
  ghcr.io/tigamingtv/jwp-session-server:latest
```

## Firewall Configuration

| Port | Service | Direction |
|------|---------|-----------|
| 8096 | Jellyfin HTTP | Inbound |
| 8920 | Jellyfin HTTPS | Inbound (if using SSL) |
| 3000 | Session Server | Inbound |
| 3001 | Admin panel | Only if you use it; keep it private (see [Admin Panel]({{ '/admin-panel/' | relative_url }})) |
| 3002 | Discord integration API | Never publish. Stays inside the Docker network |

```bash
# UFW (Ubuntu)
sudo ufw allow 8096/tcp
sudo ufw allow 3000/tcp

# firewalld (Fedora/CentOS)
sudo firewall-cmd --permanent --add-port=8096/tcp
sudo firewall-cmd --permanent --add-port=3000/tcp
sudo firewall-cmd --reload
```

## Next Steps

- [Configuration]({{ '/configuration/' | relative_url }}) - Configure JWT, CORS, and sync tuning
- [Security]({{ '/security/' | relative_url }}) - Set up authentication and hardening
- [Deployment]({{ '/deployment/' | relative_url }}) - Production deployment behind a reverse proxy
- [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }}) - If something isn't working
