<p align="center">
  <img src="docs/logo.png" alt="OpenWatchParty" width="400">
</p>

<p align="center">
  <strong>Watch movies together, no matter the distance.</strong>
</p>

<p align="center">
  <a href="https://github.com/TIGamingTV/OpenWatchParty/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/TIGamingTV/OpenWatchParty/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <img src="https://img.shields.io/badge/Jellyfin-10.9%2B-00a4dc?style=flat-square&logo=jellyfin" alt="Jellyfin 10.9+">
  <img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="MIT License">
</p>

---

OpenWatchParty enables synchronized media playback for [Jellyfin](https://jellyfin.org/). It consists of a **Jellyfin Plugin** (C#) that integrates the UI and a **Session Server** (Rust) that manages rooms and synchronization via WebSocket.

## Quick Start

### Users

```bash
docker run -d --name owp-session -p 3000:3000 \
  -e ALLOWED_ORIGINS="http://your-jellyfin:8096" \
  ghcr.io/tigamingtv/owp-session-server:latest
```

Then install the plugin from Jellyfin's catalog. See the [Installation Guide](https://tigamingtv.github.io/OpenWatchParty/operations/installation.html) for full instructions.

### Developers

```bash
git clone https://github.com/TIGamingTV/OpenWatchParty.git
cd OpenWatchParty
just up
```

See the [Development Setup Guide](https://tigamingtv.github.io/OpenWatchParty/development/setup.html) for the full workflow.

## Documentation

**[tigamingtv.github.io/OpenWatchParty](https://tigamingtv.github.io/OpenWatchParty/)**

## Contributing

- [Report bugs](https://github.com/TIGamingTV/OpenWatchParty/issues)
- [Submit pull requests](https://github.com/TIGamingTV/OpenWatchParty/pulls)
- [Contributing Guide](https://tigamingtv.github.io/OpenWatchParty/development/contributing.html)

## License

MIT
