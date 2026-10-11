---
title: Telegram Bot
nav_order: 7.6
---

# Telegram Bot

The Telegram bot does what the [Discord bot]({{ '/discord-bot/' | relative_url }})
does, in one Telegram group you choose. People there can run watch parties
for apps that can't show the Watch Party panel (Android TV, Fladder,
Swiftfin, ...) without asking an admin:

- create a room, with or without a password;
- join a room (with its password);
- put **their own** Jellyfin devices into a room, as host or as receiver;
- as the room's owner: pick the host, remove people and devices, change the
  name and password, hand the room over, and close it.

It runs in the same container as the Discord bot
(`ghcr.io/tigamingtv/jwp-discord-bot`): set the Telegram tokens and it
starts. You can run Discord, Telegram or both.

## How it differs from Discord

Telegram has no private replies in a group and no forms. So:

- **Anything secret or personal happens in a private chat with the bot**:
  linking (the code), room passwords, your device list, and managing a
  room. The bot **deletes your messages with a code or password** right
  after reading them.
- In the group, buttons answer with a short note at the top of the screen
  (**Join**, **Leave**) or send you a private message (**Add my device**,
  **Remove my device**, **Manage**).
- A bot can only write to you after you started a chat with it. If you
  never did, pressing such a button opens the private chat; press
  **Start** there and you're right where you left off.
- Telegram has no roles. Anyone in the group may use the bot (once linked).
  Optionally, **group administrators** count as admins of every room.

## Linking accounts

Linking works as for Discord (see
[How Discord accounts are linked]({{ '/discord-bot/' | relative_url }}#how-discord-accounts-are-linked-to-jellyfin-users)
and its limits on guessing codes):

1. An admin assigns a code under **Users and link codes** in the admin
   panel.
2. The user sends `/link` to the bot **in a private chat**. The bot asks
   for their Jellyfin user name, then the code, and deletes the message
   with the code. (`/link alice 1234` in one message works too, and is
   deleted the same way.)

The same code links one Discord account and one Telegram account. **Unlink**
in the admin panel disconnects one platform; **New code** and **Remove
code** disconnect both.

If someone sends `/link` with a code in the group, the bot deletes that
message (when it may delete messages there, see below) and points them to
the private chat.

## Setup

You need the [admin panel]({{ '/admin-panel/' | relative_url }}) and
[Jellyfin devices]({{ '/admin-panel/' | relative_url }}#jellyfin-devices)
(`JELLYFIN_URL`, `JELLYFIN_API_KEY`) working first.

### 1. Create the bot

1. Talk to [@BotFather](https://t.me/BotFather): `/newbot`, pick a name and
   a username. The token it gives you is `TELEGRAM_BOT_TOKEN`.
2. Add the bot to your group.
3. Recommended: make it an **administrator** with only **Delete messages**.
   It then removes codes that people paste into the group by mistake, and
   sees `/rooms` and `/newroom` even when other bots are in the group.
   Without admin rights the bot still works, but with Telegram's privacy
   mode it may only see commands addressed to it (`/rooms@YourBot`) if the
   group has other bots.
4. In the group, send `/groupid`. The bot answers with the group's ID, a
   negative number such as `-1001234567890`. The bot also logs it when it is
   added to a group.

### 2. Configure the containers

In `.env`:

```bash
# Shared secret between the session server and the bot, only for Telegram
# (32+ characters, different from DISCORD_INTEGRATION_TOKEN)
TELEGRAM_INTEGRATION_TOKEN=$(openssl rand -hex 32)
TELEGRAM_BOT_TOKEN=123456789:AA...
```

The compose files in the repository and the
[Installation]({{ '/installation/' | relative_url }}#quick-start-docker-compose)
quick start already pass these on. In your own compose file:

```yaml
services:
  jwp-session:
    image: ghcr.io/tigamingtv/jwp-session-server:latest
    environment:
      # ... your existing settings, plus:
      - DATA_DIR=/data
      - INTEGRATION_HOST=0.0.0.0          # inside the Docker network only
      - TELEGRAM_INTEGRATION_TOKEN=${TELEGRAM_INTEGRATION_TOKEN}
    volumes:
      - jwp-data:/data
    # Do not add 3002 to ports.

  jwp-discord-bot:                         # the chat bot, despite its name
    image: ghcr.io/tigamingtv/jwp-discord-bot:latest
    restart: unless-stopped
    profiles: [discord, telegram]
    depends_on: [jwp-session]
    environment:
      - JWP_INTEGRATION_URL=http://jwp-session:3002
      - TELEGRAM_BOT_TOKEN=${TELEGRAM_BOT_TOKEN}
      - TELEGRAM_INTEGRATION_TOKEN=${TELEGRAM_INTEGRATION_TOKEN}

volumes:
  jwp-data:
```

```bash
docker compose --profile telegram up -d
```

| Variable | Where | Meaning |
|---|---|---|
| `TELEGRAM_INTEGRATION_TOKEN` (`_FILE`) | session server and bot | The same value on both. Must differ from `DISCORD_INTEGRATION_TOKEN`. |
| `TELEGRAM_BOT_TOKEN` (`_FILE`) | bot | From @BotFather. The Telegram part of the bot only runs when it is set. |
| `TELEGRAM_API_URL` | bot | Bot API server (default `https://api.telegram.org`). Only for a [self-hosted Bot API server](https://github.com/tdlib/telegram-bot-api). |
| `JWP_INTEGRATION_URL` | bot | As for Discord: the session server's integration API. |

The bot needs no open port: it fetches updates from Telegram (long
polling). Only one program may fetch a bot's updates, so don't run two
containers with the same token; the log warns about a conflict if you do.

### 3. Turn it on in the admin panel

Under **Telegram bot**:

| Setting | Meaning |
|---|---|
| Bot enabled | Off by default. Needs a Group ID first. |
| Group ID | The group's ID from `/groupid` (negative). The command menu is set up there within about 30 seconds. |
| Group administrators manage all rooms | The group's creator and administrators can manage every bot room. Off: only Jellyfin administrators can. |
| Rooms per user / in total, Close empty rooms after, Rooms need a password, Devices may be added as host / receiver | As for Discord. Limits count only rooms created from Telegram. |

## Using it

In the group:

| Command | What it does |
|---|---|
| `/newroom Movie night` | Create an open room; its panel is posted here. Without a name, or when rooms need a password, the bot continues in a private chat. |
| `/rooms` | List the rooms. |
| `/link` | Opens the private chat to link your account. |
| `/groupid` | The group's ID (works in any group). |

In a private chat with the bot:

| Command | What it does |
|---|---|
| `/link` | Link your account: Jellyfin user name, then the code. |
| `/rooms` | The rooms, each with a button that shows its panel here. |
| `/newroom` | Create a room: name, then a password or `/skip`. The panel is posted in the group. |
| `/whoami`, `/unlink` | Your linked account and rooms; unlink. |
| `/cancel` | Stop what the bot is asking for. |

Each room's **panel** in the group shows the owner, the host, everyone
watching with their sync status, and who joined on Telegram. Its buttons:

- **Join**: open rooms right away; for rooms with a password, the bot asks
  for it in the private chat.
- **Leave**: your devices leave too.
- **Add my device** / **Remove my device**: a list of your devices in the
  private chat. A device only shows up while its Jellyfin app is open and
  signed in as you.
- **Manage (owner)**: for the owner and admins, in the private chat:
  rename, password, pick the host, remove someone, hand over, post the
  panel again, close.

Panels are edited at most every few seconds, to stay within Telegram's
limits. Rooms live in the session server's memory: a server restart closes
them.

## Security notes

Everything in the [Discord bot's security notes]({{ '/discord-bot/' | relative_url }}#security-notes)
applies. In addition:

- Codes and room passwords are only accepted in a private chat, and the
  bot deletes those messages right away (Telegram deletes them for both
  sides).
- In a private chat, the bot checks that you're still in the configured
  group (checked again at most every minute).
- Anonymous group admins and posts "as the channel" are refused: the bot
  can't tell who sent them.
- Button data is checked by the server like any other request; choices
  made from private menus only work for the person they were shown to,
  for 10 minutes.
- The bot token is part of every request URL to Telegram; the bot never
  logs those URLs and follows no redirects.
- With **Group administrators manage all rooms** on, the session server
  trusts the bot's report of who is an administrator, as it trusts
  Discord roles.

## Problems?

See [Troubleshooting]({{ '/troubleshooting/' | relative_url }}#telegram-bot).
