using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Dto;
using Microsoft.Extensions.Logging;
using Moq;
using OpenWatchParty.Plugin.Services;
using Xunit;

namespace OpenWatchParty.Plugin.Tests;

/// <summary>
/// Tests for HostBridgeManager's session-eligibility filtering — the pure
/// query logic behind the admin config page's host picker.
/// </summary>
public class HostBridgeManagerTests
{
    private static SessionInfo CreateSession(string id, bool isPlaying)
    {
        var session = new SessionInfo(Mock.Of<ISessionManager>(), Mock.Of<ILogger>())
        {
            Id = id,
            UserId = Guid.NewGuid(),
            UserName = "User-" + id,
            DeviceName = "Device-" + id,
            Client = "Fladder",
        };

        if (isPlaying)
        {
            session.NowPlayingItem = new BaseItemDto { Id = Guid.NewGuid(), Name = "Some Movie" };
        }

        return session;
    }

    private static HostBridgeManager CreateManager(IReadOnlyList<SessionInfo> sessions)
    {
        var sessionManager = new Mock<ISessionManager>();
        sessionManager.Setup(m => m.Sessions).Returns(sessions);
        return new HostBridgeManager(sessionManager.Object, Mock.Of<ILogger<HostBridgeManager>>());
    }

    [Fact]
    public void GetEligibleSessions_ExcludesSessionsWithNoNowPlayingItem()
    {
        var playing = CreateSession("s1", isPlaying: true);
        var idle = CreateSession("s2", isPlaying: false);
        var manager = CreateManager(new[] { playing, idle });

        var eligible = manager.GetEligibleSessions();

        Assert.Single(eligible);
        Assert.Equal("s1", eligible[0].SessionId);
    }

    [Fact]
    public void GetEligibleSessions_ReturnsEmpty_WhenNoSessionsArePlaying()
    {
        var idle = CreateSession("s1", isPlaying: false);
        var manager = CreateManager(new[] { idle });

        var eligible = manager.GetEligibleSessions();

        Assert.Empty(eligible);
    }

    [Fact]
    public void GetEligibleSessions_IncludesNowPlayingItemName()
    {
        var playing = CreateSession("s1", isPlaying: true);
        var manager = CreateManager(new[] { playing });

        var eligible = manager.GetEligibleSessions();

        Assert.Equal("Some Movie", eligible[0].NowPlayingItemName);
    }

    [Fact]
    public void GetActiveBridges_EmptyByDefault()
    {
        var manager = CreateManager(Array.Empty<SessionInfo>());

        Assert.Empty(manager.GetActiveBridges());
    }

    [Fact]
    public async Task StartBridgeAsync_Throws_WhenSessionDoesNotExist()
    {
        var manager = CreateManager(Array.Empty<SessionInfo>());

        await Assert.ThrowsAsync<InvalidOperationException>(() => manager.StartBridgeAsync("missing-session"));
    }
}
