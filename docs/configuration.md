---
title: Configuration
nav_order: 6
---

# Configuration Reference

This page is the complete reference for every setting. For a guided setup,
see [Installation]({{ '/installation/' | relative_url }}).

JellyWatchParty is configured in three places:

| Where | What |
|-------|------|
| Jellyfin: **Dashboard > Plugins > JellyWatchParty** | Plugin settings (authentication, session server URL, UI options, panel bridging) |
| Session server environment variables | Ports, origins, authentication, admin panel, Jellyfin devices, chat integrations |
| Chat bot environment variables | Bot tokens and how to reach the session server |

Chat bot behaviour (server or group, channels, roles, limits) is set in the
[admin panel]({{ '/admin-panel/' | relative_url }}#chat-bots), not in the
environment.

## Plugin Settings

Open **Dashboard > Plugins**, select **JellyWatchParty**, change the settings
and click **Save**. No Jellyfin restart is needed. Settings that affect the
browser (everything except panel bridging) apply after users reload the page.

| Setting | Default | Description |
|---------|---------|-------------|
| **JWT Secret** | empty | Secret used to sign session tokens. Must be **the same value as `JWT_SECRET`** on the session server. Use at least 32 characters (`openssl rand -base64 32`). Empty, or shorter than 32 characters, disables authentication: the plugin then issues no tokens. |
| **JWT Audience** | `JellyWatchParty` | `aud` claim. Must match `JWT_AUDIENCE`. |
| **JWT Issuer** | `Jellyfin` | `iss` claim. Must match `JWT_ISSUER`. |
| **Token TTL (seconds)** | `3600` | Token lifetime, 60–86400. The web client renews its token before it expires. |
| **Session Server URL** | empty | WebSocket URL of the session server, e.g. `wss://jellyfin.example.com/ws`. Empty: the browser connects to `ws://<Jellyfin host>:3000/ws` (or `wss://` when Jellyfin is opened over HTTPS). Must be reachable **from users' browsers**, so don't use a Docker service name. Suspicious values show a warning but are saved anyway. Required for panel bridging. |
| **Hide Jellyfin's built-in SyncPlay button** | off | Hides the native SyncPlay button in the web client. |
| **Allow democratic mode** | off | Gives room hosts on Jellyfin Web and Jellyfin Desktop an **Everyone can control playback** switch. Admins can turn democratic mode on for any room in the admin panel regardless. See [Democratic mode]({{ '/features/' | relative_url }}#democratic-mode). |
| **Let users bridge their devices from the Watch Party panel** | off | Master switch for the two options below. For small servers with trusted users. Turning it off stops devices bridged from the panel. |
| **Allow third-party clients to host** | off | With the master switch on: users can make one of their own playing devices (Fladder, Swiftfin, mobile apps, ...) host a new room from the panel. |
| **Allow supported clients as receivers** | off | With the master switch on: users can attach one of their own playing devices (e.g. the Android TV app) to their current room as a receiver. Not for password-protected rooms. |

Admins can put any device into any room from the
[admin panel]({{ '/admin-panel/' | relative_url }}#jellyfin-devices) without
enabling panel bridging.

## Session Server Environment Variables

All variables are read **at startup**. After changing one, recreate the
container (`docker compose up -d`); `docker restart` keeps the old
environment. The only exception is the files named by
`ADMIN_USERNAME_FILE` / `ADMIN_PASSWORD_FILE`, which are re-read while the
server runs.

An empty value is treated like an unset variable. Boolean variables accept
`true`/`false`, `1`/`0`, `yes`/`no` and `on`/`off`.

### Core

| Variable | Default | Description |
|----------|---------|-------------|
| `HOST` | `0.0.0.0` | IP address the WebSocket server binds to. |
| `PORT` | `3000` | WebSocket port. Clients connect to `/ws`; `/health` answers `{"status":"ok","auth_enabled":…}`. |
| `ALLOWED_ORIGINS` | `http://localhost:8096,https://localhost:8096` | Comma-separated list of the exact origins (`scheme://host[:port]`, no trailing slash) your users open Jellyfin from. Browser connections from other origins are refused with 403. Connections without an `Origin` header (such as the plugin's own bridge connections) are always allowed. `*` allows every origin and logs a security warning on each connection. |
| `JWT_SECRET` | empty | Same value as the plugin's **JWT Secret**. Empty disables authentication: anyone who can reach the server can join rooms under any name. Shorter than 32 characters logs a warning; since the plugin ignores such secrets, use 32 or more. |
| `JWT_AUDIENCE` | `JellyWatchParty` | Expected `aud` claim; must match the plugin. |
| `JWT_ISSUER` | `Jellyfin` | Expected `iss` claim; must match the plugin. |
| `RUST_LOG` | `info` | Log level: `error`, `warn`, `info`, `debug` or `trace`. |

### Admin panel

The [admin panel]({{ '/admin-panel/' | relative_url }}) is on by default but
only starts when a password is set. Jellyfin devices and the chat bots need
it.

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_PASSWORD` | none | Login password. **Required for the panel to start.** Use a long random value (`openssl rand -base64 18`); under 12 characters logs a warning. Surrounding spaces and one pair of quotes are removed. |
| `ADMIN_PASSWORD_FILE` | none | Read the password from this file instead (Docker secrets). `ADMIN_PASSWORD` wins if both are set. Re-read on every login and every 10 seconds; a change signs everyone out. |
| `ADMIN_USERNAME` | `admin` | Login name (case-sensitive). |
| `ADMIN_USERNAME_FILE` | none | Read the login name from this file instead. Same rules as `ADMIN_PASSWORD_FILE`. |
| `ADMIN_ENABLED` | `true` | `false` switches the panel off. Any value other than a boolean keeps the panel off and logs why. |
| `ADMIN_HOST` | `0.0.0.0` | IP address the panel binds to (an IP literal such as `0.0.0.0`, `::` or `127.0.0.1`; not a host name). |
| `ADMIN_PORT` | `3001` | Panel port; must differ from `PORT`. |
| `ADMIN_SESSION_TTL_SECS` | `43200` | How long a login stays valid (12 hours; maximum 30 days). |
| `ADMIN_COOKIE_SECURE` | `false` | `true` when the panel is served over HTTPS: the login cookie is then only sent over HTTPS. Leave `false` for plain `http://` access, or logins won't stick. |
| `ADMIN_TRUST_X_FORWARDED_FOR` | `false` | `true` behind a reverse proxy, so failed-login throttling uses the visitor's address (last `X-Forwarded-For` entry). Only enable it if the panel can't be reached without the proxy. |
| `ADMIN_EMPTY_GROUP_TTL_SECS` | `600` | A group created in the panel that nobody joins is removed after this many seconds. |

### Jellyfin devices

Lets the session server put Jellyfin apps that can't show the Watch Party
panel (TV apps, Fladder, ...) into rooms. Needs the admin panel. See
[Admin Panel: Jellyfin devices]({{ '/admin-panel/' | relative_url }}#jellyfin-devices).

| Variable | Default | Description |
|----------|---------|-------------|
| `JELLYFIN_URL` | none | Jellyfin's address **as seen from the session server**, starting with `http://` or `https://`, e.g. `http://jellyfin:8096` on a shared Docker network. |
| `JELLYFIN_API_KEY` | none | API key from Jellyfin **Dashboard > API Keys**. It has full admin rights on Jellyfin: keep it secret. |
| `JELLYFIN_API_KEY_FILE` | none | Read the API key from this file instead. Read at startup only. |
| `BRIDGE_POLL_INTERVAL_MS` | `1000` | How often device positions are read from Jellyfin, in milliseconds (minimum 250). Polling only runs while a device is in a room or the device list is open in the panel. |

Both `JELLYFIN_URL` and an API key are needed; setting only one logs a
warning and leaves the feature off. The standard `HTTPS_PROXY` / `NO_PROXY`
variables apply to these requests.

### Chat integrations (Discord, Telegram)

Needs the admin panel and Jellyfin devices. See the
[Discord Bot]({{ '/discord-bot/' | relative_url }}) and
[Telegram Bot]({{ '/telegram-bot/' | relative_url }}) guides.

| Variable | Default | Description |
|----------|---------|-------------|
| `DATA_DIR` | none | Directory where bot settings, link codes and linked accounts are stored (`integrations.json`, `secret.key`). Without it, chat integrations are off. The compose files set `/data` (the `jwp-data` volume). |
| `DISCORD_INTEGRATION_TOKEN` | none | Shared secret between the session server and the bot for Discord; at least 32 characters (`openssl rand -hex 32`). Same value on both containers. |
| `TELEGRAM_INTEGRATION_TOKEN` | none | Same for Telegram; must differ from the Discord token. |
| `DISCORD_INTEGRATION_TOKEN_FILE`, `TELEGRAM_INTEGRATION_TOKEN_FILE` | none | Read the token from a file instead. |
| `INTEGRATION_HOST` | `127.0.0.1` | IP address the integration API binds to. Set `0.0.0.0` in Docker so the bot container can reach it; **never publish the port**. |
| `INTEGRATION_PORT` | `3002` | Integration API port; must differ from `PORT` and `ADMIN_PORT`. |

The integration API only starts when at least one integration token is set.

## Chat Bot Environment Variables

The bot image (`ghcr.io/tigamingtv/jwp-discord-bot`) runs Discord, Telegram
or both. A platform runs when its bot token is set; with neither set the bot
exits.

| Variable | Default | Description |
|----------|---------|-------------|
| `DISCORD_BOT_TOKEN` | none | From the Discord Developer Portal (**Bot > Reset Token**). Turns Discord on. |
| `DISCORD_INTEGRATION_TOKEN` | none | Required with `DISCORD_BOT_TOKEN`; same value as on the session server. |
| `TELEGRAM_BOT_TOKEN` | none | From @BotFather (`/newbot`), format `123456:ABC...`. Turns Telegram on. |
| `TELEGRAM_INTEGRATION_TOKEN` | none | Required with `TELEGRAM_BOT_TOKEN`; same value as on the session server. |
| `JWP_INTEGRATION_URL` | `http://session-server:3002` | The session server's integration API. Use the session server's compose service name and `INTEGRATION_PORT`. |
| `TELEGRAM_API_URL` | `https://api.telegram.org` | Only for a self-hosted Telegram Bot API server. |
| `RUST_LOG` | `info` | Log level. |

Every token variable also accepts a `_FILE` variant (for example
`DISCORD_BOT_TOKEN_FILE`). `JWP_INTEGRATION_TOKEN` is an old name for the
bot's `DISCORD_INTEGRATION_TOKEN`; it still works for Discord, with a
warning.

## Compose Helper Variables

The compose files in this repository also read these from `.env`. They are
not read by the applications.

| Variable | Default | Description |
|----------|---------|-------------|
| `JWP_TAG` | `latest` | Image tag for both images: `latest` (last release), `beta` (last build of `main`) or `dev` (last build of `develop`). |
| `SESSION_SERVER_PORT` | `3000` | Host port published for the WebSocket server (`infra/docker/*/docker-compose.yml`). |
| `ADMIN_PANEL_PORT` | `3001` | Host port published for the admin panel. |
| `JELLYFIN_PORT`, `MEDIA_DIR`, `UID`, `GID` | | Development compose only; see [Development Setup]({{ '/development/setup/' | relative_url }}). |

## Checking the Configuration

```bash
# Session server: is it up, and is authentication on?
curl http://localhost:3000/health
# {"status":"ok","auth_enabled":true}

# Session server: what it started with (admin panel, devices, chat bots)
docker compose logs session-server | head -n 40
```

In the Jellyfin log, the plugin reports at startup either
`[JellyWatchParty] JWT authentication is enabled.` or a warning that
authentication is disabled. `auth_enabled` on the session server and the
plugin must agree.
