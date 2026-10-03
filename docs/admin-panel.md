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
  password**, and **close** it.

This works for every room, including rooms that users created.

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
| `ADMIN_HOST` | `0.0.0.0` | Address the panel listens on. |
| `ADMIN_PORT` | `3001` | Port the panel listens on. In the compose files, `ADMIN_PANEL_PORT` sets the published host port. |
| `ADMIN_SESSION_TTL_SECS` | `43200` | How long a login stays valid (12 hours). |
| `ADMIN_COOKIE_SECURE` | `false` | Set to `true` when the panel is served over HTTPS, so the login cookie is never sent over plain HTTP. |
| `ADMIN_TRUST_X_FORWARDED_FOR` | `false` | Behind a reverse proxy, set to `true` so failed-login throttling uses each visitor's address (the last `X-Forwarded-For` entry) instead of the proxy's. Only enable it if the panel can't be reached without the proxy. |
| `ADMIN_EMPTY_GROUP_TTL_SECS` | `600` | A group created in the panel that nobody joins is removed after this long. |

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
- Responses carry a strict Content-Security-Policy, `X-Frame-Options: DENY`
  and `Cache-Control: no-store`.

### Behind a reverse proxy

The UI uses relative URLs, so it also works under a path. Example for
Caddy, serving the panel at `https://jellyfin.example.com/jwp-admin/`:

```caddy
jellyfin.example.com {
    handle_path /jwp-admin/* {
        reverse_proxy session-server:3001
    }
}
```

nginx:

```nginx
location /jwp-admin/ {
    proxy_pass http://session-server:3001/;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}
```

With HTTPS in front, also set `ADMIN_COOKIE_SECURE=true`, and
`ADMIN_TRUST_X_FORWARDED_FOR=true` if port 3001 isn't reachable any other
way.
