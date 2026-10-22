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
  name and password, hand the room over, and close it;
- if the admin allows it, play, pause and seek for everyone in a room in
  [democratic mode](#democratic-mode).

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
activity log. The reverse case also works: a Discord account that enters the
right code for a *different* Jellyfin user moves its link to that user, and
its old link is dropped.

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
- A Discord account that enters **5 wrong codes within 15 minutes** is blocked
  until that 15-minute window ends. The window starts with the first wrong
  code, so the block can be shorter than 15 minutes.
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

The [installation compose file]({{ '/installation/' | relative_url }}#docker-compose-recommended)
already contains the bot (service `discord-bot`, image
`ghcr.io/tigamingtv/jwp-discord-bot`) and passes the variables on. Set them
in `.env`:

```bash
# Shared secret between the session server and the bot (32+ characters).
# Generate it once with `openssl rand -hex 32` and paste the result.
DISCORD_INTEGRATION_TOKEN=<64 hex characters>
DISCORD_BOT_TOKEN=<token from the Developer Portal>
```

Then start the session server and the bot:

```bash
docker compose --profile discord up -d
docker compose logs -f discord-bot
```

Use the same image tag (`JWP_TAG`) for the session server and the bot.

If you use your own compose file, the session server needs `DATA_DIR`
(with a volume), `INTEGRATION_HOST=0.0.0.0` and `DISCORD_INTEGRATION_TOKEN`;
the bot needs `DISCORD_BOT_TOKEN`, the same `DISCORD_INTEGRATION_TOKEN`, and
`JWP_INTEGRATION_URL` pointing at the session server's service name and port
3002 (default `http://session-server:3002`). **Never publish port 3002.**
All variables: [Configuration: Chat integrations]({{ '/configuration/' | relative_url }}#chat-integrations-discord-telegram)
and [Chat bot variables]({{ '/configuration/' | relative_url }}#chat-bot-environment-variables).

The session server keeps bot settings and link codes in `DATA_DIR` (the
`jwp-data` volume): `integrations.json` and `secret.key`, readable by the
server user only. Back up the whole volume
([Deployment: Backing up]({{ '/deployment/' | relative_url }}#backing-up)).

### 3. Turn it on in the admin panel

Under **Discord bot**:

| Setting | Meaning |
|---|---|
| Bot enabled | Off by default. While off, every command answers "Watch parties from chat are turned off on this server". Needs a Server ID first. |
| Server ID | The one Discord server the bot works in. Turn on Developer Mode in Discord, then right-click the server > **Copy Server ID**. `/jwp` is registered there within about 30 seconds. |
| Channel IDs | Only accept commands in these channels (empty: any channel). |
| Required role ID | Only members with this role may use the bot (empty: everyone in the server). |
| Admin role ID | Members with this role can manage every bot room. By default only Jellyfin administrators can. |
| Rooms per user / in total | Limits for rooms created from Discord: rooms per person (default 2, 1 to 10) and across the server (default 20, 1 to 100). |
| Close empty rooms after | A bot room with nobody in it closes after this many minutes (default 30, 5 to 1440). |
| Rooms need a password | Refuse rooms without a password. Off by default. |
| Devices may be added as host / receiver | Which roles users may give their devices. Both are on by default. Turn one off to refuse it for everyone. |
| Democratic mode commands | Off by default. Room owners and admins can turn on [democratic mode](#democratic-mode), and everyone in a democratic room gets `/jwp playback` and the panel's playback buttons. `/jwp` is registered again with or without these commands within about 30 seconds of a change. |

The status chip reads **Bot online** while the bot checks in (every 30
seconds). It reads **Bot offline** if nothing has been heard for 90 seconds,
and **No bot token** if `DISCORD_INTEGRATION_TOKEN` is not set.

## Using it

| Command | What it does |
|---|---|
| `/jwp link` | Link your account (form: Jellyfin user name + code). |
| `/jwp unlink`, `/jwp whoami` | Unlink; show your linked account and rooms. |
| `/jwp room create` | Form: name + optional password. Posts the room's panel in the channel. |
| `/jwp room join` / `leave` / `list` | Join (form asks for the password), leave (your devices leave too), list. An owner can't leave their own room: hand it over with `transfer` or close it. |
| `/jwp device add` / `remove` | Put one of your devices into a room as receiver or host; take it out. |
| `/jwp room host` / `kick` / `rename` / `password` / `transfer` / `close` | Owner and admins only. |
| `/jwp room panel` | Owner and admins: post the room's panel again in this channel (the old one is marked as replaced). After a bot restart, older panels are no longer marked, so delete any stale ones by hand. |
| `/jwp room democratic` | Owner and admins: turn [democratic mode](#democratic-mode) on or off (`mode`: `on` / `off`). Only with *Democratic mode commands*. |
| `/jwp playback play` / `pause` | Anyone in a democratic room: play or pause for everyone. Only with *Democratic mode commands*. |
| `/jwp playback seek` | Anyone in a democratic room: `1:23:45`, `83:10` or `95` (seconds) jump to that time; `+30`, `-10` or `+1:30` jump ahead or back by that much. Only with *Democratic mode commands*. |

Each room gets a **panel** message that stays up to date. It shows the
owner, the host, everyone watching with their sync status, and who joined
on Discord. Its buttons:

- **Join** asks for the password if the room has one.
- **Add my device** lists your devices that are open in a Jellyfin app,
  each as "follow the host" and/or "be the host".
- **Remove my device** and **Leave**.
- **Pick host** and **Close room** are for the owner and admins.
- **Play**, **Pause**, **−30s** and **+30s**, only on a democratic room
  while *Democratic mode commands* is on.

With *Democratic mode commands* on, or when the room is democratic anyway,
the panel also has a **Control** field: *Everyone* or *Host only*.

All replies are private (only you see them). Passwords and codes are only
typed into forms, never into visible command options.

A device only shows up while its Jellyfin app is open and signed in **as
you**. While a room has no host, anyone who joined it may add a device as host
(a device added as receiver never becomes host by itself). After that,
only the owner (or an admin) can change the host.

### Democratic mode

In a democratic room, everyone in it can play, pause and seek, not just the
host (see [Features: Democratic Mode]({{ '/features/' | relative_url }}#democratic-mode)). From
Discord this needs the admin panel setting **Democratic mode commands**.
Then:

- the room's owner or an admin turns it on or off with
  `/jwp room democratic`. The host can also switch it from their Watch
  Party panel, and an admin from the admin panel;
- everyone who joined the room on Discord (and the owner and admins) can use
  `/jwp playback play`, `pause` and `seek`, and the panel's playback
  buttons, once the room has started playing. The command goes to everyone
  in the room, the host included.

Possible refusals:

| Reason | Meaning |
|---|---|
| `democratic_off` | *Democratic mode commands* is off for this bot. |
| `not_owner` | Only the owner or an admin may switch the mode. |
| `not_participant` | Join the room first. |
| `not_democratic` | The room is host-only; the owner can turn on democratic mode. |
| `not_started` | Nothing is playing in the room yet (the host starts it). |
| `invalid` | The time couldn't be read. |

Rooms live in the session server's memory: a server restart closes them.
Links, codes and settings are kept. A code's own lock (after 10 wrong tries)
also survives a restart, but the short per-account and global windows
(see [Limits on guessing codes](#limits-on-guessing-codes)) start over.

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
- Wrong room passwords are throttled as in the Watch Party panel: 5 wrong
  attempts per user, per room, per minute.
- The admin role setting trusts Discord's role list as passed on by the bot.
  Leave it empty to rely on Jellyfin administrator rights only.
- Codes are never stored, only an HMAC of them. They are never logged and
  are shown to the admin once.
- Names from users (rooms, devices) are shown as plain text, and the bot
  never pings anyone.

## Problems?

See [Troubleshooting]({{ '/troubleshooting/' | relative_url }}#discord-bot).
