---
title: Plugin
parent: Technical Reference
nav_order: 4
---

# Jellyfin Plugin (C#)

## Overview

The JellyWatchParty plugin integrates with Jellyfin's plugin architecture to serve the client JavaScript and provide configuration management.

## Project Structure

```
src/plugins/jellyfin/
├── JellyWatchParty/
│   ├── Plugin.cs                     # Plugin entry point
│   ├── JellyWatchPartyPlugin.csproj   # Project file
│   ├── Controllers/
│   │   └── JellyWatchPartyController.cs  # REST API endpoints
│   ├── Configuration/
│   │   └── PluginConfiguration.cs    # Configuration model
│   ├── Services/                     # Host Bridge (see host-bridge.md)
│   │   ├── HostBridgeManager.cs      # Hosted service, owns active bridges
│   │   ├── SessionHostBridge.cs      # One bridge: session → session-server WS
│   │   └── SessionServerAuth.cs      # Shared JWT minting
│   └── Web/
│       ├── configPage.html           # Admin configuration page
│       └── plugin.js                 # Client JS loader (fetches modules
│                                      # individually, not a pre-bundled script)
└── JellyWatchParty.Tests/              # xUnit/Moq test project
```

## Plugin.cs

### Description
The plugin entry point. Implements `BasePlugin<PluginConfiguration>` and `IHasWebPages`.

### Key Elements

```csharp
public class Plugin : BasePlugin<PluginConfiguration>, IHasWebPages
{
    // Singleton instance - standard Jellyfin plugin pattern
    public static Plugin? Instance { get; private set; }

    public Plugin(IApplicationPaths applicationPaths, IXmlSerializer xmlSerializer, ILogger<Plugin> logger)
        : base(applicationPaths, xmlSerializer)
    {
        Instance = this;

        // Log JWT configuration status
        if (string.IsNullOrEmpty(Configuration.JwtSecret))
        {
            _logger.LogWarning("[JellyWatchParty] JwtSecret not configured. Authentication DISABLED.");
        }
    }

    public override string Name => "JellyWatchParty";
    public override Guid Id => new("0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d");

    public IEnumerable<PluginPageInfo> GetPages()
    {
        return new[]
        {
            new PluginPageInfo
            {
                Name = "JellyWatchParty",
                EmbeddedResourcePath = GetType().Namespace + ".Web.configPage.html"
            }
        };
    }
}
```

### Singleton Pattern

The `Instance` static property follows Jellyfin's standard plugin pattern. It's set once during plugin initialization and provides access to the plugin configuration from controllers.

## JellyWatchPartyController.cs

### Description
ASP.NET Core controller providing REST API endpoints.

### Endpoints

#### `GET /JellyWatchParty/ClientScript`

Serves the client JS *loader* (`plugin.js`) with caching support. The
loader then fetches each individual module via
`GET /JellyWatchParty/Client/{*path}` (`GetClientModule`, same embedded-
resource/ETag caching model) — the client is not shipped as one
pre-bundled file. See [Client](client.md) for the module list, and the
[REST API Reference](#rest-api-reference) below for the full endpoint
list, including the [Host Bridge](host-bridge.md) endpoints this
controller also exposes.

```csharp
[HttpGet("ClientScript")]
[Produces("text/javascript")]
public async Task<ActionResult> GetClientScript()
{
    // ETag validation for cache
    var requestETag = Request.Headers["If-None-Match"].FirstOrDefault();
    if (!string.IsNullOrEmpty(requestETag) && requestETag == _cachedScriptETag)
    {
        return StatusCode(304); // Not Modified
    }

    // Load from embedded resource (cached after first load)
    if (_cachedScript == null)
    {
        var assembly = typeof(JellyWatchPartyController).Assembly;
        var resourceName = "JellyWatchParty.Plugin.Web.plugin.js";
        using var stream = assembly.GetManifestResourceStream(resourceName);
        if (stream == null) return NotFound();
        using var reader = new StreamReader(stream);
        _cachedScript = await reader.ReadToEndAsync();
        _cachedScriptETag = $"\"{ComputeETag(_cachedScript)}\"";
    }

    // Set cache headers
    Response.Headers["Cache-Control"] = "public, max-age=3600";
    Response.Headers["ETag"] = _cachedScriptETag;

    return Content(_cachedScript, "text/javascript");
}
```

**Features:**
- Embedded resource loading
- ETag-based cache validation
- HTTP 304 Not Modified support
- 1-hour cache lifetime

#### `GET /JellyWatchParty/Token`

Generates JWT tokens for authenticated users.

```csharp
[HttpGet("Token")]
[Authorize]
[Produces("application/json")]
public ActionResult GetToken()
{
    // Get user from authenticated context
    var userId = User.FindFirst(ClaimTypes.NameIdentifier)?.Value;
    var userName = User.FindFirst(ClaimTypes.Name)?.Value;

    // Validate claims
    if (string.IsNullOrEmpty(userId))
    {
        return Unauthorized(new { error = "User identity not found" });
    }

    // Rate limiting: 10 tokens per minute per user
    if (!CheckRateLimit(userId))
    {
        return StatusCode(429, new { error = "Rate limit exceeded" });
    }

    // Check if JWT is configured
    if (string.IsNullOrEmpty(config.JwtSecret))
    {
        return Ok(new {
            token = (string?)null,
            auth_enabled = false,
            user_id = userId,
            user_name = userName
        });
    }

    var token = GenerateJwtToken(userId, userName, config);

    return Ok(new {
        token,
        auth_enabled = true,
        expires_in = config.TokenTtlSeconds,
        user_id = userId,
        user_name = userName
    });
}
```

**Features:**
- Jellyfin authentication required
- Rate limiting (10 tokens/minute/user)
- JWT token generation
- Graceful handling when JWT not configured

### JWT Token Generation

```csharp
private static string GenerateJwtToken(string userId, string userName, PluginConfiguration config)
{
    var securityKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(config.JwtSecret));
    var credentials = new SigningCredentials(securityKey, SecurityAlgorithms.HmacSha256);

    var claims = new[]
    {
        new Claim(JwtRegisteredClaimNames.Sub, userId),
        new Claim(JwtRegisteredClaimNames.Name, userName),
        new Claim(JwtRegisteredClaimNames.Aud, config.JwtAudience),
        new Claim(JwtRegisteredClaimNames.Iss, config.JwtIssuer),
        new Claim(JwtRegisteredClaimNames.Iat, DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString()),
    };

    var token = new JwtSecurityToken(
        issuer: config.JwtIssuer,
        audience: config.JwtAudience,
        claims: claims,
        expires: DateTime.UtcNow.AddSeconds(config.TokenTtlSeconds),
        signingCredentials: credentials
    );

    return new JwtSecurityTokenHandler().WriteToken(token);
}
```

## PluginConfiguration.cs

### Description
Configuration model with validation.

```csharp
public class PluginConfiguration : BasePluginConfiguration
{
    private string _jwtSecret = string.Empty;
    private int _tokenTtlSeconds = 3600;
    private int _inviteTtlSeconds = 3600;

    /// <summary>
    /// JWT secret. If empty, authentication is disabled.
    /// Set a value (min 32 chars) to enable authentication.
    /// </summary>
    public string JwtSecret
    {
        get => _jwtSecret;
        set => _jwtSecret = value ?? string.Empty;
    }

    /// <summary>
    /// JWT audience claim. Defaults to "JellyWatchParty".
    /// </summary>
    public string JwtAudience { get; set; } = "JellyWatchParty";

    /// <summary>
    /// JWT issuer claim. Defaults to "Jellyfin".
    /// </summary>
    public string JwtIssuer { get; set; } = "Jellyfin";

    /// <summary>
    /// Token TTL in seconds. Clamped between 60 and 86400.
    /// </summary>
    public int TokenTtlSeconds
    {
        get => _tokenTtlSeconds;
        set => _tokenTtlSeconds = Math.Clamp(value, 60, 86400);
    }

    /// <summary>
    /// Invite TTL in seconds. Clamped between 60 and 86400.
    /// </summary>
    public int InviteTtlSeconds
    {
        get => _inviteTtlSeconds;
        set => _inviteTtlSeconds = Math.Clamp(value, 60, 86400);
    }

    /// <summary>
    /// WebSocket server URL. If empty, uses default (same host, port 3000).
    /// </summary>
    public string SessionServerUrl { get; set; } = string.Empty;

    /// <summary>
    /// Checks a Session Server URL for common misconfigurations (wrong
    /// scheme, malformed URL, bare internal hostname) and returns
    /// human-readable warnings. Never rejects a value - empty result means
    /// no issues found.
    /// </summary>
    public static IReadOnlyList<string> ValidateSessionServerUrl(string? value) { /* ... */ }
}
```

**Validation:**
- TTL values are clamped to valid range (1 minute to 24 hours)
- Null JWT secret is converted to empty string
- `SessionServerUrl` is checked by `ValidateSessionServerUrl` and any
  warnings are logged (`Plugin` constructor) — this is advisory only, the
  value itself is never modified or rejected

## configPage.html

### Description
Admin configuration page rendered in Jellyfin dashboard.

### Features

- **JWT Secret** - Password input field (never exposed in GET response)
- **JWT Audience** - Configurable audience claim
- **JWT Issuer** - Configurable issuer claim
- **Session Server URL** - Live warning indicator (updates on input/blur) that flags suspicious values without blocking save
- **Save button** - Persists configuration

### Security Considerations

- JWT secret is never sent back to the client
- Password field prevents shoulder surfing
- Only admins can access the plugin configuration page

## Embedded Resources

The project file configures embedded resources:

```xml
<ItemGroup>
  <EmbeddedResource Include="Web\configPage.html" />
  <EmbeddedResource Include="Web\plugin.js" />
</ItemGroup>
```

Resources are accessed via:
```csharp
assembly.GetManifestResourceStream("JellyWatchParty.Plugin.Web.plugin.js");
```

## Dependencies

```xml
<ItemGroup>
  <PackageReference Include="Jellyfin.Controller" Version="10.11.11" ExcludeAssets="runtime" />
  <PackageReference Include="Jellyfin.Model" Version="10.11.11" ExcludeAssets="runtime" />
  <PackageReference Include="Newtonsoft.Json" Version="13.0.3" ExcludeAssets="runtime" />
  <PackageReference Include="System.IdentityModel.Tokens.Jwt" Version="6.35.0" />
  <PackageReference Include="Microsoft.IdentityModel.Tokens" Version="6.35.0" />
</ItemGroup>
```

## Building

```bash
# Build with dotnet
dotnet build

# Or use just (from project root)
just build plugin
```

The built DLL and dependencies are placed in `bin/Debug/net9.0/`.

## REST API Reference

**Base URL:** `http(s)://<jellyfin-host>:<port>/JellyWatchParty`

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/JellyWatchParty/ClientScript` | None | Client JS loader (`plugin.js`), ETag-cached |
| `GET` | `/JellyWatchParty/Client/{*path}` | None | Individual client module by path, e.g. `Client/playback/sync.js` |
| `GET` | `/JellyWatchParty/Token` | Jellyfin auth | Issues a JWT (or no-auth response) for the current user |
| `GET` | `/JellyWatchParty/Bridge/Sessions` | Jellyfin auth (any user) | Sessions eligible to bridge in as a room host — see [Host Bridge](host-bridge.md) |
| `GET` | `/JellyWatchParty/Bridge/Status` | Jellyfin auth (any user) | Active bridges |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Start` | Jellyfin auth (any user) | Start bridging a session in as host |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Follow?roomId=…` | Jellyfin auth (any user) | Attach a session to a room as a receiver (follower) |
| `POST` | `/JellyWatchParty/Bridge/{sessionId}/Stop` | Jellyfin auth (any user) | Stop an active bridge |

The Bridge endpoints are gated with plain `[Authorize]` — **not** admin-only
— since session info (username, device, now-playing title) is deliberately
not treated as private within a server.

### GET /JellyWatchParty/Token

```bash
curl -H "X-Emby-Token: YOUR_API_KEY" "http://localhost:8096/JellyWatchParty/Token"
```

```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "auth_enabled": true,
  "expires_in": 3600,
  "user_id": "abc123",
  "user_name": "John"
}
```

When JWT isn't configured, `token` is `null` and `auth_enabled` is `false`.

| Status | Meaning |
|--------|---------|
| 200 | Token generated successfully |
| 401 | Not authenticated or claims missing |
| 429 | Rate limit exceeded (10 tokens/min per user) |
| 500 | Plugin not configured |

All error responses share the shape `{"error": "..."}`.

### Configuration API

Plugin configuration is managed through Jellyfin's standard plugin
configuration endpoints (admin privileges required):

```bash
# Read
curl -H "X-Emby-Token: $TOKEN" \
  "http://localhost:8096/System/Configuration/Plugin/0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d"

# Update
curl -X POST \
  -H "X-Emby-Token: $TOKEN" -H "Content-Type: application/json" \
  -d '{"JwtSecret": "...", "JwtAudience": "JellyWatchParty", "JwtIssuer": "Jellyfin", "TokenTtlSeconds": 3600, "InviteTtlSeconds": 3600, "SessionServerUrl": ""}' \
  "http://localhost:8096/System/Configuration/Plugin/0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d"
```

See [Configuration](../configuration) for the field reference and examples.

### WebSocket API

The session server itself uses WebSocket, not REST, for real-time
communication — endpoint `ws(s)://<host>:3000/ws`. See
[Protocol](protocol.md) for the complete message specification.
