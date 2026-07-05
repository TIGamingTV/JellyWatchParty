using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using Microsoft.IdentityModel.Tokens;
using OpenWatchParty.Plugin.Configuration;

namespace OpenWatchParty.Plugin.Services;

/// <summary>
/// Mints JWTs for authenticating to the Rust session server. Shared by the
/// user-facing <c>/OpenWatchParty/Token</c> endpoint and the host bridge
/// (which mints tokens on behalf of a session's owner rather than the
/// current HTTP caller).
/// </summary>
public static class SessionServerAuth
{
    private static SigningCredentials? _cachedSigningCredentials;
    private static string? _cachedJwtSecret;
    private static readonly JwtSecurityTokenHandler TokenHandler = new();

    /// <summary>
    /// Gets or creates cached signing credentials. Credentials are cached
    /// and reused until the JWT secret changes (P-CS02 fix, originally in
    /// OpenWatchPartyController).
    /// </summary>
    private static SigningCredentials GetSigningCredentials(string jwtSecret)
    {
        if (_cachedSigningCredentials != null && _cachedJwtSecret == jwtSecret)
        {
            return _cachedSigningCredentials;
        }

        var securityKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwtSecret));
        _cachedSigningCredentials = new SigningCredentials(securityKey, SecurityAlgorithms.HmacSha256);
        _cachedJwtSecret = jwtSecret;
        return _cachedSigningCredentials;
    }

    /// <summary>
    /// Creates a signed JWT for the given user, suitable for authenticating
    /// to the Rust session server's <c>auth</c> WebSocket message.
    /// </summary>
    public static string CreateToken(string userId, string userName, PluginConfiguration config)
    {
        var credentials = GetSigningCredentials(config.JwtSecret);

        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, userId),
            new Claim(JwtRegisteredClaimNames.Name, userName),
            new Claim(JwtRegisteredClaimNames.Iat, DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString(), ClaimValueTypes.Integer64),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
        };

        // Note: aud and iss are set via constructor parameters only — adding them
        // to the claims array too would produce JSON arrays instead of strings,
        // which breaks deserialization on the Rust session server.
        var token = new JwtSecurityToken(
            issuer: config.JwtIssuer,
            audience: config.JwtAudience,
            claims: claims,
            expires: DateTime.UtcNow.AddSeconds(config.TokenTtlSeconds),
            signingCredentials: credentials
        );

        return TokenHandler.WriteToken(token);
    }
}
