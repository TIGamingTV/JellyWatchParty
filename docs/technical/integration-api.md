---
title: Integration API
parent: Technical Reference
nav_order: 7
---

# Integration API (chat bots)

The session server's API for chat sidecars: the
[Discord bot]({{ '/discord-bot/' | relative_url }}) and the
[Telegram bot]({{ '/telegram-bot/' | relative_url }}).
It runs on its own
listener (`INTEGRATION_HOST:INTEGRATION_PORT`, default `127.0.0.1:3002`;
the host must be an IP address and the port must differ from `PORT` and
`ADMIN_PORT`) and only starts when the admin panel, Jellyfin devices
(`JELLYFIN_URL`, `JELLYFIN_API_KEY`) and `DATA_DIR` are set up and at least
one sidecar token is configured. The bots reach it at
`JWP_INTEGRATION_URL` (default `http://session-server:3002`).

Code: `src/server/src/integration/` (`mod.rs` errors, limits and the
empty-room reaper, `config.rs` environment, `api.rs` HTTP, `actions.rs`
permissions and room operations, `store.rs` data file and codes, `view.rs`
room list), with admin endpoints in `src/server/src/admin/integrations.rs`.
The bots live in `src/integrations/discord-bot/` (one crate for Discord and
Telegram).

## Authentication

Every request needs `Authorization: Bearer <token>`. The token decides the
platform: `DISCORD_INTEGRATION_TOKEN` means `discord`,
`TELEGRAM_INTEGRATION_TOKEN` means `telegram`. Each platform needs its own
token, so one bot can never act as another: tokens must be at least 32
characters and different from each other (or the integration doesn't
start), and each can be read from a file with the `_FILE` suffix. Tokens
are compared as SHA-256 digests in constant time. A wrong or missing token
gets `401 {"reason":"unauthorized"}`; an unknown path gets `404
{"reason":"invalid"}`. Bodies are limited to 16 KiB.

User actions carry an `actor`: what the platform said about the person
asking.

```json
{ "id": "123456789012345678", "name": "alice", "guild_id": "...", "channel_id": "...", "roles": ["..."] }
```

| Field | Discord | Telegram |
|---|---|---|
| `id` | User snowflake | User id |
| `guild_id` | Server (guild) id | The group's chat id (negative) |
| `channel_id` | Channel id | The chat the request came from |
| `roles` | Role ids | `["admin"]` for group administrators, else empty |

Before running an action, the server checks:

1. `id` is a positive number.
2. The account is under its rate limit (30 requests per minute).
3. The platform is enabled.
4. `guild_id` is the configured server or group.
5. `channel_id` is allowed, if channels are restricted (Discord only).
6. The required role is in `roles`, if one is configured (Discord only).

For everything except `link` and `unlink`, it then also checks that the
account is linked, and that the Jellyfin user exists and is enabled.
Jellyfin's user list is cached for 60 s (a stale copy is used for up to
10 minutes while Jellyfin is unreachable). Jellyfin administrators, and
holders of the platform's `admin_role_id`, count as admins.

`link` checks `username` (case-insensitive) and the 4-digit `code`
(spaces and dashes ignored). Wrong attempts are limited (`locked_out`
with `retry_after_ms`): 5 per chat account per 15 minutes, 50 from
everyone per 10 minutes. 10 wrong codes for one Jellyfin user freeze that
code until an admin assigns a new one.

## Errors

`{"error": "<message for the user>", "reason": "<code>", "retry_after_ms"?: n}`

| reason | When |
|---|---|
| `unauthorized` | Bad token |
| `invalid` | Malformed request |
| `disabled`, `not_configured`, `wrong_guild`, `channel_not_allowed`, `missing_role` | Platform policy |
| `rate_limited` | Over 30 requests per minute (`retry_after_ms`) |
| `not_linked`, `account_disabled`, `target_not_linked` | Linking |
| `bad_credentials`, `locked_out`, `already_linked_elsewhere` | `link` |
| `not_found`, `member_not_found` | Room or member gone |
| `not_participant`, `not_owner`, `not_your_device`, `is_owner`, `owner_cannot_leave` | Permissions |
| `wrong_password`, `too_many_attempts` | Room password |
| `password_required`, `room_limit`, `room_full`, `role_not_allowed` | Limits and settings |
| `session_not_found`, `runs_web_client`, `no_remote_control`, `device_already_bridged`, `jellyfin_unavailable` | Adding a device |
| `democratic_off`, `not_democratic` (403), `not_started` (409) | Democratic mode: the platform's `democratic_commands` is off; the room is host-only; nothing is playing yet |

## Endpoints (`/v1`)

| Method and path | Body | Answer |
|---|---|---|
| `GET /config` | | `{provider, version, settings}`. `version` changes when an admin saves settings. |
| `POST /heartbeat` | `{bot_name}` | `{ok, now}`. Marks the bot as online in the admin panel (for 90 s). |
| `GET /rooms?since=<v>` | | `{version, rooms}`. With `since`, waits up to 25 s for a change after version `v`. |
| `PUT /rooms/{id}/panel` | `{channel_id, message_id}` | Remembers where the room's panel message is. `channel_id` may be negative (Telegram groups). |
| `POST /link` | `{actor, username, code}` | `{ok, user_name}` |
| `POST /unlink` | `{actor}` | `{ok}` |
| `POST /me` | `{actor}` | `{user_id, user_name, is_admin, owns, joined}` (room ids) |
| `POST /devices` | `{actor}` | `{devices: [{session_id, device_name, client, remote_control, now_playing, bridged_as, room_id}]}`. Only the actor's own sessions that don't run the web client and are recently active or already bridged. |
| `POST /rooms` | `{actor, name, password?}` | `{ok, id, name}`. The creator is owner and first participant. `password_required` if the platform requires one; `room_limit` over `max_rooms_per_user` (not for admins) or `max_rooms_total`. |
| `POST /rooms/{id}/join` | `{actor, password?}` | `{ok}`, or `{ok, already: true}`. Admins skip the password; 5 wrong ones per minute give `too_many_attempts`. At most 50 participants. |
| `POST /rooms/{id}/leave` | `{actor}` | `{ok}`. Also removes the actor's devices from the room. The owner can't leave. |
| `POST /rooms/{id}/update` | `{actor, name?, password?}` | `{ok}`. `password`: absent keeps it, `null`/`""` removes it. Owner only. |
| `POST /rooms/{id}/close` | `{actor}` | `{ok}`. Owner only. |
| `POST /rooms/{id}/owner` | `{actor, to}` | `{ok}`. `to` is a linked chat account id that has joined the room. Owner only. |
| `POST /rooms/{id}/kick` | `{actor, member}` or `{actor, user}` | `{ok}`. A member (client id), or a participant by account id together with their devices. Owner only. |
| `POST /rooms/{id}/host` | `{actor, member}` | `{ok}`. Owner only. |
| `POST /rooms/{id}/devices` | `{actor, session_id, role: "host"\|"receiver"}` | `{ok, member_id}`. Participants (and admins) only; the role must be allowed (`allow_host` / `allow_receiver`). `host` is allowed for the owner and admins, or when the room has no host. |
| `POST /rooms/{id}/devices/remove` | `{actor, member}` | `{ok}`. Own devices; the owner can remove any member. |
| `POST /rooms/{id}/democratic` | `{actor, enabled}` | `{ok, democratic}`. Owner only; needs `democratic_commands`. Sends `democratic_changed` to the room. |
| `POST /rooms/{id}/playback` | `{actor, action: "play"\|"pause"}`, `{actor, action: "seek", position}` or `{actor, action: "seek", offset}` | `{ok, action, position}`. Participants (and the owner and admins) of a democratic room that has started, has media and has members; needs `democratic_commands`. `position` seeks to that second, `offset` by that many from the room's current position (negative: back); the result is clamped to 0–86400. Any other combination is `invalid`. |

"Owner only" also allows admins: Jellyfin administrators, or holders of
the configured admin role (on Telegram: group administrators, when
**Group administrators manage all rooms** is on).

### Democratic mode

`playback` goes through `ws::server_command`: it updates the room as a
member's `player_event` would and sends the event to **every** member, the
host included, with `by: ""` and `by_name` set to the chat user's Jellyfin
name. `play` and `pause` use the room's current expected position. The
host's `state_update`s are ignored for the 2 s command cooldown afterwards
so its old state can't undo the command (see
[Protocol: Democratic mode]({{ '/technical/protocol/' | relative_url }}#democratic-mode)).
Both endpoints add to the activity log (`room_democratic`,
`room_playback`). The bots only offer these commands while
`democratic_commands` is on, but the server checks it on every request.

Rooms, limits and settings are per platform: a bot only sees and reaches
the rooms created on its own platform, and `max_rooms_per_user` /
`max_rooms_total` count only those.

A room in `GET /rooms` (see `src/integrations/fixtures/rooms.json`, which
tests on both sides check):

```json
{
  "id": "…", "name": "…", "has_password": true, "democratic": false,
  "owner": { "user_id": "…", "name": "Alice", "external_id": "…" },
  "participants": [{ "user_id": "…", "name": "…", "external_id": "…" | null }],
  "host": { "id": "…", "name": "…" } | null,
  "members": [{ "id": "…", "name": "…", "kind": "jellyfin|web|plugin_bridge", "is_host": true,
                "status": "…", "owner_user_id": "…" | null, "owner_external_id": "…" | null }],
  "media_id": "…" | null, "play_state": "playing|paused",
  "panel": { "channel_id": "…", "message_id": "…" } | null,
  "created_at": 0, "empty_since": 0 | null
}
```

## Chat rooms

A room created through the API (`Room.chat`, `types::ChatRoom`) has a
platform, an owner and participants. All of them are Jellyfin users,
identified by their normalized id (32 lowercase hex characters).

- **Like an admin group:** it starts empty and hostless, with no start
  countdown. While it has no host, the next Watch Party panel user to join,
  or the next device added with `role: "host"`, becomes host. A device
  added as receiver doesn't.
- **Stays open when empty:** when the last member leaves, the room stays
  open without a host (`room/leave.rs`). The integration's reaper closes it
  after the platform's `empty_room_minutes`.
- **Device ownership:** `Bridges::add(.., owner)` refuses a session whose
  `UserId` isn't the owner (`AddError::NotYourDevice`). The check uses the
  same fresh `/Sessions` result that the device is added from.
- **Change signal:** `events::bump()` runs on every room-list or
  participant broadcast. It wakes the long poll.

## Data file

`DATA_DIR/integrations.json` (mode 0600; written to a temp file, synced,
then renamed):

```json
{ "version": 1,
  "settings": { "discord": { "enabled": true, "guild_id": "…", … },
                "telegram": { "enabled": true, "guild_id": "-100…", "admin_role_id": "admin", … } },
  "users": { "<jellyfin id>": { "name": "Alice", "code_hmac": "…", "assigned_at": 0,
                                "failed": 0, "frozen": false,
                                "links": { "discord": { "external_id": "…", "display_name": "…", "linked_at": 0 },
                                           "telegram": { … } } } } }
```

One code links one account on each platform: the code isn't used up, and
links are kept per platform. **New code** and **Remove code** drop the
links on every platform; **Unlink** only drops one.

Settings are checked per platform when saved:

| Setting | Discord | Telegram |
|---|---|---|
| `guild_id` | Server id (digits) | Group id (a negative number; the bot's `/groupid` shows it) |
| `channel_ids` | Up to 25 channel ids | Must be empty |
| `required_role_id` | Role id | Must be empty |
| `admin_role_id` | Role id | Empty, or `admin` (group administrators) |

`enabled` (default `false`) needs `guild_id`. The other settings are the
same on both platforms:

| Setting | Default | Range |
|---|---|---|
| `max_rooms_per_user` | `2` | 1–10 |
| `max_rooms_total` | `20` | 1–100 |
| `empty_room_minutes` | `30` | 5–1440 |
| `require_password` | `false` | |
| `allow_host` / `allow_receiver` | `true` | |
| `democratic_commands` | `false` | |

Missing fields (and a missing `telegram` block) load with their
defaults.

`code_hmac` is HMAC-SHA256 over `"jwp-link-code\0" || user id || "\0" ||
code`, keyed with `DATA_DIR/secret.key` (32 random bytes, created on first
start). Codes are 4 digits and never stored. A file that doesn't parse, or
was written by a newer server, stops the integration from starting; it is
never overwritten.

## Admin endpoints (admin panel, session and CSRF header required)

| Method and path | |
|---|---|
| `GET /api/integrations` | Availability, whether the API is listening, and per platform: token set, bot heartbeat, settings |
| `PUT /api/integrations/{provider}` | Save a platform's settings (`discord`, `telegram`; validated per platform) |
| `GET /api/users` | Jellyfin users with code state and links. Users deleted from Jellyfin show as `missing`. |
| `POST /api/users/{id}/code` | Assign a new code: returns `{code}` once, drops links, resets the count and lock |
| `DELETE /api/users/{id}/code` | Remove the code and links |
| `DELETE /api/users/{id}/links/{provider}` | Unlink |
| `GET /api/audit` | Activity log, newest first (last 500 entries, in memory) |

## Adding a platform

The server side is per platform. Add the platform and its token variable
to `integration::config::PROVIDERS`, a settings slot to `store::Settings`
(with `#[serde(default)]`, so older data files still load), and its rules
to `ChatSettings::validate`. The admin panel lists platforms in `PLATFORMS`
in `admin/ui/app.js`. A sidecar then only needs to map its platform's
users, groups and roles onto `actor`.
