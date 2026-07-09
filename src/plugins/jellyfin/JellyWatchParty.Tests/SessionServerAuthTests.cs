using JellyWatchParty.Plugin.Configuration;
using JellyWatchParty.Plugin.Services;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Tests for SessionServerAuth's token signing, particularly the guard
/// against unusably short JWT secrets (regression coverage for the
/// IDX10653 crash on GET /JellyWatchParty/Token).
/// </summary>
public class SessionServerAuthTests
{
    private static PluginConfiguration CreateConfig(string jwtSecret) => new()
    {
        JwtSecret = jwtSecret,
        JwtAudience = "JellyWatchParty",
        JwtIssuer = "Jellyfin",
        TokenTtlSeconds = 3600,
    };

    [Fact]
    public void CreateToken_TooShortSecret_ThrowsClearInvalidOperationException()
    {
        // Regression test: previously this reached the JWT library and threw
        // a cryptic ArgumentOutOfRangeException (IDX10653) from three layers
        // down instead of failing fast with an actionable message.
        var config = CreateConfig("short10ch");

        var ex = Assert.Throws<InvalidOperationException>(
            () => SessionServerAuth.CreateToken("user-1", "Alice", config));

        Assert.Contains("JwtSecret", ex.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void CreateToken_UsableSecret_ReturnsNonEmptyToken()
    {
        var config = CreateConfig("0123456789012345678901234567890123456789");

        var token = SessionServerAuth.CreateToken("user-1", "Alice", config);

        Assert.False(string.IsNullOrEmpty(token));
    }
}
