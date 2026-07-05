using System.Collections.Concurrent;
using System.Security.Claims;
using System.Text;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;
using OpenWatchParty.Plugin.Configuration;
using OpenWatchParty.Plugin.Services;

namespace OpenWatchParty.Plugin.Controllers;

/// <summary>
/// Controller for OpenWatchParty plugin endpoints.
/// Provides client script serving and JWT token generation for watch party sessions.
/// </summary>
[ApiController]
[Route("OpenWatchParty")]
public class OpenWatchPartyController : ControllerBase
{
    private readonly ILogger<OpenWatchPartyController> _logger;

    // Rate limiting: max 30 tokens per minute per user (allows for reconnections)
    private const int MaxTokensPerMinute = 30;
    private static readonly ConcurrentDictionary<string, (int Count, DateTime ResetTime)> TokenRateLimits = new();
    private static DateTime _lastRateLimitCleanup = DateTime.UtcNow;
    private static readonly TimeSpan RateLimitCleanupInterval = TimeSpan.FromMinutes(5);

    // Cache for embedded script content using Lazy<T> for thread-safe initialization (fixes audit 4.5.1)
    private static readonly Lazy<(string Content, string ETag)> _scriptCache = new(LoadScriptFromResource, LazyThreadSafetyMode.ExecutionAndPublication);
    private static readonly ConcurrentDictionary<string, (string Content, string ETag)> _clientModuleCache = new(StringComparer.OrdinalIgnoreCase);

    private readonly HostBridgeManager _hostBridgeManager;

    /// <summary>
    /// Initializes a new instance of the controller with logging support.
    /// </summary>
    /// <param name="logger">The logger instance for this controller.</param>
    /// <param name="hostBridgeManager">Manages native-client host bridges.</param>
    public OpenWatchPartyController(ILogger<OpenWatchPartyController> logger, HostBridgeManager hostBridgeManager)
    {
        _logger = logger;
        _hostBridgeManager = hostBridgeManager;
    }

    /// <summary>
    /// Loads the client script from embedded resources (thread-safe, called once via Lazy).
    /// </summary>
    private static (string Content, string ETag) LoadScriptFromResource()
    {
        var assembly = typeof(OpenWatchPartyController).Assembly;
        var resourceName = "OpenWatchParty.Plugin.Web.plugin.js";
        using var stream = assembly.GetManifestResourceStream(resourceName);
        if (stream == null)
        {
            throw new InvalidOperationException($"Embedded resource '{resourceName}' not found");
        }
        using var reader = new StreamReader(stream);
        var content = reader.ReadToEnd();
        var hash = System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(content));
        var etag = $"\"{Convert.ToBase64String(hash)[..16]}\"";
        return (content, etag);
    }

    /// <summary>
    /// Returns the OpenWatchParty client JavaScript.
    /// Supports ETag caching for efficient client-side caching.
    /// </summary>
    /// <returns>The JavaScript client script.</returns>
    [HttpGet("ClientScript")]
    [Produces("text/javascript")]
    public ActionResult GetClientScript()
    {
        // Get cached script (thread-safe via Lazy<T>)
        var (content, etag) = _scriptCache.Value;

        // Check If-None-Match header for cache validation
        var requestETag = Request.Headers["If-None-Match"].FirstOrDefault();
        if (!string.IsNullOrEmpty(requestETag) && requestETag == etag)
        {
            return StatusCode(304); // Not Modified
        }

        // Set cache headers
        Response.Headers["Cache-Control"] = "public, max-age=3600";
        Response.Headers["ETag"] = etag;

        return Content(content, "text/javascript");
    }

    /// <summary>
    /// Returns an OpenWatchParty client module JavaScript file from embedded resources.
    /// This avoids relying on Jellyfin's /web/plugins static path.
    /// </summary>
    /// <param name="path">Relative module path, e.g. "state.js" or "ui/render.js".</param>
    /// <returns>The JavaScript module content.</returns>
    [HttpGet("Client/{*path}")]
    [Produces("text/javascript")]
    public ActionResult GetClientModule([FromRoute] string? path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return NotFound();
        }

        var normalizedPath = path.Replace('\\', '/').TrimStart('/');
        if (normalizedPath.Contains("..", StringComparison.Ordinal) || !normalizedPath.EndsWith(".js", StringComparison.OrdinalIgnoreCase))
        {
            return NotFound();
        }

        try
        {
            var (content, etag) = _clientModuleCache.GetOrAdd(normalizedPath, LoadClientModuleFromResource);

            var requestETag = Request.Headers["If-None-Match"].FirstOrDefault();
            if (!string.IsNullOrEmpty(requestETag) && requestETag == etag)
            {
                return StatusCode(304);
            }

            Response.Headers["Cache-Control"] = "public, max-age=3600";
            Response.Headers["ETag"] = etag;

            return Content(content, "text/javascript");
        }
        catch (FileNotFoundException)
        {
            return NotFound();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to serve embedded client module '{Path}'", normalizedPath);
            return StatusCode(500);
        }
    }

    private static (string Content, string ETag) LoadClientModuleFromResource(string normalizedPath)
    {
        var assembly = typeof(OpenWatchPartyController).Assembly;
        var resourceName = "OpenWatchParty.Plugin.Web." + normalizedPath.Replace('/', '.');

        using var stream = assembly.GetManifestResourceStream(resourceName)
            ?? throw new FileNotFoundException($"Embedded resource '{resourceName}' not found");

        using var reader = new StreamReader(stream);
        var content = reader.ReadToEnd();
        var hash = System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(content));
        var etag = $"\"{Convert.ToBase64String(hash)[..16]}\"";
        return (content, etag);
    }

    /// <summary>
    /// Returns plugin information including the plugin ID.
    /// Useful for configuration pages to dynamically get the plugin GUID.
    /// </summary>
    /// <returns>Plugin info including ID, name, and version.</returns>
    [HttpGet("Info")]
    [Produces("application/json")]
    public ActionResult GetPluginInfo()
    {
        return Ok(new
        {
            id = Plugin.PluginGuid,
            name = Plugin.Instance?.Name ?? "OpenWatchParty",
            version = Plugin.PluginVersion
        });
    }

    /// <summary>
    /// Cleans up expired entries from the rate limit dictionary (P-CS01 fix).
    /// Called periodically to prevent memory leak from accumulating stale entries.
    /// </summary>
    private static void CleanupExpiredRateLimits()
    {
        var now = DateTime.UtcNow;
        if (now - _lastRateLimitCleanup < RateLimitCleanupInterval)
        {
            return;
        }

        _lastRateLimitCleanup = now;

        // Remove all expired entries
        var expiredKeys = TokenRateLimits
            .Where(kvp => now > kvp.Value.ResetTime)
            .Select(kvp => kvp.Key)
            .ToList();

        foreach (var key in expiredKeys)
        {
            TokenRateLimits.TryRemove(key, out _);
        }
    }

    /// <summary>
    /// Generates a JWT token for the authenticated user to connect to the session server.
    /// Rate limited to 10 tokens per minute per user.
    /// </summary>
    /// <returns>Token response containing the JWT or indication that auth is disabled.</returns>
    /// <response code="200">Returns the token or auth disabled response.</response>
    /// <response code="401">User identity not found in claims.</response>
    /// <response code="429">Rate limit exceeded.</response>
    /// <response code="500">Plugin not configured.</response>
    [HttpGet("Token")]
    [Authorize]
    [Produces("application/json")]
    public ActionResult GetToken()
    {
        // P-CS01 fix: Periodically clean up expired rate limit entries to prevent memory leak
        CleanupExpiredRateLimits();

        var config = Plugin.Instance?.Configuration;
        if (config == null)
        {
            return StatusCode(500, new { error = "Plugin not configured" });
        }

        // Get user info from the authenticated context
        var userId = User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                  ?? User.FindFirst("Jellyfin-UserId")?.Value;
        var userName = User.FindFirst(ClaimTypes.Name)?.Value
                    ?? User.Identity?.Name;

        // Validate user claims are present
        if (string.IsNullOrEmpty(userId))
        {
            return Unauthorized(new { error = "User identity not found in claims" });
        }
        if (string.IsNullOrEmpty(userName))
        {
            userName = "User";  // Fallback for display name only
        }

        // Rate limiting check
        var now = DateTime.UtcNow;
        var limit = TokenRateLimits.GetOrAdd(userId, _ => (0, now.AddMinutes(1)));
        if (now >= limit.ResetTime)
        {
            limit = (1, now.AddMinutes(1));
            TokenRateLimits[userId] = limit;
        }
        else if (limit.Count >= MaxTokensPerMinute)
        {
            _logger.LogWarning("Token rate limit exceeded for user {UserId}", userId);
            return StatusCode(429, new { error = "Rate limit exceeded. Try again later." });
        }
        else
        {
            TokenRateLimits[userId] = (limit.Count + 1, limit.ResetTime);
        }

        // Check if JWT is configured
        if (string.IsNullOrEmpty(config.JwtSecret))
        {
            // Return a special response indicating auth is disabled
            return Ok(new {
                token = (string?)null,
                auth_enabled = false,
                user_id = userId,
                user_name = userName,
                session_server_url = config.SessionServerUrl ?? string.Empty
            });
        }

        var token = GenerateJwtToken(userId, userName, config);
        _logger.LogDebug("Generated token for user {UserName} ({UserId})", userName, userId);

        return Ok(new {
            token,
            auth_enabled = true,
            expires_in = config.TokenTtlSeconds,
            user_id = userId,
            user_name = userName,
            session_server_url = config.SessionServerUrl ?? string.Empty
        });
    }

    private static string GenerateJwtToken(string userId, string userName, PluginConfiguration config)
    {
        return SessionServerAuth.CreateToken(userId, userName, config);
    }

    /// <summary>
    /// Lists Jellyfin sessions eligible to be bridged in as an OpenWatchParty
    /// room host (i.e. sessions currently playing something), for the
    /// in-player OpenWatchParty widget's host picker. Any logged-in user can
    /// see this list and start/stop a bridge — session info (username,
    /// device, now-playing title) is not treated as private within a server.
    /// </summary>
    [HttpGet("Bridge/Sessions")]
    [Authorize]
    [Produces("application/json")]
    public ActionResult GetBridgeableSessions()
    {
        // Jellyfin's controllers do not auto-camelCase JSON output (see the
        // existing /Token endpoint, which spells out user_id/auth_enabled
        // literally) — project onto anonymous objects with the exact keys
        // the injected JS (ui/bridge.js) expects, rather than relying on a
        // naming policy that isn't actually applied.
        var sessions = _hostBridgeManager.GetEligibleSessions().Select(s => new
        {
            sessionId = s.SessionId,
            userName = s.UserName,
            deviceName = s.DeviceName,
            client = s.Client,
            nowPlayingItemName = s.NowPlayingItemName
        });
        return Ok(sessions);
    }

    /// <summary>
    /// Lists currently active host bridges, for the in-player widget's
    /// status display.
    /// </summary>
    [HttpGet("Bridge/Status")]
    [Authorize]
    [Produces("application/json")]
    public ActionResult GetBridgeStatus()
    {
        var bridges = _hostBridgeManager.GetActiveBridges().Select(ToBridgeStatusJson);
        return Ok(bridges);
    }

    /// <summary>
    /// Starts bridging the given Jellyfin session's playback into a new
    /// OpenWatchParty room, with that session as the room's host.
    /// </summary>
    /// <param name="sessionId">The Jellyfin session identifier to bridge.</param>
    [HttpPost("Bridge/{sessionId}/Start")]
    [Authorize]
    [Produces("application/json")]
    public async Task<ActionResult> StartBridge([FromRoute] string sessionId)
    {
        try
        {
            var status = await _hostBridgeManager.StartBridgeAsync(sessionId);
            return Ok(ToBridgeStatusJson(status));
        }
        catch (InvalidOperationException ex)
        {
            return BadRequest(new { error = ex.Message });
        }
    }

    private static object ToBridgeStatusJson(BridgeStatus status) => new
    {
        sessionId = status.SessionId,
        userName = status.UserName,
        roomId = status.RoomId,
        connected = status.Connected
    };

    /// <summary>
    /// Stops an active host bridge for the given Jellyfin session, closing
    /// the OpenWatchParty room it was hosting.
    /// </summary>
    /// <param name="sessionId">The Jellyfin session identifier to stop bridging.</param>
    [HttpPost("Bridge/{sessionId}/Stop")]
    [Authorize]
    [Produces("application/json")]
    public async Task<ActionResult> StopBridge([FromRoute] string sessionId)
    {
        await _hostBridgeManager.StopBridgeAsync(sessionId);
        return Ok();
    }
}
