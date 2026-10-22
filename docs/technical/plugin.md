---
title: Plugin
parent: Technical Reference
nav_order: 4
---

# Jellyfin Plugin (C#)

## Overview

The JellyWatchParty plugin injects the client into Jellyfin's web
interface, serves the client JavaScript, issues JWTs for the session
server, and hosts the [Host Bridge]({{ '/technical/host-bridge/' | relative_url }}). It targets
Jellyfin 12.x only.

## Project Structure

```
src/plugins/jellyfin/
├── Directory.Build.props              # TargetFrameworks, JellyfinPackageVersion, JellyfinTargetAbi
├── JellyWatchParty/
│   ├── Plugin.cs                      # Plugin entry point
│   ├── JellyWatchPartyPlugin.csproj   # Project file
│   ├── FileTransformationIntegration.cs  # Startup task: File Transformation registration / direct index.html write
│   ├── ScriptInjectionMiddleware.cs   # Request-level fallback, startup filter, DI registration
│   ├── Controllers/
│   │   └── JellyWatchPartyController.cs  # REST API endpoints
│   ├── Configuration/
│   │   └── PluginConfiguration.cs     # Configuration model
│   ├── Services/                      # Host Bridge (see host-bridge.md)
│   │   ├── HostBridgeManager.cs       # Hosted service, owns active bridges
│   │   ├── SessionHostBridge.cs       # Host bridge: session → session-server WS
│   │   ├── SessionFollowerBridge.cs   # Receiver bridge: room → remote-control commands
│   │   ├── SessionServerAuth.cs       # Shared JWT minting
│   │   └── BridgeDtos.cs              # BridgeableSessionInfo, BridgeStatus
│   └── Web/
│       ├── configPage.html            # Admin configuration page
│       └── plugin.js, state.js, …     # Build output: copied from src/clients/jellyfin-web (gitignored)
└── JellyWatchParty.Tests/             # xUnit/Moq test project
```

## Plugin.cs

`Plugin : BasePlugin<PluginConfiguration>, IHasWebPages`.

- `Name` `"JellyWatchParty"`, `Id` from `PluginGuid`
  (`0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d`), `Description` "Watch movies
  together in sync with friends", `PluginVersion` (assembly version,
  3 parts).
- `Instance`: the static singleton (standard Jellyfin pattern), used by
  controllers and services to read the configuration.
- The constructor logs whether JWT auth is enabled: warns when `JwtSecret`
  is empty, or shorter than `PluginConfiguration.MinJwtSecretLength` (32),
  which also disables auth; logs any `ValidateSessionServerUrl` warnings;
  and compares the Jellyfin version it was built against with the running
  one (`LogServerCompatibility`), warning when the server is a newer major
  version.
- `InjectionEnabled` (static, default `true`) gates all three injection
  paths. `OnUninstalling()` sets it to `false` and removes the script tag
  from a directly modified `index.html`
  (`FileTransformationIntegration.RemoveInjectedScriptFromIndexHtml`).
- `GetPages()` registers the embedded `Web/configPage.html` as the
  configuration page.

## Script Injection

The client is loaded by a `<script src="../JellyWatchParty/ClientScript"
defer></script>` tag in Jellyfin Web's `index.html`, inserted before
`</body>` (or `</head>`) by `FileTransformationIntegration.InjectScript`,
which does nothing if a `JellyWatchParty/ClientScript` tag is already
present. Three layers add it:

1. **File Transformation plugin** (preferred).
   `FileTransformationIntegration` is an `IScheduledTask` with a startup
   trigger (*JellyWatchParty File Transformation Registration*). It finds
   the `Jellyfin.Plugin.FileTransformation` assembly by reflection and
   calls `PluginInterface.RegisterTransformation` with the pattern
   `"index.html"` and the callback `TransformIndexHtml`. The pattern must
   be the literal `index.html`: File Transformation picks one pipeline per
   file, preferring an exact key, and other plugins register that key, so
   any other spelling would end up in a pipeline that is never used.
2. **Direct `index.html` write.** If File Transformation is absent or
   registration fails, the same task writes the tag into
   `$JELLYFIN_WEB_DIR/index.html`, but only when `JELLYFIN_WEB_DIR` is set
   and the file is writable. Otherwise it logs that injection is not
   automatic and suggests adding the tag by hand ("Custom HTML"). Whether a
   Custom HTML field exists depends on the Jellyfin version; the middleware
   below usually makes this unnecessary.
3. **`ScriptInjectionMiddleware`** (last resort). Registered through an
   `IStartupFilter` (`ScriptInjectionStartupFilter`, by
   `ServiceRegistrator`) ahead of Jellyfin's static files. For `/web` and
   `/web/index.html` it serves `index.html` from `JELLYFIN_WEB_DIR` (default
   `/usr/share/jellyfin/web`) with the tag added, ETag-cached
   (`Cache-Control: no-cache`; a failed load is retried on the next
   request). When File Transformation is loaded it stands down, because
   answering the request itself would drop every other plugin's
   transformations; but if File Transformation serves a `200` for
   `index.html` without ever calling `TransformIndexHtml`, the middleware
   logs a warning and takes over from the next request on.

`ServiceRegistrator` also registers `HostBridgeManager` as a singleton and
as a hosted service.

## JellyWatchPartyController.cs

ASP.NET Core controller at route `JellyWatchParty` (see the
[REST API Reference](#rest-api-reference) for the endpoint table).

### Client scripts

`GET /JellyWatchParty/ClientScript` returns the embedded loader
(`JellyWatchParty.Plugin.Web.plugin.js`), loaded once through a
`Lazy<T>`. `GET /JellyWatchParty/Client/{*path}` (`GetClientModule`)
returns any other embedded module, e.g. `Client/playback/sync.js`; paths
containing `..` or not ending in `.js` get `404`. Both send
`Cache-Control: public, max-age=3600` and an ETag (first 16 Base64
characters of the content's SHA-256) and answer a matching
`If-None-Match` with `304`. See [Client]({{ '/technical/client/' | relative_url }}) for the module list.

### `GET /JellyWatchParty/Token`

Requires a Jellyfin-authenticated user (`[Authorize]`). The user id comes
from the `NameIdentifier` or `Jellyfin-UserId` claim (missing: `401`), the
name from the `Name` claim (fallback `"User"`). Rate limit: 30 requests
per minute per user (`MaxTokensPerMinute`; the controller's XML doc
comment still says 10), answered with `429`. When `HasUsableJwtSecret` is
false (secret empty or shorter than 32 characters), it answers with
`token: null` and `auth_enabled: false`; otherwise it mints a token via
`SessionServerAuth.CreateToken`.

### JWT generation (`Services/SessionServerAuth.cs`) {#jwt-generation}

HS256 over the UTF-8 bytes of `JwtSecret` (signing credentials cached
until the secret changes; a secret under 32 characters throws). Claims:
`sub` (Jellyfin user id), `name`, `iat`, `jti` (random GUID), plus `aud`
(`JwtAudience`), `iss` (`JwtIssuer`) and `exp` (now + `TokenTtlSeconds`)
set through the `JwtSecurityToken` constructor, so they serialize as
strings rather than arrays. The host bridges use the same method with the
bridged session's owner.

## PluginConfiguration.cs

| Property | Default | Notes |
|---|---|---|
| `JwtSecret` | `""` | `null` becomes `""`. Empty or shorter than `MinJwtSecretLength` (32): auth disabled (`HasUsableJwtSecret` false). |
| `JwtAudience` | `"JellyWatchParty"` | Must match the session server. |
| `JwtIssuer` | `"Jellyfin"` | Must match the session server. |
| `TokenTtlSeconds` | `3600` | Clamped to 60–86400. |
| `InviteTtlSeconds` | `3600` | Clamped to 60–86400. Not used anywhere and not shown on the configuration page. |
| `SessionServerUrl` | `""` | Empty: clients use `ws(s)://<page host>:3000/ws`. Required for the Host Bridge. |
| `HideNativeSyncButton` | `false` | The web client hides Jellyfin's SyncPlay button. |
| `AllowDemocraticMode` | `false` | Hosts on Jellyfin Web / Desktop get the *Everyone can control playback* switch. Only a UI gate: the session server never sees it, and admins can turn democratic mode on per room from the admin panel regardless. |
| `EnablePanelBridging` | `false` | Master switch for bridging from the Watch Party panel. |
| `AllowThirdPartyClientHost` | `false` | With the master switch: host role allowed (`PanelHostAllowed`). |
| `AllowSupportedClientReceiver` | `false` | With the master switch: receiver role allowed (`PanelReceiverAllowed`). |

`ValidateSessionServerUrl(value)` returns warnings (not an absolute URL,
scheme not `ws`/`wss`, missing host, a bare internal/Docker hostname) and
never rejects a value; `Plugin` logs them at startup.

## configPage.html

The admin page (embedded resource) loads the plugin id from `GET
/JellyWatchParty/Info` (falling back to the hard-coded GUID) and reads and
writes the configuration through `ApiClient.getPluginConfiguration` /
`updatePluginConfiguration`. Sections:

- **Authentication:** *JWT Secret* (password field with a show/hide
  toggle), *JWT Audience*, *JWT Issuer*, *Token TTL (seconds)*
  (60–86400).
- **Session Server:** *Session Server URL* (placeholder "Auto-detect (same
  host, port 3000)") with a live warning line (on input/blur) that
  mirrors the C# and client validation and never blocks saving.
- **Interface:** *Hide Jellyfin's built-in SyncPlay button*
  (`HideNativeSyncButton`) and *Allow democratic mode*
  (`AllowDemocraticMode`); both take effect on the next page load.
- **Watch Party Panel Bridging (trusted servers only):** *Let users bridge
  their devices from the Watch Party panel* (`EnablePanelBridging`), *Allow
  third-party clients to host* and *Allow supported clients as
  receivers*; the two role options are disabled while the master switch is
  off.
- **Save.**

The page sees the whole configuration, JWT secret included, through
Jellyfin's admin-only plugin configuration API. The secret never reaches
non-admins or the injected client: `/Token` returns a signed token.

## Embedded Resources

```xml
<ItemGroup>
  <EmbeddedResource Include="Web\configPage.html" />
  <EmbeddedResource Include="Web\**\*.js" />
  <None Include="thumb.png" CopyToOutputDirectory="PreserveNewest" CopyToPublishDirectory="PreserveNewest" />
</ItemGroup>
```

The `Web/**/*.js` files are not in git: `just build plugin` and CI copy
them from `src/clients/jellyfin-web/` before building. Resource names
follow the root namespace, e.g. `JellyWatchParty.Plugin.Web.plugin.js` and
`JellyWatchParty.Plugin.Web.playback.sync.js`.

## Dependencies

`Directory.Build.props` sets `TargetFrameworks` `net10.0`,
`JellyfinPackageVersion` `12.0.0` and `JellyfinTargetAbi` `12.0.0.0`
(Jellyfin 12.x only; Jellyfin 10.11.x needs an older release):

```xml
<ItemGroup>
  <FrameworkReference Include="Microsoft.AspNetCore.App" />
  <PackageReference Include="Jellyfin.Controller" Version="$(JellyfinPackageVersion)" ExcludeAssets="runtime" />
  <PackageReference Include="Jellyfin.Model" Version="$(JellyfinPackageVersion)" ExcludeAssets="runtime" />
  <PackageReference Include="Newtonsoft.Json" Version="13.0.3" ExcludeAssets="runtime" />
  <PackageReference Include="System.IdentityModel.Tokens.Jwt" Version="6.35.0" />
  <PackageReference Include="Microsoft.IdentityModel.Tokens" Version="6.35.0" />
</ItemGroup>
```

## Building

Requires the .NET 10 SDK:

```bash
# From the project root: copies the client JS into Web/, then builds in Docker
just build plugin

# Or directly (after copying the client JS into Web/)
dotnet build
```

`just build plugin` writes the plugin to
`src/plugins/jellyfin/JellyWatchParty/dist/`; a plain `dotnet build`
writes to `bin/Debug/net10.0/`.

## REST API Reference

**Base URL:** `http(s)://<jellyfin-host>:<port>/JellyWatchParty`

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/JellyWatchParty/ClientScript` | None | Client JS loader (`plugin.js`), ETag-cached |
| `GET` | `/JellyWatchParty/Client/{*path}` | None | Individual client module by path, e.g. `Client/playback/sync.js` |
| `GET` | `/JellyWatchParty/Info` | None | `{id, name, version}` (used by the configuration page) |
| `GET` | `/JellyWatchParty/Token` | Jellyfin auth | Issues a JWT (or the auth-disabled response) for the current user, plus the client settings |
| `GET` | `/JellyWatchParty/Bridge/Sessions` | Jellyfin auth | The caller's own sessions that can be bridged (admins: all); empty unless a panel role is allowed |
| `GET` | `/JellyWatchParty/Bridge/Status` | Jellyfin auth | The caller's active bridges (admins: all) |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Start` | Jellyfin auth, session owner or admin | Bridge a session in as the host of a new room |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Follow?roomId=…` | Jellyfin auth, session owner or admin | Attach a session to a room as a receiver |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Stop` | Jellyfin auth, bridge owner or admin | Stop an active bridge |

Start and Follow answer `400 {"error": …}` while the role is not allowed
("Bridging devices from the Watch Party panel is turned off on this
server. …"), when the session is gone, or when starting fails (e.g.
"Session Server URL is not configured.", "Session '…' is already
bridged.", or the session server's refusal), and `403` ("You can only
bridge your own devices.") for someone else's session. Ownership compares
the session's `UserId` with the caller's `Jellyfin-UserId` claim; callers
in the `Administrator` role may bridge any session. Stop also works with
panel bridging off, and answers `200` when there is no such bridge. See
[Host Bridge]({{ '/technical/host-bridge/' | relative_url }}) for response fields.

### GET /JellyWatchParty/Token

```bash
curl -H 'Authorization: MediaBrowser Token="YOUR_API_KEY"' "http://localhost:8096/JellyWatchParty/Token"
```

```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "auth_enabled": true,
  "expires_in": 3600,
  "user_id": "abc123",
  "user_name": "John",
  "session_server_url": "",
  "hide_native_sync_button": false,
  "allow_third_party_host": false,
  "allow_supported_receiver": false,
  "allow_democratic_mode": false
}
```

When auth is disabled, `token` is `null`, `auth_enabled` is `false` and
there is no `expires_in`; the other fields are sent either way.
`session_server_url` is `SessionServerUrl`, `hide_native_sync_button` is
`HideNativeSyncButton`, `allow_third_party_host` / `allow_supported_receiver`
are `PanelHostAllowed` / `PanelReceiverAllowed` (master switch and role
combined), and `allow_democratic_mode` is `AllowDemocraticMode` (the
client shows the host's switch when it is `true`, or when the room already
is democratic).

| Status | Body |
|--------|------|
| 200 | Token or auth-disabled response |
| 401 | `{"error": "User identity not found in claims"}` (or Jellyfin's own 401 when not authenticated) |
| 429 | `{"error": "Rate limit exceeded. Try again later."}` (30 per minute per user) |
| 500 | `{"error": "Plugin not configured"}` |

### Configuration API

Plugin configuration goes through Jellyfin's standard plugin configuration
endpoints (admin only):

```bash
# Read
curl -H "Authorization: MediaBrowser Token=\"$TOKEN\"" \
  "http://localhost:8096/Plugins/0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d/Configuration"

# Update (send the whole object back)
curl -X POST \
  -H "Authorization: MediaBrowser Token=\"$TOKEN\"" -H "Content-Type: application/json" \
  -d '{"JwtSecret": "...", "JwtAudience": "JellyWatchParty", "JwtIssuer": "Jellyfin", "TokenTtlSeconds": 3600, "SessionServerUrl": "", "HideNativeSyncButton": false, "AllowDemocraticMode": false, "EnablePanelBridging": false, "AllowThirdPartyClientHost": false, "AllowSupportedClientReceiver": false}' \
  "http://localhost:8096/Plugins/0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d/Configuration"
```

See [Configuration]({{ '/configuration/' | relative_url }}) for the field reference and examples.

### WebSocket API

The session server uses WebSocket, not REST, for real-time
communication: `ws(s)://<host>:3000/ws`. See
[Protocol]({{ '/technical/protocol/' | relative_url }}) for the message specification.
