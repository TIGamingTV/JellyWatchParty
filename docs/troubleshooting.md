---
title: Troubleshooting & FAQ
nav_order: 7
---

# Troubleshooting & FAQ

## Quick Checklist

1. Session server running? `curl http://localhost:3000/health` should answer
   `{"status":"ok",...}`.
2. Plugin installed and active? **Dashboard > Plugins**.
3. Browser cache cleared? Reload with Ctrl+F5 (Cmd+Shift+R on macOS).
4. Panel says **Online**? If not, see [Cannot connect](#cannot-connect-to-session-server).
5. `JWT_SECRET` and the plugin's **JWT Secret** identical (or both empty)?
6. Using a chat bot? See [Discord Bot](#discord-bot) or [Telegram Bot](#telegram-bot).

Where to look:

| Log | How |
|-----|-----|
| Session server | `docker compose logs -f session-server`. The first lines show the allowed origins, `JWT: ENABLED`/`DISABLED`, and whether the admin panel, Jellyfin devices and chat integrations started (and why not). |
| Chat bot | `docker compose logs -f discord-bot` |
| Jellyfin | **Dashboard > Logs**, or `docker logs <jellyfin container>`. Plugin lines start with `[JellyWatchParty]`. |
| Browser | Developer tools (F12) > Console, filter by `JellyWatchParty`. The Network tab, filtered by **WS**, shows the WebSocket connection. |

## Common Issues

### Watch Party Button Not Visible

1. **Reload without cache** (Ctrl+F5).
2. **Check the plugin is active** in **Dashboard > Plugins**, and restart
   Jellyfin after installing or updating it.
3. **Check the script injection in the Jellyfin log.** At startup the plugin
   logs one of:
   - `Registered index.html transformation with File Transformation` (good);
   - `Injected client script into ...` or `Client script already present in ...` (good);
   - `File Transformation plugin not available and JELLYFIN_WEB_DIR not set`
     or `No write permission to ...`: install the
     [File Transformation](https://github.com/IAmParadox27/jellyfin-plugin-file-transformation)
     plugin and restart Jellyfin.
4. **Check the browser console.** A `404` for `/JellyWatchParty/ClientScript`
   or `/JellyWatchParty/Client/...` means the plugin isn't loaded.
5. **Check the Jellyfin version.** The plugin targets Jellyfin 12.x and logs
   a warning at startup if the server is a newer major version.

The button is not shown on the admin dashboard.

### Cannot Connect to Session Server

The panel shows **Offline**, and its footer shows the server it tries
(**Server: host:port**).

1. **Is the server reachable from the browser's machine?** Open
   `http://<server>:3000/health` in that browser.
2. **Is the Session Server URL right?** Empty means `ws://<Jellyfin
   host>:3000/ws`. It must be an address browsers can reach: not a Docker
   service name, not `localhost` (unless the browser runs on the server).
3. **HTTPS:** a Jellyfin page opened over `https://` can only connect to
   `wss://`. Put the session server behind your reverse proxy
   ([Deployment]({{ '/deployment/' | relative_url }}#behind-a-reverse-proxy))
   and set the URL to `wss://.../ws`. The client shows a "Session Server URL
   problem" notice for an insecure URL.
4. **Origin:** if the session server logs
   `Rejected connection from origin: <origin>`, add exactly that origin to
   `ALLOWED_ORIGINS` and run `docker compose up -d`.
5. **Firewall:** port 3000 (or your proxy) must be open.

### Authentication Errors

- **"Authentication required: no token provided"**: the session server has
  `JWT_SECRET` set but the plugin's **JWT Secret** is empty or shorter than
  32 characters (the plugin then issues no tokens). Use the same 32+ character
  secret on both.
- **"Authentication failed"** (server log `Auth failed for ...`): the secrets,
  **JWT Audience** / `JWT_AUDIENCE` or **JWT Issuer** / `JWT_ISSUER` differ.
- **Token rate limit:** the plugin issues at most 30 tokens per user per
  minute. Repeated page reloads can hit it; wait a minute.

Tokens are renewed automatically before they expire.

### Sync Issues

1. **Wait a few seconds**: after joining or a seek, guests catch up by
   adjusting playback speed.
2. **Host: pause and play again** to re-sync everyone.
3. **Check the latency** (**RTT** in the panel). High latency or an
   unstable connection makes sync worse.
4. **Transcoding:** transcoded (HLS) streams buffer more. Lower the quality
   or use direct play if a guest keeps falling behind.
5. **Same item:** guests always play the host's item; different versions of
   a file can have different lengths.

Details: [Sync Algorithms]({{ '/technical/sync/' | relative_url }}).

### Room Closes Unexpectedly

- The session server restarted (rooms are kept in memory).
- Everyone left. When the host leaves, the person who joined earliest
  becomes host; the room only ends when nobody is left. A dropped
  connection keeps its place for 90 seconds.
- Creating a new room closes the room you currently host.
- An admin closed it, or (chat bot rooms) it was empty longer than
  **Close empty rooms after**.

### "Rate limit exceeded"

Each connection may send 30 messages per second. Normal use doesn't reach
this; rapid repeated seeking can. The server logs `Rate limited client`.

### Panel Opens but Shows No Rooms

**No active rooms.** means nobody has created one. If the panel is
**Offline**, see [Cannot connect](#cannot-connect-to-session-server). Check
the browser console for script errors after an update and reload with
Ctrl+F5.

### Democratic Mode

See [Features: Democratic Mode]({{ '/features/' | relative_url }}#democratic-mode) for how it works.

1. **The host has no *Everyone can control playback* switch.** The plugin
   setting **Allow democratic mode** (Dashboard > Plugins > JellyWatchParty,
   Interface section) is off; turn it on and reload the page. Only the host
   sees the switch. An admin can turn the mode on for any room from the
   [admin panel]({{ '/admin-panel/' | relative_url }}#democratic-mode) without that setting.
2. **"Only the host can change this setting".** Someone who isn't the host
   tried to switch it.
3. **A guest's pause or seek is undone.** The room hasn't started yet (the
   first play, with its countdown, is the host's), the guest is still
   loading the item or is on another item, or the guest's page still runs
   an old client script (hard refresh). Changing the item always stays with
   the host.
4. **Pausing on a TV app isn't applied to the room.** TV apps only follow
   unless an admin turned on **Devices control too** for the room. Even
   then Jellyfin only hears about it when the app reports (often several
   seconds), the app must accept remote control, and a seek must be more
   than about 8 seconds.
5. **A TV app pauses the room on its own.** With **Devices control too**, a
   device that pauses to buffer looks like someone pressing pause. Turn it
   off (**Devices follow only**) if that keeps happening.
6. **Others' commands are undone a moment later when a TV app is host.**
   The host app doesn't accept remote control (Fladder, for example), so it
   can't carry them out and its own state wins. Make a device that accepts
   remote control, or a person, the host. A host bridged from the Watch
   Party panel (plugin bridge) never follows others' commands.
7. **No democratic commands or playback buttons in Discord or Telegram.**
   **Democratic mode commands** is off in the bot's settings in the admin
   panel. After turning it on, `/jwp` is updated within about 30 seconds.
   The playback buttons only show on democratic rooms, and the commands
   answer "Only the host controls this room" until the owner turns the mode
   on, and "Nothing is playing in this room yet" until the host starts.

### Discord Bot

Setup is in the [Discord Bot guide]({{ '/discord-bot/' | relative_url }}).
Work through these in order:

1. **`/jwp` doesn't appear in Discord.** In the admin panel, check that
   **Bot enabled** is on and **Server ID** is the server's number (right-click
   the server > Copy Server ID, with Developer Mode on). Check that the bot
   was invited with the `applications.commands` scope. After you save the
   Server ID, allow up to 30 seconds. If it still doesn't appear, check
   `docker compose logs discord-bot`.
2. **"Watch parties from chat are turned off on this server".** **Bot
   enabled** is off in the admin panel.
3. **The chip reads "No bot token" or "Bot offline".** Run
   `docker ps` and check that `jwp-discord-bot` is up. It only starts with
   `--profile discord`. Both containers need the same
   `DISCORD_INTEGRATION_TOKEN` (older setups named it `JWP_INTEGRATION_TOKEN`
   on the bot; that still works, with a warning in the log).
   The token must be at least 32 characters (`openssl rand -hex 32`). If
   Discord rejects `DISCORD_BOT_TOKEN`, the bot log says
   `Discord connection ended for good (bad token or intents?)`.
4. **The session server logs `Chat integrations: NOT started - ...`.** The
   reason follows the dash. Common causes: `DATA_DIR` is unset (the compose
   files set `/data`); `DISCORD_INTEGRATION_TOKEN` is missing or too short;
   `INTEGRATION_HOST` is a name rather than an IP address (`localhost` is not
   accepted); `INTEGRATION_PORT` equals `PORT` or `ADMIN_PORT`. The bot also
   needs the admin panel running (`ADMIN_PASSWORD` set, `ADMIN_ENABLED` not
   `false`) and the Jellyfin devices settings (`JELLYFIN_URL`,
   `JELLYFIN_API_KEY`).
5. **The bot's `JWP_INTEGRATION_URL` can't reach the session server.** It
   must be `http://<session server's compose service name>:<INTEGRATION_PORT>`;
   the default `http://session-server:3002` matches the installation compose
   file. The bot logs `session server refused our token` when the integration
   tokens differ.
6. **A user gets "wrong name or code" with the right code.** The bot gives
   the same answer for a wrong name, a wrong code and a locked code, on
   purpose. Check the Jellyfin user name, then the admin panel: a code marked
   **Locked: too many wrong tries** needs **New code**. If the Discord account
   was blocked for too many wrong codes, wait for the 15-minute window to end.
   **Bot activity** lists the attempts with the Discord account id.
7. **A correct code is refused with "already linked".** The Jellyfin user is
   linked to a different Discord account. An admin unlinks the old account
   under **Users and link codes**, or assigns a new code.
8. **`/jwp device add` says "Devices can't be added as host/receiver on this
   server".** That role is turned off in **Devices may be added as host /
   receiver**. The device list only shows devices that are open in a Jellyfin
   app signed in as you.
9. **"The room already has a host; only its owner can change that".** Anyone
   can add a device as host only while a room has no host. After that, only
   the owner or an admin can change it. Add the device as receiver instead.
10. **A Discord room disappeared.** Rooms close when the session server
    restarts, and when nobody is in them for **Close empty rooms after**
    minutes (default 30). Links and settings survive a restart.
11. **Codes stop working after you move or restore the data.** The
    `secret.key` file in `DATA_DIR` is missing or was replaced. A new key is
    created silently, and every existing code stops matching. Restore the
    whole `jwp-data` volume, or assign new codes.

### Telegram Bot

Setup is in the [Telegram Bot guide]({{ '/telegram-bot/' | relative_url }}).
Most of the [Discord list](#discord-bot) applies too (codes, limits,
`DATA_DIR`, `JWP_INTEGRATION_URL`). Specific to Telegram:

1. **The bot doesn't start; the log says `Telegram refused
   TELEGRAM_BOT_TOKEN`.** Copy the token from @BotFather again
   (`/mybots` > your bot > API Token). The Discord part of the bot, if set
   up, keeps running.
2. **The chip reads "No bot token" or "Bot offline".** The session server
   and the bot need the same `TELEGRAM_INTEGRATION_TOKEN`, at least 32
   characters and different from the Discord one. The bot starts with
   `--profile telegram` (or `discord`: it's one container).
3. **"This group isn't set up for watch parties".** The **Group ID** in the
   admin panel isn't this group's. Send `/groupid` in the group and copy
   the negative number. If the group was upgraded to a supergroup, its ID
   changed (the bot log says so): update it.
4. **The bot ignores `/rooms` in the group.** With other bots in the group,
   Telegram may only pass on commands addressed to this one: use
   `/rooms@YourBot`, or make the bot an administrator.
5. **"Join the watch party group first".** In a private chat the bot only
   serves members of the configured group. Join it with the same Telegram
   account. If you just joined, wait a minute.
6. **Pressing Add my device opens a chat with the bot instead.** The bot
   may only write to people who started a chat with it. Press **Start**
   there; the device list follows.
7. **"Post as yourself".** You're posting as an anonymous admin or as the
   channel. Turn off **Remain anonymous** for yourself, or use the private
   chat.
8. **The log warns about another program fetching the bot's updates.** Two
   containers run with the same `TELEGRAM_BOT_TOKEN`, or a webhook was set
   by another tool. Stop the other one.
9. **Codes stay visible in the group.** The bot needs to be an
   administrator with **Delete messages** to remove them there. In the
   private chat it can always delete them.

## Resetting

- **Browser:** reload with Ctrl+F5, or clear the site data for your Jellyfin
  address.
- **Session server:** `docker compose restart session-server` ends all
  rooms. After changing `.env`, use `docker compose up -d` instead: a restart
  keeps the old environment.
- **Admin panel login didn't change:** see
  [Changing the username or password]({{ '/admin-panel/' | relative_url }}#changing-the-username-or-password)
  and [If you can't sign in]({{ '/admin-panel/' | relative_url }}#if-you-cant-sign-in).

## Frequently Asked Questions

**Do all participants need Jellyfin accounts?** Yes. Everyone plays the item
from the same Jellyfin server, so they also need access to its library.

**Does it work with Plex or Emby?** No. It is built for Jellyfin's plugin
system and web interface.

**Is Docker required?** No. There is a Windows binary, and the server can be
built from source. See [Installation]({{ '/installation/' | relative_url }}).

**Can I run everything on one machine?** Yes. Jellyfin uses 8096 and the
session server 3000 by default.

**Who controls playback?** The host. In [democratic mode]({{ '/features/' | relative_url }}#democratic-mode)
other participants can play, pause and seek too.

**What happens if the host leaves?** The person who joined earliest becomes
host. A brief disconnect is invisible: the host keeps their place for 90
seconds.

**Can I use the Android TV app or Fladder?** Yes, as a Jellyfin device: an
admin adds it in the [admin panel]({{ '/admin-panel/' | relative_url }}#jellyfin-devices),
or users add their own through a chat bot or (if enabled) the panel.
Fladder can only host.

**Can I make a room private?** Yes, with a password when you create it.
Admins can create password-protected groups.

**Does everyone need the same video quality?** No. Each player streams
independently; only position and play state are shared.

**What ports are used?** 3000 (WebSocket), 3001 (admin panel, optional),
3002 (chat bot API, internal only).

## Getting Help

Open an issue on [GitHub](https://github.com/TIGamingTV/JellyWatchParty/issues)
with your Jellyfin version, how you run the session server, your browser or
app, the relevant log lines (server, Jellyfin, browser console; remove
secrets) and the steps to reproduce.
