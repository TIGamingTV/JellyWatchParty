<p align="center">
  <img src="docs/logo.png" alt="JellyWatchParty" width="400">
</p>

<p align="center">
  <strong>Watch movies together, no matter the distance.</strong>
</p>

<p align="center">
  <a href="https://github.com/TIGamingTV/JellyWatchParty/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/TIGamingTV/JellyWatchParty/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/Jellyfin-12.x-00a4dc?style=flat-square&logo=jellyfin" alt="Jellyfin 12.x">
  <img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="MIT License">
</p>

---

JellyWatchParty enables synchronized media playback for [Jellyfin](https://jellyfin.org/). It consists of a **Jellyfin Plugin** (C#) that integrates the UI and a **Session Server** (Rust) that manages rooms and synchronization via WebSocket.

Forked from https://github.com/mhbxyz/OpenWatchParty

> **Releases target Jellyfin 12.x only.** Jellyfin 10.11.x support has been
> dropped; if you're still on 10.11.x, install an older
> [release](https://github.com/TIGamingTV/JellyWatchParty/releases) (any
> version with a `targetAbi` of `10.11.11.0`) instead.

## Quick Start with the File transformation Plugin

### Users

**1. Start the session server** with Docker Compose. The Discord bot is
optional; it only starts with the `discord` profile.

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
      # Never publish 3002 (integration API for the Discord bot).
    environment:
      - ALLOWED_ORIGINS=http://your-jellyfin:8096
      - JWT_SECRET=${JWT_SECRET:-}            # same value as in the plugin settings
      - ADMIN_PASSWORD=${ADMIN_PASSWORD:-}
      # Jellyfin devices (TV apps, Fladder, ...) in the admin panel and the bot
      - JELLYFIN_URL=${JELLYFIN_URL:-}        # as seen from this container
      - JELLYFIN_API_KEY=${JELLYFIN_API_KEY:-}
      # Discord bot: settings and link codes are kept in /data
      - DATA_DIR=/data
      - INTEGRATION_HOST=0.0.0.0
      - DISCORD_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN:-}
    volumes:
      - jwp-data:/data

  # Optional: docker compose --profile discord up -d
  jwp-discord-bot:
    image: ghcr.io/tigamingtv/jwp-discord-bot:${JWP_TAG:-latest}
    container_name: jwp-discord-bot
    restart: unless-stopped
    profiles: [discord]
    depends_on: [jwp-session]
    environment:
      - DISCORD_BOT_TOKEN=${DISCORD_BOT_TOKEN:-}
      - JWP_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN:-}
      - JWP_INTEGRATION_URL=http://jwp-session:3002

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
```

```bash
docker compose up -d                     # session server only
docker compose --profile discord up -d   # session server + Discord bot
```

The admin panel is at `http://<server>:3001/`. To set up the bot, see the
[Discord Bot guide](https://tigamingtv.github.io/JellyWatchParty/discord-bot/).

**2. Add the plugin repository** in Jellyfin: **Dashboard > Plugins > Repositories > Add**

```
https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json
```

Then go to the **Catalog** tab, install **JellyWatchParty**, and restart Jellyfin.

For Windows Server, manual installs, and enabling the client script: see the
**[Installation Guide](https://tigamingtv.github.io/JellyWatchParty/installation/)**.

### Testing develop builds

Every merge into `develop` publishes test builds. They aren't meant for
production.

| Component | Stable (release) | Develop |
|-----------|------------------|---------|
| Session server | `ghcr.io/tigamingtv/jwp-session-server:latest` | `ghcr.io/tigamingtv/jwp-session-server:dev` |
| Discord bot | `ghcr.io/tigamingtv/jwp-discord-bot:latest` | `ghcr.io/tigamingtv/jwp-discord-bot:dev` |
| Plugin repository | `.../jellyfin-plugin-repo/manifest.json` | `.../jellyfin-plugin-repo/manifest-dev.json` |

With the compose file above, set `JWP_TAG=dev` in `.env` (or `JWP_TAG=1.2.3`
to pin a release), then:

```bash
docker compose --profile discord pull
docker compose --profile discord up -d
```

For the plugin, add
`https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest-dev.json`
as a repository and install **JellyWatchParty (Develop)**. Keep the server,
bot and plugin on the same channel.

### Developers

```bash
git clone https://github.com/TIGamingTV/JellyWatchParty.git
cd JellyWatchParty
just up
```

See the [Development Setup Guide](https://tigamingtv.github.io/JellyWatchParty/development/setup/) for the full workflow.

## Documentation

**[tigamingtv.github.io/JellyWatchParty](https://tigamingtv.github.io/JellyWatchParty/)** — start with
[Installation](https://tigamingtv.github.io/JellyWatchParty/installation/),
[Features](https://tigamingtv.github.io/JellyWatchParty/features/), and
[Core Structure](https://tigamingtv.github.io/JellyWatchParty/core-structure/).

## Contributing

- [Report bugs](https://github.com/TIGamingTV/JellyWatchParty/issues)
- [Submit pull requests](https://github.com/TIGamingTV/JellyWatchParty/pulls)
- [Contributing Guide](https://tigamingtv.github.io/JellyWatchParty/development/contributing/)

## License

MIT
