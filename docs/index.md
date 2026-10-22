---
title: Home
layout: home
nav_order: 1
---

<p align="center">
  <img src="logo.png" alt="JellyWatchParty" width="400">
</p>

# JellyWatchParty

JellyWatchParty adds synchronized watch parties to [Jellyfin](https://jellyfin.org/)
12.x. One person hosts a room from the Watch Party panel in Jellyfin's web
interface, others join, and everyone's player follows the host's play,
pause and seeks, with chat in the room. TV apps and other clients without
the panel can join as devices, added by an admin or by users themselves
through an optional Discord or Telegram bot.

It consists of a Jellyfin plugin and a small session server (Docker image or
Windows binary).

## Where to Start

- **[Installation]({{ '/installation/' | relative_url }})**: run the session server, install and configure the plugin
- **[Features]({{ '/features/' | relative_url }})**: what it does, supported clients, limits
- **[User Guide]({{ '/user-guide/' | relative_url }})**: create and join watch parties
- **[Configuration]({{ '/configuration/' | relative_url }})**: every plugin setting and environment variable
- **[Admin Panel]({{ '/admin-panel/' | relative_url }})**: manage rooms and put TV apps into rooms
- **[Discord Bot]({{ '/discord-bot/' | relative_url }})** and **[Telegram Bot]({{ '/telegram-bot/' | relative_url }})**: optional self-service from a chat
- **[Deployment]({{ '/deployment/' | relative_url }})**: HTTPS, reverse proxies, updates and backups
- **[Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }})**

Contributing? See [Development]({{ '/development/' | relative_url }}) and the
[Technical Reference]({{ '/technical/' | relative_url }}).

## Glossary

| Term | Meaning |
|------|---------|
| **Room** | A watch party. Rooms live in the session server's memory. |
| **Host** | The room member everyone follows. Only the host controls playback, unless the room is in democratic mode. |
| **Receiver** | A member, usually a Jellyfin device, that follows the host. |
| **Jellyfin device** | A Jellyfin app without the Watch Party panel (TV app, Fladder, ...) that the session server controls through Jellyfin's remote control. |
| **Session server** | The program that manages rooms and keeps players in sync (WebSocket, port 3000). |
| **JWT** | The signed token the plugin gives the browser so the session server knows which Jellyfin user it is. |
| **Origin** | The address a web page was loaded from (`scheme://host:port`), checked against `ALLOWED_ORIGINS`. |
| **RTT** | Round-trip time to the session server, shown in the panel. |
| **Drift** | How far a guest's position is from the host's; corrected by adjusting playback speed or seeking. |
