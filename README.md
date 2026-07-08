<p align="center">
  <img src="docs/logo.png" alt="JellyWatchParty" width="400">
</p>

<p align="center">
  <strong>Watch movies together, no matter the distance.</strong>
</p>

<p align="center">
  <a href="https://github.com/TIGamingTV/JellyWatchParty/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/TIGamingTV/JellyWatchParty/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/Jellyfin-10.11%2B-00a4dc?style=flat-square&logo=jellyfin" alt="Jellyfin 10.11+">
  <img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="MIT License">
</p>

---

JellyWatchParty enables synchronized media playback for [Jellyfin](https://jellyfin.org/). It consists of a **Jellyfin Plugin** (C#) that integrates the UI and a **Session Server** (Rust) that manages rooms and synchronization via WebSocket.

Forked from https://github.com/mhbxyz/OpenWatchParty

## Quick Start

### Users

**1. Start the session server** with Docker Compose:

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

**2. Add the plugin repository** in Jellyfin: **Dashboard > Plugins > Repositories > Add**

```
https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json
```

Then go to the **Catalog** tab, install **JellyWatchParty**, and restart Jellyfin.

See the [Installation Guide](https://tigamingtv.github.io/JellyWatchParty/operations/installation/) for enabling the client script and full setup details.

### Developers

```bash
git clone https://github.com/TIGamingTV/JellyWatchParty.git
cd JellyWatchParty
just up
```

See the [Development Setup Guide](https://tigamingtv.github.io/JellyWatchParty/development/setup/) for the full workflow.

## Documentation

**[tigamingtv.github.io/JellyWatchParty](https://tigamingtv.github.io/JellyWatchParty/)**

## Contributing

- [Report bugs](https://github.com/TIGamingTV/JellyWatchParty/issues)
- [Submit pull requests](https://github.com/TIGamingTV/JellyWatchParty/pulls)
- [Contributing Guide](https://tigamingtv.github.io/JellyWatchParty/development/contributing.html)

## License

MIT
