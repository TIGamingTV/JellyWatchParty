using MediaBrowser.Model.Plugins;

namespace JellyWatchParty.Plugin.Configuration;

/// <summary>
/// Configuration for the JellyWatchParty plugin.
/// Provides settings for JWT authentication and session server connection.
/// </summary>
public class PluginConfiguration : BasePluginConfiguration
{
    /// <summary>
    /// Minimum JWT secret length (in characters) for it to be considered
    /// usable for HS256 signing. HS256's hard technical floor is 128 bits
    /// (16 UTF-8 bytes) - below that, signing throws. This constant is set
    /// well above that floor, matching the security recommendation already
    /// documented for admins, so a secret that passes this check can never
    /// hit the crypto library's minimum-key-size exception.
    /// </summary>
    public const int MinJwtSecretLength = 32;

    private string _jwtSecret = string.Empty;
    private int _tokenTtlSeconds = 3600;
    private int _inviteTtlSeconds = 3600;

    /// <summary>
    /// Gets or sets the JWT secret. If empty, authentication is disabled.
    /// Set a value (min 32 chars) to enable authentication.
    /// </summary>
    /// <remarks>
    /// For security, the secret should be at least 32 characters with high entropy.
    /// Use a cryptographically random string for production deployments.
    /// </remarks>
    public string JwtSecret
    {
        get => _jwtSecret;
        set => _jwtSecret = value ?? string.Empty;
    }

    /// <summary>
    /// True when <see cref="JwtSecret"/> is both set and long enough to be
    /// safely used for HS256 signing. False covers two distinct cases that
    /// callers should treat identically: auth intentionally disabled (empty
    /// secret) and a misconfigured, too-short secret - in both cases, skip
    /// token generation and respond as if auth were disabled rather than
    /// attempting to sign with an unusable key.
    /// </summary>
    public bool HasUsableJwtSecret => JwtSecret.Length >= MinJwtSecretLength;

    /// <summary>
    /// JWT audience claim. Defaults to "JellyWatchParty".
    /// </summary>
    public string JwtAudience { get; set; } = "JellyWatchParty";

    /// <summary>
    /// JWT issuer claim. Defaults to "Jellyfin".
    /// </summary>
    public string JwtIssuer { get; set; } = "Jellyfin";

    /// <summary>
    /// Token TTL in seconds. Must be between 60 and 86400 (1 min to 24 hours).
    /// </summary>
    public int TokenTtlSeconds
    {
        get => _tokenTtlSeconds;
        set => _tokenTtlSeconds = Math.Clamp(value, 60, 86400);
    }

    /// <summary>
    /// Invite TTL in seconds. Must be between 60 and 86400 (1 min to 24 hours).
    /// </summary>
    public int InviteTtlSeconds
    {
        get => _inviteTtlSeconds;
        set => _inviteTtlSeconds = Math.Clamp(value, 60, 86400);
    }

    /// <summary>
    /// The WebSocket server URL. If empty, uses the default (same host, port 3000).
    /// </summary>
    /// <example>ws://localhost:3000/ws or wss://party.example.com/ws</example>
    public string SessionServerUrl { get; set; } = string.Empty;

    /// <summary>
    /// When true, the web client hides Jellyfin's built-in SyncPlay button
    /// (the "groups" icon in the header / player OSD). JellyWatchParty provides
    /// its own watch-party controls that replace that feature, so admins can
    /// remove the redundant native button to avoid confusion. Defaults to false
    /// so upgrading an existing install never silently removes a native Jellyfin
    /// control - it is opt-in from the plugin configuration page.
    /// </summary>
    public bool HideNativeSyncButton { get; set; }

    /// <summary>
    /// When true, third-party / native Jellyfin clients that cannot run the
    /// injected JellyWatchParty script (Fladder, Swiftfin, Infuse, official
    /// mobile apps, ...) may be bridged in as a room <em>host</em>: the server
    /// mirrors that session's playback into a brand-new room for others to
    /// join. This is a server-side workaround, so it is opt-in - defaults to
    /// false so it stays off until an admin deliberately enables it from the
    /// plugin configuration page.
    /// </summary>
    public bool AllowThirdPartyClientHost { get; set; }

    /// <summary>
    /// When true, supported native clients (such as the official Jellyfin
    /// Android TV app) may be attached to an existing room as a
    /// <em>receiver</em>: the server drives that session to follow the room's
    /// host via remote-control playstate commands. Opt-in - defaults to false
    /// so it stays off until an admin deliberately enables it from the plugin
    /// configuration page.
    /// </summary>
    public bool AllowSupportedClientReceiver { get; set; }

    /// <summary>
    /// Checks a Session Server URL for common misconfigurations and returns
    /// human-readable warnings. Does not reject anything - an empty result
    /// means no issues were found. Empty/whitespace input always passes
    /// (it means "auto-detect") since this only flags likely mistakes.
    /// </summary>
    public static IReadOnlyList<string> ValidateSessionServerUrl(string? value)
    {
        var warnings = new List<string>();
        if (string.IsNullOrWhiteSpace(value))
        {
            return warnings;
        }

        if (!Uri.TryCreate(value.Trim(), UriKind.Absolute, out var uri))
        {
            warnings.Add("Session Server URL is not a valid absolute URL.");
            return warnings;
        }

        if (!uri.Scheme.Equals("ws", StringComparison.OrdinalIgnoreCase)
            && !uri.Scheme.Equals("wss", StringComparison.OrdinalIgnoreCase))
        {
            warnings.Add($"Session Server URL should use ws:// or wss:// (got '{uri.Scheme}://').");
        }

        if (string.IsNullOrEmpty(uri.Host))
        {
            warnings.Add("Session Server URL is missing a host.");
        }
        else if (!uri.Host.Contains('.', StringComparison.Ordinal)
            && !uri.Host.Equals("localhost", StringComparison.OrdinalIgnoreCase)
            && !System.Net.IPAddress.TryParse(uri.Host, out _))
        {
            warnings.Add($"'{uri.Host}' looks like an internal/Docker hostname - it may not be reachable from a browser outside the container network.");
        }

        return warnings;
    }
}
