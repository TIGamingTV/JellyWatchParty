using OpenWatchParty.Plugin.Configuration;
using Xunit;

namespace OpenWatchParty.Plugin.Tests;

/// <summary>
/// Tests for PluginConfiguration validation and clamping behavior.
/// </summary>
public class PluginConfigurationTests
{
    [Fact]
    public void JwtSecret_DefaultIsEmpty()
    {
        var config = new PluginConfiguration();
        Assert.Equal(string.Empty, config.JwtSecret);
    }

    [Fact]
    public void JwtSecret_NullBecomesEmpty()
    {
        var config = new PluginConfiguration();
        config.JwtSecret = null!;
        Assert.Equal(string.Empty, config.JwtSecret);
    }

    [Fact]
    public void JwtAudience_DefaultIsOpenWatchParty()
    {
        var config = new PluginConfiguration();
        Assert.Equal("OpenWatchParty", config.JwtAudience);
    }

    [Fact]
    public void JwtIssuer_DefaultIsJellyfin()
    {
        var config = new PluginConfiguration();
        Assert.Equal("Jellyfin", config.JwtIssuer);
    }

    [Theory]
    [InlineData(60, 60)]      // Minimum valid
    [InlineData(3600, 3600)]  // Default
    [InlineData(86400, 86400)] // Maximum valid
    public void TokenTtlSeconds_ValidValuesAccepted(int input, int expected)
    {
        var config = new PluginConfiguration();
        config.TokenTtlSeconds = input;
        Assert.Equal(expected, config.TokenTtlSeconds);
    }

    [Theory]
    [InlineData(0, 60)]       // Below minimum -> clamped to 60
    [InlineData(59, 60)]      // Just below minimum -> clamped to 60
    [InlineData(-1000, 60)]   // Negative -> clamped to 60
    [InlineData(86401, 86400)] // Just above maximum -> clamped to 86400
    [InlineData(100000, 86400)] // Way above maximum -> clamped to 86400
    public void TokenTtlSeconds_ClampedToValidRange(int input, int expected)
    {
        var config = new PluginConfiguration();
        config.TokenTtlSeconds = input;
        Assert.Equal(expected, config.TokenTtlSeconds);
    }

    [Theory]
    [InlineData(60, 60)]
    [InlineData(3600, 3600)]
    [InlineData(86400, 86400)]
    public void InviteTtlSeconds_ValidValuesAccepted(int input, int expected)
    {
        var config = new PluginConfiguration();
        config.InviteTtlSeconds = input;
        Assert.Equal(expected, config.InviteTtlSeconds);
    }

    [Theory]
    [InlineData(0, 60)]
    [InlineData(-100, 60)]
    [InlineData(100000, 86400)]
    public void InviteTtlSeconds_ClampedToValidRange(int input, int expected)
    {
        var config = new PluginConfiguration();
        config.InviteTtlSeconds = input;
        Assert.Equal(expected, config.InviteTtlSeconds);
    }

    [Fact]
    public void SessionServerUrl_DefaultIsEmpty()
    {
        var config = new PluginConfiguration();
        Assert.Equal(string.Empty, config.SessionServerUrl);
    }

}
