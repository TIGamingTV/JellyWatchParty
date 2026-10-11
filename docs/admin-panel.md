---
title: Admin Panel
nav_order: 7
---

# Admin Panel

The session server has a small web UI for admins, on its own port
(default `3001`). It shows:

- every room, with its members, who is host, and each member's sync status;
- connected clients that are not in a room.

Admins can use it to:

- **create groups**: named rooms with a password, so normal users can
  join them from the Watch Party panel;
- **add people to a room** (no password needed) and **move** them between
  rooms;
- **make someone host** (everyone else in the room follows the host);
- **remove** people from a room, **rename** a room, **set or remove its
  password**, and **close** it;
- **put Jellyfin devices into rooms**: TV apps and other clients that
  can't show the Watch Party panel (official Android TV app, Fladder,
  Swiftfin, Infuse, ...), as **host** or **receiver**. See
  [Jellyfin devices](#jellyfin-devices);
- **let users do this themselves from Discord or Telegram**: set up the
  [Discord bot]({{ '/discord-bot/' | relative_url }}) or the Telegram bot
  and give users a 4-digit link code under **Users and link codes**.
  Linked users can create rooms and put their *own* devices in them,
  without an admin.

This works for every room, including rooms that users created.

This is the home of the third-party client workarounds. The plugin's
in-player "Host From Another Device" / "Add a Device to This Room"
pickers are off unless an admin turns on panel bridging (meant for small
servers with trusted users), and even then users can only bridge their
own devices; see [Host Bridge]({{ '/technical/host-bridge/' | relative_url }}). Devices bridged that way
show up here as *Plugin bridge* and can't be added a second time.

## Turning it on

The panel is on by default (opt-out), but it only starts once a password
is set. Without one the session server logs
`Admin panel: NOT started - ADMIN_PASSWORD (or ADMIN_PASSWORD_FILE) is not set`
and runs normally without it.

```yaml
services:
  session-server:
    image: ghcr.io/tigamingtv/jwp-session-server:latest
    ports:
      - "3000:3000"
      - "3001:3001"   # admin panel; "127.0.0.1:3001:3001" keeps it off the LAN
    environment:
      - ALLOWED_ORIGINS=https://jellyfin.example.com
      - JWT_SECRET=${JWT_SECRET}
      - ADMIN_PASSWORD=${ADMIN_PASSWORD}
```

Then open `http://<server>:3001/` and sign in. All admins share the one
account set in the environment.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_ENABLED` | `true` | Set to `false` to switch the panel off. |
| `ADMIN_USERNAME` | `admin` | Login name. |
| `ADMIN_PASSWORD` | (empty) | Login password. **Required**; the panel doesn't start without it. Use a long random value (`openssl rand -base64 18`); shorter than 12 characters logs a warning. |
| `ADMIN_PASSWORD_FILE` | (empty) | Read the password from this file instead (Docker secrets). Used only when `ADMIN_PASSWORD` is empty. |
| `ADMIN_HOST` | `0.0.0.0` | IP address the panel listens on (`0.0.0.0` all IPv4, `::` all IPv6, `127.0.0.1` local only). |
| `ADMIN_PORT` | `3001` | Port the panel listens on; must differ from `PORT`. In the compose files, `ADMIN_PANEL_PORT` sets the published host port. |
| `ADMIN_SESSION_TTL_SECS` | `43200` | How long a login stays valid (12 hours). |
| `ADMIN_COOKIE_SECURE` | `false` | Set to `true` when the panel is served over HTTPS, so the login cookie is never sent over plain HTTP. |
| `ADMIN_TRUST_X_FORWARDED_FOR` | `false` | Behind a reverse proxy, set to `true` so failed-login throttling uses each visitor's address (the last `X-Forwarded-For` entry) instead of the proxy's. Only enable it if the panel can't be reached without the proxy. |
| `ADMIN_EMPTY_GROUP_TTL_SECS` | `600` | A group created in the panel that nobody joins is removed after this long. |
| `JELLYFIN_URL` | (empty) | Jellyfin's address **as seen from the session server**, e.g. `http://jellyfin:8096` on a shared Docker network. Needed for [Jellyfin devices](#jellyfin-devices). |
| `JELLYFIN_API_KEY` | (empty) | A Jellyfin API key (Dashboard > API Keys). Needed for Jellyfin devices. |
| `JELLYFIN_API_KEY_FILE` | (empty) | Read the API key from this file instead (Docker secrets). |
| `BRIDGE_POLL_INTERVAL_MS` | `1000` | How often device positions are read from Jellyfin (minimum 250). Polling only runs while a device is in a room or the panel is open. |

## Groups

A group created in the panel starts empty and has no host. The first
person to arrive becomes its host, whether they join with the password or
an admin adds them. After that a group behaves like any other room: if
the host leaves, the person who joined earliest becomes host, and the
room closes when the last person leaves. A group that nobody joins is
removed after `ADMIN_EMPTY_GROUP_TTL_SECS`. Groups, like all rooms, are
kept in memory only and are lost when the session server restarts.

The **Generate** button fills in a random 12-character password. Tell
your users the group name and password; they join from the room list in
the Watch Party panel.

## Adding and moving people

**Add a client...** in a room lists everyone who is signed in and
connected but not in that room: people in the lobby and people in other
rooms. Adding someone who is in another room moves them: they leave the
old room (which carries on, or picks a new host), join the new one
without needing its password, and their player opens the room's item. The
web client shows "An admin added you to ...".

Connections that haven't signed in yet (when `JWT_SECRET` is set) are
shown but can't be added.

**Remove** takes someone out of a room. They see "An admin removed you
from the room" and are back in the lobby. **Close** ends the room for
everyone.

## Jellyfin devices

Some Jellyfin clients can't show the Watch Party panel: the official
Android TV app, Fladder, Swiftfin, Infuse, Kodi and so on. With
`JELLYFIN_URL` and `JELLYFIN_API_KEY` set, the panel lists every active
Jellyfin client except browsers and Jellyfin Desktop (those run the panel
themselves and join as normal clients), and you can put them into rooms.

### Setup

1. In Jellyfin, open **Dashboard > API Keys**, click **+** and name the key
   (e.g. `JellyWatchParty`).
2. Give it to the session server, with Jellyfin's address as the session
   server sees it:

   ```yaml
   environment:
     - ADMIN_PASSWORD=${ADMIN_PASSWORD}
     - JELLYFIN_URL=http://jellyfin:8096
     - JELLYFIN_API_KEY=${JELLYFIN_API_KEY}
   ```

3. Restart the session server. Its log says
   `Admin panel: Jellyfin devices enabled`, and the panel header shows
   *Jellyfin connected*.

The API key has full admin rights on Jellyfin. Keep it out of version
control (use `.env` or `JELLYFIN_API_KEY_FILE`), and keep the admin
panel private.

### How it works

A device you add becomes a normal member of the room, shown as
*Jellyfin device*. The session server drives it over the Jellyfin API
(the same remote control the Jellyfin dashboard uses), so nothing needs
to be installed on the device.

- **As receiver**: the device follows the room's host. If it isn't
  playing the room's item, it is told to play it from the room's
  position (so an idle TV on its home screen just starts). After that it
  is paused, unpaused and seeked to stay within 2 seconds of the host.
  When the host switches to another item, the device follows. A play
  that is scheduled (the start countdown, or the short delay every play
  has) starts on the device at the same moment as everyone else.
  If someone stops the movie on the device itself, the bridge leaves it
  alone until the host moves on to another item. If the device doesn't
  start the item after three tries, the bridge stops asking and says so.
- **As host**: the room follows the device. Play, pause, seeks and
  switching to another item on the device go to everyone in the room.
  If playback stops on the device, the room pauses. A device host has no
  start countdown: it is already playing, so the room starts with it.

Each device can be in one room at a time. **Make host** works for devices
too: the role is simply whether the device is the room's host right now.
When the host leaves on its own, a person in the room is preferred over
a device as the new host.

**Host only** devices: some apps (Fladder, for example) don't accept
remote control from Jellyfin. They can only be host; as a receiver they
show an error until you make them host or remove them.

### Status and limits

The room shows each device's status and drift from the room (for
example *In sync -0.4s*). *Loading* means the device was told to play the
room's item and hasn't started yet; *Offline* means Jellyfin no longer
lists it (after 90 seconds it leaves the room). Problems (a command
Jellyfin refused, an app that can't be a receiver) show in red under the
device's name.

- Jellyfin only learns a device's position when the device reports
  progress, every few seconds. The session server estimates the position
  in between, so receivers are kept within about 2 seconds, not frame
  accurate like the web client.
- The clocks of the Jellyfin server and the session server don't need to
  agree: the difference is measured from the devices' progress reports
  and taken into account.
- The devices list shows apps active in the last 16 minutes. A device in
  a room stays as long as Jellyfin knows it, even if it sits idle.
- For an `https://` `JELLYFIN_URL` with a certificate from your own
  certificate authority, install that CA in the session server container
  (the system store is trusted, as well as the usual public CAs).
  `HTTPS_PROXY` / `NO_PROXY` are honoured.
- A device starts following when it is added; it doesn't remember its
  room after a session server restart.

## Changing the host

**Make host** gives the host role to another member. The old host becomes
a normal member and starts following the new one. A play that was waiting
for everyone to be ready is cancelled; the new host just presses play
again.

## Sync status

Each member's status is what their client last reported: `synced`,
`syncing` (catching up), `buffering`, `loading` (opening the item or
waiting for a scheduled play), `idle` (not in the player), or
`playing`/`paused` for the host. The dot shows whether the member's
connection is up; a disconnected member keeps their place for 90 seconds
in case they come back.

## Chat bots

When chat integrations are set up (see the
[Discord bot]({{ '/discord-bot/' | relative_url }})), these sections
appear:

- **Discord bot**: turns the bot on, and sets its server, allowed channels,
  roles and limits. The chips show whether the bot container is connected.
- **Telegram bot**: turns the bot on, and sets its group (a negative id;
  send `/groupid` in the group to see it) and limits. **Group
  administrators manage all rooms** lets the group's admins act as owners
  of every room. Telegram has no roles or channel list.
- **Users and link codes**: every Jellyfin user, with their code and a
  column for each platform in use with the linked account. **Assign code**
  / **New code** show a 4-digit code once; the same code links one account
  on each platform. **Unlink** disconnects one platform's account; **New
  code** and **Remove code** disconnect all of them. A code marked
  **Locked** got too many wrong tries; assign a new one.
- **Bot activity**: links, wrong codes (with the platform and account id)
  and room changes. Kept until the server restarts.

Rooms created from a chat show a *Discord room* or *Telegram room* badge
with their owner. You can manage them like any other room.

## Security

- The panel listens on its own port, separate from the public websocket
  port, so you can firewall it or publish it only on `127.0.0.1`. Don't
  expose it to the internet without HTTPS in front of it.
- Logins are throttled: 5 failed attempts per address per minute, and 30
  from everyone together per minute. Failed logins are logged with the
  address (never the password).
- The login is an `HttpOnly`, `SameSite=Strict` cookie, kept in memory
  (restarting the server signs everyone out). Every change needs a custom
  request header, and a foreign `Origin` is refused, which blocks
  cross-site requests.
- Every admin action is logged on the session server as an `admin:` line.
- `JELLYFIN_API_KEY` is a full-admin Jellyfin credential. It is only sent
  to `JELLYFIN_URL`, never to browsers. Use `https://` for `JELLYFIN_URL`
  if the session server reaches Jellyfin over an untrusted network.
- Responses carry a strict Content-Security-Policy, `X-Frame-Options: DENY`
  and `Cache-Control: no-store`.

### Behind a reverse proxy

Give the panel **its own host name** (e.g. `jwp-admin.example.com`)
rather than a path on your Jellyfin site. On the same host, any script
running on the Jellyfin pages (a malicious plugin, an XSS bug) could use a
signed-in admin's panel session. The UI uses relative URLs, so a path
under its own host works too.

Caddy:

```caddy
jwp-admin.example.com {
    reverse_proxy session-server:3001
}
```

nginx:

```nginx
server {
    listen 443 ssl;
    server_name jwp-admin.example.com;
    # ssl_certificate ...;

    location / {
        proxy_pass http://session-server:3001;
        proxy_set_header Host $http_host;
        proxy_set_header X-Forwarded-Host $http_host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

With HTTPS in front, also set `ADMIN_COOKIE_SECURE=true`, and
`ADMIN_TRUST_X_FORWARDED_FOR=true` if port 3001 isn't reachable any other
way.
