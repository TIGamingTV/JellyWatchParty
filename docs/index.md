---
title: Home
layout: home
nav_order: 1
---

<p align="center">
  <img src="logo.png" alt="JellyWatchParty" width="400">
</p>

# JellyWatchParty Documentation

JellyWatchParty is a Jellyfin plugin that enables synchronized media playback across multiple clients. Watch movies and shows together with friends, no matter where they are.

## Where to Start

- **[Installation](installation)** - All install options: Docker, Windows Server, and the Jellyfin plugin
- **[Features](features)** - What JellyWatchParty does, compatibility, and the roadmap
- **[Core Structure](core-structure)** - How the plugin, session server, and web client fit together
- **[User Guide](user-guide)** - Creating and joining watch parties
- **[Troubleshooting & FAQ](troubleshooting)** - Common issues and questions

Contributing code instead? See [Development Setup](development/setup) to
get a local environment running with `just up`.

---

## Glossary

Technical terms used throughout this documentation:

| Term | Full Name | Description |
|------|-----------|-------------|
| **HLS** | HTTP Live Streaming | Adaptive streaming protocol that breaks video into small segments. Used by Jellyfin for transcoded content. |
| **RTT** | Round-Trip Time | Time for a message to travel from client to server and back. Displayed in the Watch Party panel as latency indicator. |
| **EMA** | Exponential Moving Average | Smoothing algorithm used for clock synchronization. Prevents sudden jumps in time offset. |
| **JWT** | JSON Web Token | Compact, URL-safe token format for authentication. Contains user identity claims signed with a secret key. |
| **CORS** | Cross-Origin Resource Sharing | Browser security mechanism controlling which websites can connect to the session server. |
| **WebSocket** | - | Full-duplex communication protocol over a single TCP connection. Used for real-time sync. |
| **Drift** | - | Difference between expected and actual playback position. Corrected by adjusting playback rate. |
| **Host** | - | The user who created the watch party room. Has exclusive control over playback. |
