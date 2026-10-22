<p align="center">
  <img src="docs/logo.png" alt="JellyWatchParty" width="400">
</p>

<p align="center">
  <strong>Synchronized watch parties for Jellyfin.</strong>
</p>

<p align="center">
  <a href="https://github.com/TIGamingTV/JellyWatchParty/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/TIGamingTV/JellyWatchParty/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/Jellyfin-12.x-00a4dc?style=flat-square&logo=jellyfin" alt="Jellyfin 12.x">
</p>

JellyWatchParty lets people on the same Jellyfin server watch together: one
person hosts a room, others join it, and everyone's player follows the
host's play, pause and seeks. It is made of a **Jellyfin plugin**, which adds
a Watch Party panel to Jellyfin's web interface, and a small **session
server** (Rust) that manages rooms and keeps players in sync.

## Features

- **Watch Party panel** in the Jellyfin header and player: create, join and leave rooms, optional room passwords
- **Synchronized playback** with a 3-2-1 start countdown, drift correction, and guests following the host to new items
- **Chat** in each room, participant list with sync status, **Watch Parties** row on the home screen
- **Democratic mode** (optional): everyone in the room can play, pause and seek
- **Admin panel** (optional): see every room, create password-protected groups, move people, change hosts
- **Jellyfin devices**: put TV apps and other clients without the panel (Android TV, Fladder, ...) into rooms as host or receiver
- **Discord and Telegram bots** (optional): linked users create rooms and add their own devices from a chat
- Authentication with the user's Jellyfin identity (JWT)

Details: [Features](https://tigamingtv.github.io/JellyWatchParty/features/).

## Requirements

- Jellyfin **12.x** (for 10.11.x, use an older [release](https://github.com/TIGamingTV/JellyWatchParty/releases) with `targetAbi` `10.11.11.0`)
- Docker with Compose (images for `linux/amd64` and `linux/arm64`), or Windows for the prebuilt server binary
- Port 3000 reachable from users' browsers, directly or through a reverse proxy

## Quick Start

### 1. Start the session server

Create `compose.yaml`:

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

Create `.env` next to it:

```bash
# Required: the address users open Jellyfin at (scheme://host:port, no trailing slash)
ALLOWED_ORIGINS=http://192.168.1.10:8096
# Required: shared secret, also entered in the plugin settings (openssl rand -base64 32)
JWT_SECRET=<32+ random characters>
# Optional: admin panel at http://localhost:3001 (openssl rand -base64 18)
ADMIN_PASSWORD=
# Optional: Jellyfin devices. URL as seen from the container; key from Dashboard > API Keys
JELLYFIN_URL=
JELLYFIN_API_KEY=
```

```bash
docker compose up -d
curl http://localhost:3000/health   # {"status":"ok","auth_enabled":true}
```

### 2. Install the plugin

In Jellyfin, open **Dashboard > Plugins**, go to the plugin repositories and
add this repository URL:

```
https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json
```

Then install **JellyWatchParty** from the **Catalog** and restart Jellyfin.

### 3. Configure the plugin

In **Dashboard > Plugins > JellyWatchParty**:

1. **JWT Secret**: the same value as `JWT_SECRET`.
2. **Session Server URL**: leave empty if browsers can reach port 3000 on
   the Jellyfin host; otherwise the URL they should use, e.g.
   `ws://192.168.1.20:3000/ws` or `wss://jellyfin.example.com/ws` behind a
   reverse proxy.
3. **Save**, then reload Jellyfin in the browser (Ctrl+F5).

For the most reliable script injection, also install the
[File Transformation](https://github.com/IAmParadox27/jellyfin-plugin-file-transformation)
plugin.

### 4. Watch together

1. Start a video and click the **group icon** in the player controls (or
   next to your avatar in the header). The panel should say **Online**.
2. Click **Create Room**, optionally set a password, and confirm. You are the host.
3. Others open the panel (or the **Watch Parties** row on the home screen)
   and click **Join**. Their player loads your item and follows you.

The full walkthrough, Windows and manual installs, HTTPS setup and every
setting are in the documentation below.

## Documentation

**[tigamingtv.github.io/JellyWatchParty](https://tigamingtv.github.io/JellyWatchParty/)**

| Guide | Contents |
|-------|----------|
| [Installation](https://tigamingtv.github.io/JellyWatchParty/installation/) | Full setup, Windows binary, manual plugin install, verification |
| [Configuration](https://tigamingtv.github.io/JellyWatchParty/configuration/) | Every plugin setting and environment variable |
| [User Guide](https://tigamingtv.github.io/JellyWatchParty/user-guide/) | Creating and joining rooms, chat, democratic mode, TV apps |
| [Admin Panel](https://tigamingtv.github.io/JellyWatchParty/admin-panel/) | Rooms, groups, Jellyfin devices |
| [Discord Bot](https://tigamingtv.github.io/JellyWatchParty/discord-bot/) / [Telegram Bot](https://tigamingtv.github.io/JellyWatchParty/telegram-bot/) | Chat bot setup and commands |
| [Deployment](https://tigamingtv.github.io/JellyWatchParty/deployment/) | Reverse proxy and HTTPS, updates, backups |
| [Troubleshooting & FAQ](https://tigamingtv.github.io/JellyWatchParty/troubleshooting/) | Common problems |
| [Development](https://tigamingtv.github.io/JellyWatchParty/development/) | Building, testing and releasing |

## Contributing

Bug reports and pull requests are welcome on
[GitHub](https://github.com/TIGamingTV/JellyWatchParty/issues). See the
[Contributing Guide](https://tigamingtv.github.io/JellyWatchParty/development/contributing/);
`just up` starts a local development environment.

## License

MIT. Forked from [mhbxyz/OpenWatchParty](https://github.com/mhbxyz/OpenWatchParty).
