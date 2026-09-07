using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Tests for the Plugin class constants and static members.
/// </summary>
public class PluginTests
{
    [Fact]
    public void PluginGuid_IsValidGuid()
    {
        Assert.True(Guid.TryParse(Plugin.PluginGuid, out _), "PluginGuid should be a valid GUID");
    }

    [Fact]
    public void PluginGuid_HasExpectedValue()
    {
        // This test ensures the GUID doesn't accidentally change
        Assert.Equal("0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d", Plugin.PluginGuid);
    }

    [Fact]
    public void PluginGuid_MatchesExpectedFormat()
    {
        // GUID should be lowercase and in standard format
        var guid = new Guid(Plugin.PluginGuid);
        Assert.Equal(Plugin.PluginGuid, guid.ToString());
    }

    [Fact]
    public void PluginVersion_IsNotEmpty()
    {
        Assert.False(string.IsNullOrEmpty(Plugin.PluginVersion), "PluginVersion should not be empty");
    }

    [Fact]
    public void PluginVersion_HasValidFormat()
    {
        // Version should be in X.Y.Z format
        var parts = Plugin.PluginVersion.Split('.');
        Assert.True(parts.Length >= 2, "Version should have at least major.minor parts");
        Assert.All(parts, part => Assert.True(int.TryParse(part, out _), $"Version part '{part}' should be numeric"));
    }

    // -- Server version diagnostics --

    [Fact]
    public void TargetedServerVersion_IsResolvedFromTheMediaBrowserCommonReference()
    {
        Assert.NotNull(Plugin.TargetedServerVersion);
    }

    [Fact]
    public void RunningServerVersion_IsResolvedFromTheLoadedAssembly()
    {
        Assert.NotNull(Plugin.RunningServerVersion);
    }

    [Fact]
    public void IsServerNewerMajor_FlagsAServerAWholeMajorAhead()
    {
        // The case this exists for: a 10.11-targeted build installed on
        // Jellyfin 12, which Jellyfin allows because targetAbi is a floor.
        Assert.True(Plugin.IsServerNewerMajor(new Version("12.0.0.0"), new Version("10.11.11.0")));
    }

    [Theory]
    [InlineData("10.11.11.0", "10.11.11.0")]
    [InlineData("10.11.12.0", "10.11.11.0")]
    [InlineData("10.10.7.0", "10.11.11.0")]
    public void IsServerNewerMajor_IsQuietWithinTheSameMajor(string running, string targeted)
    {
        Assert.False(Plugin.IsServerNewerMajor(new Version(running), new Version(targeted)));
    }

    [Fact]
    public void IsServerNewerMajor_IsQuietWhenEitherVersionIsUnknown()
    {
        Assert.False(Plugin.IsServerNewerMajor(null, new Version("10.11.11.0")));
        Assert.False(Plugin.IsServerNewerMajor(new Version("12.0.0.0"), null));
    }
}
