---
title: Installation
nav_order: 2
---

# Installation

JellyWatchParty has two required parts:

1. the **session server**, a small standalone program that keeps track of
   rooms and keeps players in sync (Docker image or Windows binary);
2. the **Jellyfin plugin**, which adds the Watch Party panel to Jellyfin's
   web interface and signs users in to the session server.

The admin panel, Jellyfin devices (TV apps) and the Discord/Telegram bots
are optional and run inside the session server or next to it.

## Requirements

- **Jellyfin 12.x** and an administrator account. Current releases don't
  support 10.11.x; on 10.11, install an older release from the
  [releases page](https://github.com/TIGamingTV/JellyWatchParty/releases)
  whose plugin repository entry has `targetAbi` `10.11.11.0`.
- **Docker with Docker Compose** (recommended), or a Windows machine for the
  prebuilt Windows binary. Images are published for `linux/amd64` and
  `linux/arm64`.
- **Port 3000** reachable from your users' browsers (directly, or through a
  reverse proxy; see [Deployment]({{ '/deployment/' | relative_url }})).

## 1. Run the Session Server

### Docker Compose (recommended)

Create a folder (for example `jellywatchparty/`) with these two files.

`compose.yaml`:

```yaml
services:
  session-server:
    image: ghcr.io/tigamingtv/jwp-session-server:${JWP_TAG:-latest}
    container_name: jwp-session-server
    restart: unless-stopped
    ports:
      - "3000:3000"            # WebSocket server: must be reachable from browsers
      - "127.0.0.1:3001:3001"  # Admin panel: this machine only
      # Never publish 3002 (internal API for the chat bot).
    environment:
      ALLOWED_ORIGINS: ${ALLOWED_ORIGINS:?set ALLOWED_ORIGINS in .env}
      JWT_SECRET: ${JWT_SECRET:?set JWT_SECRET in .env}
      # Admin panel: only starts when ADMIN_PASSWORD is set
      ADMIN_USERNAME: ${ADMIN_USERNAME:-admin}
      ADMIN_PASSWORD: ${ADMIN_PASSWORD:-}
      # Jellyfin devices (TV apps, Fladder, ...); needed by the chat bots
      JELLYFIN_URL: ${JELLYFIN_URL:-}
      JELLYFIN_API_KEY: ${JELLYFIN_API_KEY:-}
      # Chat bots: settings and link codes are stored in /data
      DATA_DIR: /data
      INTEGRATION_HOST: 0.0.0.0
      DISCORD_INTEGRATION_TOKEN: ${DISCORD_INTEGRATION_TOKEN:-}
      TELEGRAM_INTEGRATION_TOKEN: ${TELEGRAM_INTEGRATION_TOKEN:-}
      RUST_LOG: ${RUST_LOG:-info}
    volumes:
      - jwp-data:/data

  # Optional Discord and/or Telegram bot (one container for both).
  # Starts only with: docker compose --profile discord up -d
  discord-bot:
    image: ghcr.io/tigamingtv/jwp-discord-bot:${JWP_TAG:-latest}
    container_name: jwp-discord-bot
    restart: unless-stopped
    profiles: [discord, telegram]
    depends_on: [session-server]
    environment:
      JWP_INTEGRATION_URL: http://session-server:3002
      DISCORD_BOT_TOKEN: ${DISCORD_BOT_TOKEN:-}
      DISCORD_INTEGRATION_TOKEN: ${DISCORD_INTEGRATION_TOKEN:-}
      TELEGRAM_BOT_TOKEN: ${TELEGRAM_BOT_TOKEN:-}
      TELEGRAM_INTEGRATION_TOKEN: ${TELEGRAM_INTEGRATION_TOKEN:-}
      RUST_LOG: ${RUST_LOG:-info}

volumes:
  jwp-data:
```

`.env` (keep it private; replace every `<...>`):

```bash
# Required
# The address your users open Jellyfin at, exactly as in the browser's
# address bar (scheme, host, port; no trailing slash). Comma-separate several.
ALLOWED_ORIGINS=http://192.168.1.10:8096
# Shared secret; enter the same value in the plugin settings (step 3).
# Generate with: openssl rand -base64 32
JWT_SECRET=<32+ random characters>

# Optional: admin panel (http://localhost:3001 on this machine)
ADMIN_USERNAME=admin
ADMIN_PASSWORD=<openssl rand -base64 18>

# Optional: Jellyfin devices. URL as seen from the container (not localhost);
# API key from Jellyfin Dashboard > API Keys.
JELLYFIN_URL=http://192.168.1.10:8096
JELLYFIN_API_KEY=<Jellyfin API key>

# Optional: chat bots. Each integration token: openssl rand -hex 32 (different values)
DISCORD_INTEGRATION_TOKEN=
DISCORD_BOT_TOKEN=
TELEGRAM_INTEGRATION_TOKEN=
TELEGRAM_BOT_TOKEN=

# Image tag: latest (releases, default), beta (main) or dev (develop)
# JWP_TAG=latest
```

Write values without quotes. If a value contains `$`, write it as `$$`.

Start it from that folder:

```bash
docker compose up -d                       # session server
docker compose logs -f session-server      # watch the startup log (Ctrl+C to stop)
curl http://localhost:3000/health          # {"status":"ok","auth_enabled":true}
```

The startup log lists what is enabled (`JWT: ENABLED`, the allowed origins,
the admin panel, Jellyfin devices, chat integrations) and why something is
off. Every setting is described in the
[Configuration Reference]({{ '/configuration/' | relative_url }}).

Rooms are kept in memory; the `jwp-data` volume only holds chat bot
settings and link codes.

### Docker without Compose

```bash
docker run -d --name jwp-session-server --restart unless-stopped \
  -p 3000:3000 \
  -e ALLOWED_ORIGINS="http://192.168.1.10:8096" \
  -e JWT_SECRET="<32+ random characters>" \
  ghcr.io/tigamingtv/jwp-session-server:latest
```

### Windows (prebuilt binary)

1. Download `jwp-session-server-windows-<version>.zip` from the latest
   [release](https://github.com/TIGamingTV/JellyWatchParty/releases/latest)
   and extract it.
2. Set the configuration as environment variables, e.g. in PowerShell in the
   same window you start it from:
   ```powershell
   $env:ALLOWED_ORIGINS = "http://192.168.1.10:8096"
   $env:JWT_SECRET = "<32+ random characters>"
   $env:ADMIN_PASSWORD = "<long random password>"   # optional
   .\session-server.exe
   ```
3. Allow it through Windows Firewall when asked (port 3000).

To keep it running in the background, run it as a service with a wrapper
such as [NSSM](https://nssm.cc/). The chat bot has no Windows build; run it
with Docker if you need it.

### Build from source

See [Development Setup]({{ '/development/setup/' | relative_url }}). In short:
`cargo build --release` in `src/server`, then run
`target/release/session-server` with the environment variables above.

## 2. Install the Jellyfin Plugin

### From the plugin repository (recommended)

1. In Jellyfin, open **Dashboard > Plugins** and go to the plugin
   repositories (the **Repositories** tab or **Manage repositories**,
   depending on your Jellyfin version).
2. Add a repository with any name (e.g. `JellyWatchParty`) and this URL:

   ```
   https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json
   ```

3. Open the **Catalog**, find **JellyWatchParty** and click **Install**.
4. Restart Jellyfin (**Dashboard > Restart**, or restart its container or
   service).
5. Check **Dashboard > Plugins**: JellyWatchParty is listed as active.

Jellyfin then offers new versions as updates.

### Manual install

1. Download `JellyWatchParty-v<version>.zip` from the
   [releases page](https://github.com/TIGamingTV/JellyWatchParty/releases).
2. Extract its files into a new `JellyWatchParty` folder inside Jellyfin's
   `plugins` directory, for example:
   - Docker: `/config/plugins/JellyWatchParty/` inside the Jellyfin container
   - Linux package: `/var/lib/jellyfin/plugins/JellyWatchParty/`
   - Windows: `%ProgramData%\Jellyfin\Server\plugins\JellyWatchParty\`
3. Restart Jellyfin.

Manual installs don't receive update notifications.

## 3. Configure the Plugin

1. Open **Dashboard > Plugins > JellyWatchParty**.
2. **JWT Secret**: paste the same value as `JWT_SECRET`.
3. **Session Server URL**: leave it empty if Jellyfin and the session server
   run on the same host and users can reach port 3000 on it; the browser
   then connects to `ws://<Jellyfin host>:3000/ws`. Otherwise enter the URL
   browsers should use, e.g. `ws://192.168.1.20:3000/ws`, or
   `wss://jellyfin.example.com/ws` behind an HTTPS reverse proxy.
4. Click **Save**.

The other settings (democratic mode, hiding SyncPlay, panel bridging) are
optional; see [Plugin Settings]({{ '/configuration/' | relative_url }}#plugin-settings).

## 4. Check That the Watch Party UI Loads

The plugin adds its script to Jellyfin's web page automatically:

- **Recommended:** install the
  [File Transformation](https://github.com/IAmParadox27/jellyfin-plugin-file-transformation)
  plugin (follow its README to add its repository), then restart Jellyfin.
  JellyWatchParty registers with it at startup, which works alongside other
  plugins that modify the web page.
- **Without it**, JellyWatchParty patches `index.html` itself: it writes to
  the file when Jellyfin's `JELLYFIN_WEB_DIR` environment variable is set and
  the file is writable, and otherwise serves a patched copy of the page.

Reload Jellyfin in your browser (Ctrl+F5). You should see a **group icon**
in the header next to your user avatar and in the video player's controls.
If it's missing, see
[Troubleshooting]({{ '/troubleshooting/' | relative_url }}#watch-party-button-not-visible).

## 5. Verify

1. Click the group icon. The panel shows **JellyWatchParty** with a green
   **Online** indicator.
2. Start a video, click **Create Room**, leave the password empty and
   confirm. You are now the host.
3. On a second browser or account, open the panel and **Join** the room.
   The guest loads the same item and follows your play, pause and seeks.

**Offline** means the browser can't reach the session server: check the
Session Server URL, `ALLOWED_ORIGINS` and your firewall
([Troubleshooting]({{ '/troubleshooting/' | relative_url }}#cannot-connect-to-session-server)).

## Ports

| Port | Used by | Expose |
|------|---------|--------|
| 3000 | WebSocket server | To users' browsers (directly or through a reverse proxy) |
| 3001 | Admin panel (when `ADMIN_PASSWORD` is set) | Only to admins: localhost, LAN, or HTTPS proxy |
| 3002 | Chat bot API (when an integration token is set) | Never; stays inside the Docker network |

## Optional Components

- [Admin Panel]({{ '/admin-panel/' | relative_url }}): manage rooms, create
  password-protected groups, put TV apps into rooms.
- [Discord Bot]({{ '/discord-bot/' | relative_url }}) and
  [Telegram Bot]({{ '/telegram-bot/' | relative_url }}): let linked users
  run watch parties for their own devices from a chat.

## Test Builds

Builds from the `develop` branch are published for testing, not for
production. Keep the server, bot and plugin on the same channel:

| Component | Stable | Develop |
|-----------|--------|---------|
| Docker images | `JWP_TAG=latest` | `JWP_TAG=dev` |
| Plugin repository | `.../jellyfin-plugin-repo/manifest.json` | `https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest-dev.json` (**JellyWatchParty (Develop)**) |

Develop plugin versions (`0.0.x`) are lower than releases, so uninstall the
stable plugin before installing the develop one. See
[Release: Develop Plugin Channel]({{ '/development/release/' | relative_url }}#develop-plugin-channel).

## Next Steps

- [User Guide]({{ '/user-guide/' | relative_url }}): create and join watch parties
- [Deployment]({{ '/deployment/' | relative_url }}): HTTPS, reverse proxies, updates and backups
- [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }})
