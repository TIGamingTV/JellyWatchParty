---
title: Integration API
parent: Technical Reference
nav_order: 7
---

# Integration API (chat bots)

The session server's API for chat sidecars such as the
[Discord bot]({{ '/discord-bot/' | relative_url }}). It runs on its own
listener (`INTEGRATION_HOST:INTEGRATION_PORT`, default `127.0.0.1:3002`)
and only starts when the admin panel, Jellyfin devices and `DATA_DIR` are
set up and at least one sidecar token is configured.

Code: `src/server/src/integration/` (`api.rs` HTTP, `actions.rs`
permissions and room operations, `store.rs` data file and codes, `view.rs`
room list), with admin endpoints in `src/server/src/admin/integrations.rs`.

## Authentication

Every request needs `Authorization: Bearer <token>`. The token decides the
platform: `DISCORD_INTEGRATION_TOKEN` means `discord`. Tokens are compared
as SHA-256 digests in constant time. A wrong or missing token gets `401
{"reason":"unauthorized"}`. Bodies are limited to 16 KiB.

User actions carry an `actor`: what the platform said about the person
asking.

```json
{ "id": "123456789012345678", "name": "alice", "guild_id": "...", "channel_id": "...", "roles": ["..."] }
```

Before running an action, the server checks:

1. `id` is numeric.
2. The account is under its rate limit (30 requests per minute).
3. The platform is enabled.
4. `guild_id` is the configured server.
5. `channel_id` is allowed, if channels are restricted.
6. The required role is in `roles`, if one is configured.

For everything except `link`, it then also checks that the account is
linked, and that the Jellyfin user exists and is enabled. `GET /Users` is
cached for 60 s.

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

## Endpoints (`/v1`)

| Method and path | Body | Answer |
|---|---|---|
| `GET /config` | | `{provider, version, settings}`. `version` changes when an admin saves settings. |
| `POST /heartbeat` | `{bot_name}` | `{ok, now}`. Marks the bot as online in the admin panel. |
| `GET /rooms?since=<v>` | | `{version, rooms}`. With `since`, waits up to 25 s for a change after version `v`. |
| `PUT /rooms/{id}/panel` | `{channel_id, message_id}` | Remembers where the room's panel message is. |
| `POST /link` | `{actor, username, code}` | `{ok, user_name}` |
| `POST /unlink` | `{actor}` | |
| `POST /me` | `{actor}` | `{user_id, user_name, is_admin, owns, joined}` |
| `POST /devices` | `{actor}` | `{devices: [{session_id, device_name, client, remote_control, now_playing, bridged_as, room_id}]}`. Only the actor's own sessions. |
| `POST /rooms` | `{actor, name, password?}` | `{ok, id, name}` |
| `POST /rooms/{id}/join` | `{actor, password?}` | `{ok, already?}` |
| `POST /rooms/{id}/leave` | `{actor}` | Also removes the actor's devices from the room. |
| `POST /rooms/{id}/update` | `{actor, name?, password?}` | `password`: absent keeps it, `null`/`""` removes it. Owner only. |
| `POST /rooms/{id}/close` | `{actor}` | Owner only. |
| `POST /rooms/{id}/owner` | `{actor, to}` | `to` is a chat account id that has joined the room. Owner only. |
| `POST /rooms/{id}/kick` | `{actor, member}` or `{actor, user}` | A member (client id), or a participant by account id together with their devices. Owner only. |
| `POST /rooms/{id}/host` | `{actor, member}` | Owner only. |
| `POST /rooms/{id}/devices` | `{actor, session_id, role: "host"\|"receiver"}` | `{ok, member_id}`. Participants only. `host` is allowed for the owner, or when the room has no host. |
| `POST /rooms/{id}/devices/remove` | `{actor, member}` | Own devices; the owner can remove any member. |

"Owner only" also allows admins: Jellyfin administrators, or holders of
the configured admin role.

A room in `GET /rooms` (see `src/integrations/fixtures/rooms.json`, which
tests on both sides check):

```json
{
  "id": "…", "name": "…", "has_password": true,
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
  "settings": { "discord": { "enabled": true, "guild_id": "…", … } },
  "users": { "<jellyfin id>": { "name": "Alice", "code_hmac": "…", "assigned_at": 0,
                                "failed": 0, "frozen": false,
                                "links": { "discord": { "external_id": "…", "display_name": "…", "linked_at": 0 } } } } }
```

`code_hmac` is HMAC-SHA256 over `"jwp-link-code\0" || user id || "\0" ||
code`, keyed with `DATA_DIR/secret.key` (32 random bytes, created on first
start). A file that doesn't parse stops the integration from starting; it is
never overwritten.

## Admin endpoints (admin panel, session and CSRF header required)

| Method and path | |
|---|---|
| `GET /api/integrations` | Availability, whether the API is listening, and per platform: token set, bot heartbeat, settings |
| `PUT /api/integrations/discord` | Save settings (validated) |
| `GET /api/users` | Jellyfin users with code state and links. Users deleted from Jellyfin show as `missing`. |
| `POST /api/users/{id}/code` | Assign a new code: returns `{code}` once, drops links, resets the count and lock |
| `DELETE /api/users/{id}/code` | Remove the code and links |
| `DELETE /api/users/{id}/links/{provider}` | Unlink |
| `GET /api/audit` | Activity log, newest first (last 500 entries, in memory) |

## Adding a platform

The server side is per platform. Add the platform and its token variable
to `integration::config::PROVIDERS` and a settings slot to
`store::Settings`. A sidecar then only needs to map its platform's users,
groups and roles onto `actor`.
