---
title: Discord Bot
nav_order: 7.5
---

# Discord Bot

With the Discord bot, people can run watch parties for apps that can't
show the Watch Party panel (Android TV, Fladder, Swiftfin, ...) without
asking an admin. In a Discord server you choose, they can:

- create a room, with or without a password;
- join a room (with its password);
- put **their own** Jellyfin devices into a room, as host or as receiver;
- as the room's owner: pick the host, remove people and devices, change the
  name and password, hand the room over, and close it.

People with the Watch Party panel (Jellyfin in a browser, Jellyfin Desktop,
the Android/iOS app) join these rooms from their panel's room list, as
usual.

The bot works like the admin panel's [Jellyfin devices]({{ '/admin-panel/' | relative_url }}#jellyfin-devices):
the session server controls each device over the Jellyfin API. A device
added as **host** drives the room. A **receiver** follows the host, so its
app has to accept remote control (Fladder, for example, can only be host).

## How Discord accounts are linked to Jellyfin users

1. In the admin panel, under **Users and link codes**, an admin clicks
   **Assign code** next to a Jellyfin user. The panel shows a 4-digit code
   once.
2. The admin passes the code on privately.
3. The user runs `/jwp link` in Discord. A private form asks for their
   **Jellyfin user name** and the **code**.

A link stays until it is removed. The same code can be used again to link
again later, for example after `/jwp unlink`. One Jellyfin user has at most
one linked Discord account. If someone else enters the right code for a
user who is already linked, the attempt is refused and shows up in the
activity log.

**New code** gives the user a fresh code and disconnects their current
Discord account. **Unlink** only disconnects the account; the code still
works. **Remove code** does both. Rooms a user owns stay theirs whichever
account they link next, because rooms belong to the Jellyfin user, not to
the Discord account.

### Limits on guessing codes

A 4-digit code is short, so attempts are limited:

- After **10 wrong codes for one Jellyfin user**, from anyone, that user's
  code stops working until an admin assigns a new one. The panel marks it
  **Locked**. Someone guessing therefore has at most a 1 in 1,000 chance
  per assigned code.
- A Discord account that enters **5 wrong codes** waits 15 minutes.
- After **50 wrong codes from everyone** within 10 minutes, linking pauses
  for everyone until that window ends.
- A wrong user name, a wrong code and a locked code all get the same
  answer, so the bot doesn't reveal which user names exist.
- Every attempt is listed under **Bot activity** with the Discord account
  id, so you can ban whoever is guessing.

Someone could lock other people's codes on purpose by entering wrong codes
for their user names. This only stops *new* links, because existing links
keep working; give affected users a new code. Users who don't need the bot
don't need a code at all.

## Setup

You need the [admin panel]({{ '/admin-panel/' | relative_url }}) and
[Jellyfin devices]({{ '/admin-panel/' | relative_url }}#jellyfin-devices)
(`JELLYFIN_URL`, `JELLYFIN_API_KEY`) working first.

### 1. Create the Discord application

1. In the [Discord Developer Portal](https://discord.com/developers/applications),
   choose **New Application**, then **Bot** > **Reset Token**. That token is
   `DISCORD_BOT_TOKEN`.
2. The bot needs **no privileged intents**. Leave Presence, Server Members
   and Message Content off.
3. Under **OAuth2** > **URL Generator**, tick the scopes `bot` and
   `applications.commands`, and the permissions **Send Messages**, **Embed
   Links** and **View Channels**. Open the generated URL to invite the bot
   to your server.

### 2. Configure the containers

In `.env`:

```bash
# Shared secret between the session server and the bot (32+ characters)
DISCORD_INTEGRATION_TOKEN=$(openssl rand -hex 32)
DISCORD_BOT_TOKEN=...
```

The bot is published as `ghcr.io/tigamingtv/jwp-discord-bot`, with the same
tags as the session server (`latest`, `X.Y.Z`, `X.Y`, `beta`, `dev`). Keep
both on the same tag. If you use your own compose file, add the bot next to
the session server (the [Installation]({{ '/installation/' | relative_url }}#quick-start-docker-compose)
quick start already has it):

```yaml
services:
  jwp-session:
    image: ghcr.io/tigamingtv/jwp-session-server:latest
    environment:
      # ... your existing settings, plus:
      - DATA_DIR=/data
      - INTEGRATION_HOST=0.0.0.0          # inside the Docker network only
      - DISCORD_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN}
    volumes:
      - jwp-data:/data
    # Do not add 3002 to ports.

  jwp-discord-bot:
    image: ghcr.io/tigamingtv/jwp-discord-bot:latest
    restart: unless-stopped
    profiles: [discord]
    depends_on: [jwp-session]
    environment:
      - DISCORD_BOT_TOKEN=${DISCORD_BOT_TOKEN}
      - JWP_INTEGRATION_TOKEN=${DISCORD_INTEGRATION_TOKEN}
      # Service name of the session server above, port INTEGRATION_PORT
      - JWP_INTEGRATION_URL=http://jwp-session:3002

volumes:
  jwp-data:
```

Then start the bot, which is in the `discord` profile (also in the repository's
compose files):

```bash
docker compose --profile discord up -d
```

For the develop builds, use the `dev` tag on both images.

The session server keeps settings and link codes in `DATA_DIR`, which is the
`jwp-data` volume (`/data`) in the compose files. Back that volume up like any
other config. It holds `integrations.json` and `secret.key`, both readable by
the server user only.

| Variable | Where | Meaning |
|---|---|---|
| `DATA_DIR` | session server | Directory for `integrations.json` and `secret.key`. Without it, chat integrations are off. |
| `DISCORD_INTEGRATION_TOKEN` (`_FILE`) | session server | Token the bot must present. Without it, the integration API doesn't start. |
| `INTEGRATION_HOST` / `INTEGRATION_PORT` | session server | Where the integration API listens (default `127.0.0.1:3002`; compose sets `0.0.0.0` inside the Docker network). **Never publish this port.** |
| `DISCORD_BOT_TOKEN` (`_FILE`) | bot | The bot's Discord token. |
| `JWP_INTEGRATION_TOKEN` (`_FILE`) | bot | The same value as `DISCORD_INTEGRATION_TOKEN`. |
| `JWP_INTEGRATION_URL` | bot | Integration API address (default `http://session-server:3002`). |

### 3. Turn it on in the admin panel

Under **Discord bot**:

| Setting | Meaning |
|---|---|
| Bot enabled | Off: every command answers that the bot is turned off. |
| Server ID | The one Discord server the bot works in. Turn on Developer Mode in Discord, then right-click the server > **Copy Server ID**. `/jwp` is registered there within about 30 seconds. |
| Channel IDs | Only accept commands in these channels (empty: any channel). |
| Required role ID | Only members with this role may use the bot (empty: everyone in the server). |
| Admin role ID | Members with this role can manage every bot room. By default only Jellyfin administrators can. |
| Rooms per user / in total | Limits for rooms created from Discord. |
| Close empty rooms after | A bot room with nobody watching closes after this many minutes. |
| Rooms need a password | Refuse rooms without a password. |
| Devices may be added as host / receiver | Which roles users may give their devices. |

The status chips show whether the bot is connected (it checks in every 30
seconds).

## Using it

| Command | What it does |
|---|---|
| `/jwp link` | Link your account (form: Jellyfin user name + code). |
| `/jwp unlink`, `/jwp whoami` | Unlink; show your linked account and rooms. |
| `/jwp room create` | Form: name + optional password. Posts the room's panel in the channel. |
| `/jwp room join` / `leave` / `list` | Join (form asks for the password), leave (your devices leave too), list. |
| `/jwp device add` / `remove` | Put one of your devices into a room as receiver or host; take it out. |
| `/jwp room host` / `kick` / `rename` / `password` / `transfer` / `close` | Owner and admins only. |
| `/jwp room panel` | Owner: post the room's panel again in this channel (the old one is marked as replaced). |

Each room gets a **panel** message that stays up to date. It shows the
owner, the host, everyone watching with their sync status, and who joined
on Discord. Its buttons:

- **Join** asks for the password if the room has one.
- **Add my device** lists your devices that are open in a Jellyfin app,
  each as "follow the host" and/or "be the host".
- **Remove my device** and **Leave**.
- **Pick host** and **Close room** are for the owner and admins.

All replies are private (only you see them). Passwords and codes are only
typed into forms, never into visible command options.

A device only shows up while its Jellyfin app is open and signed in **as
you**. While a room has no host, anyone who joined it may add a device as host
(a device added as receiver never becomes host by itself). After that,
only the owner (or an admin) can change the host.

Rooms live in the session server's memory: a server restart closes them.
Links, codes and settings are kept.

## Security notes

- The bot holds no state and decides nothing. The session server checks
  every request: the server, channel and role it came from, the link, the
  Jellyfin account (it must still exist and be enabled), room
  participation, ownership, and that a device belongs to the person adding
  it. Ownership is checked against Jellyfin's own session list at the
  moment of adding.
- `DISCORD_INTEGRATION_TOKEN` only works on the integration API, never on
  the admin panel. Someone holding it can act as any **linked** user (and,
  if an admin role is set, claim that role), so keep it as secret as the bot
  token, and keep port 3002 inside the Docker network. The bot talks to the
  session server over plain HTTP, which is fine inside one Docker network;
  if the bot runs on another machine, put TLS in between and use an
  `https://` `JWP_INTEGRATION_URL`.
- Each Discord account may make 30 requests per minute.
- Wrong room passwords count against the same limit as in the Watch Party
  panel: 5 per minute per Jellyfin user.
- The admin role setting trusts Discord's role list as passed on by the bot.
  Leave it empty to rely on Jellyfin administrator rights only.
- Codes are never stored, only an HMAC of them. They are never logged and
  are shown to the admin once.
- Names from users (rooms, devices) are shown as plain text, and the bot
  never pings anyone.
