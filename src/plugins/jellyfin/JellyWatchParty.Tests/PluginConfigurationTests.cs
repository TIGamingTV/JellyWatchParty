using JellyWatchParty.Plugin.Configuration;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

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
    public void JwtAudience_DefaultIsJellyWatchParty()
    {
        var config = new PluginConfiguration();
        Assert.Equal("JellyWatchParty", config.JwtAudience);
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

    [Fact]
    public void HideNativeSyncButton_DefaultIsFalse()
    {
        var config = new PluginConfiguration();
        Assert.False(config.HideNativeSyncButton);
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void HideNativeSyncButton_RoundTripsValue(bool value)
    {
        var config = new PluginConfiguration { HideNativeSyncButton = value };
        Assert.Equal(value, config.HideNativeSyncButton);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("wss://party.example.com/ws")]
    [InlineData("ws://localhost:3000/ws")]
    [InlineData("wss://192.168.1.5/ws")]
    public void ValidateSessionServerUrl_NoWarningsForValidValues(string? value)
    {
        var warnings = PluginConfiguration.ValidateSessionServerUrl(value);
        Assert.Empty(warnings);
    }

    [Theory]
    [InlineData("not a url")]
    [InlineData("http://party.example.com/ws")]
    [InlineData("ws://jwp-session:3000/ws")]
    public void ValidateSessionServerUrl_WarnsForSuspiciousValues(string value)
    {
        var warnings = PluginConfiguration.ValidateSessionServerUrl(value);
        Assert.NotEmpty(warnings);
    }

    [Fact]
    public void ValidateSessionServerUrl_WarnsAboutWrongScheme()
    {
        var warnings = PluginConfiguration.ValidateSessionServerUrl("http://party.example.com/ws");
        Assert.Contains(warnings, w => w.Contains("ws:// or wss://", StringComparison.Ordinal));
    }

    [Fact]
    public void ValidateSessionServerUrl_WarnsAboutInternalHostname()
    {
        var warnings = PluginConfiguration.ValidateSessionServerUrl("ws://jwp-session:3000/ws");
        Assert.Contains(warnings, w => w.Contains("internal/Docker hostname", StringComparison.Ordinal));
    }

    [Fact]
    public void ValidateSessionServerUrl_WarnsAboutMalformedUrl()
    {
        var warnings = PluginConfiguration.ValidateSessionServerUrl("not a url");
        Assert.Contains(warnings, w => w.Contains("not a valid absolute URL", StringComparison.Ordinal));
    }

    [Theory]
    [InlineData("")]
    [InlineData("short")]
    [InlineData("0123456789")]                            // 10 chars - the exact length from the bug report
    [InlineData("0123456789012345678901234567890")]        // 31 chars
    public void HasUsableJwtSecret_FalseForEmptyOrTooShort(string secret)
    {
        var config = new PluginConfiguration { JwtSecret = secret };
        Assert.False(config.HasUsableJwtSecret);
    }

    [Theory]
    [InlineData("01234567890123456789012345678901")]       // 32 chars - minimum
    [InlineData("0123456789012345678901234567890123456789012345678901234567890123")] // 64 chars
    public void HasUsableJwtSecret_TrueForLongEnoughSecrets(string secret)
    {
        var config = new PluginConfiguration { JwtSecret = secret };
        Assert.True(config.HasUsableJwtSecret);
    }
}
