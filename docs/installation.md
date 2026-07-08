---
title: Installation
nav_order: 2
---

# Installation

JellyWatchParty has two parts to install: the **session server** (a small
standalone process that manages rooms) and the **Jellyfin plugin** (which
serves the UI and talks to the session server). Both are required.

## Prerequisites

- **Jellyfin Server** 10.11.x
- **Port 3000** available for the session server (or any port you choose)
- Admin access to Jellyfin

## Quick Start (Docker Compose)

The fastest way to get running:

```yaml
# docker-compose.yml
services:
  jwp-session:
    image: ghcr.io/tigamingtv/jwp-session-server:latest
    container_name: jwp-session
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - ALLOWED_ORIGINS=http://your-jellyfin:8096
```

```bash
docker compose up -d
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

# Or a specific version
docker run -d --name jwp-session -p 3000:3000 \
  ghcr.io/tigamingtv/jwp-session-server:v0.1.0

# Or the beta channel (latest build from main)
docker run -d --name jwp-session -p 3000:3000 \
  ghcr.io/tigamingtv/jwp-session-server:beta
```

### Docker: Build from Source

```bash
docker build -t jwp-session-server ./src/server

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
Channel](development/release#develop-plugin-channel).

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

See [Configuration](configuration) for the full settings reference.

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

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3000` | Server port |
| `HOST` | `0.0.0.0` | Bind address |
| `ALLOWED_ORIGINS` | `*` | CORS allowed origins (comma-separated) |
| `JWT_SECRET` | (none) | JWT secret for authentication |
| `LOG_LEVEL` | `info` | Logging level |

```bash
docker run -d \
  -p 3000:3000 \
  -e ALLOWED_ORIGINS="https://jellyfin.example.com" \
  -e JWT_SECRET="your-32-character-secret-key-here" \
  -e LOG_LEVEL="debug" \
  ghcr.io/tigamingtv/jwp-session-server:latest
```

## Firewall Configuration

| Port | Service | Direction |
|------|---------|-----------|
| 8096 | Jellyfin HTTP | Inbound |
| 8920 | Jellyfin HTTPS | Inbound (if using SSL) |
| 3000 | Session Server | Inbound |

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

- [Configuration](configuration) - Configure JWT, CORS, and sync tuning
- [Security](security) - Set up authentication and hardening
- [Deployment](deployment) - Production deployment behind a reverse proxy
- [Troubleshooting & FAQ](troubleshooting) - If something isn't working
