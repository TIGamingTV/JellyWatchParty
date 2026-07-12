using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Dto;
using MediaBrowser.Model.Session;
using Microsoft.Extensions.Logging;
using Moq;
using JellyWatchParty.Plugin.Configuration;
using JellyWatchParty.Plugin.Services;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Tests for SessionHostBridge's pure payload-building helpers — the parts
/// that can be verified without an actual WebSocket connection.
/// </summary>
public class SessionHostBridgeTests
{
    private static SessionInfo CreateSession(
        Guid? nowPlayingItemId = null,
        long? positionTicks = null,
        string userName = "Alice",
        string deviceName = "Shield TV")
    {
        var session = new SessionInfo(Mock.Of<ISessionManager>(), Mock.Of<ILogger>())
        {
            Id = "session-1",
            UserId = Guid.NewGuid(),
            UserName = userName,
            DeviceName = deviceName,
            Client = "Fladder",
        };

        if (nowPlayingItemId.HasValue)
        {
            session.NowPlayingItem = new BaseItemDto { Id = nowPlayingItemId.Value };
        }

        session.PlayState = new PlayerStateInfo { PositionTicks = positionTicks };

        return session;
    }

    [Fact]
    public void BuildCreateRoomPayload_FormatsMediaIdAsThirtyTwoHexChars()
    {
        var itemId = Guid.Parse("550e8400-e29b-41d4-a716-446655440000");
        var session = CreateSession(nowPlayingItemId: itemId);

        var payload = SessionHostBridge.BuildCreateRoomPayload(session);

        Assert.Equal("550e8400e29b41d4a716446655440000", payload["media_id"]!.ToString());
        Assert.Equal(32, payload["media_id"]!.ToString().Length);
    }

    [Fact]
    public void BuildCreateRoomPayload_OmitsMediaId_WhenNowPlayingItemIsNull()
    {
        var session = CreateSession(nowPlayingItemId: null);

        var payload = SessionHostBridge.BuildCreateRoomPayload(session);

        Assert.Null(payload["media_id"]);
    }

    [Fact]
    public void BuildCreateRoomPayload_ConvertsPositionTicksToSeconds()
    {
        var session = CreateSession(positionTicks: 42 * TimeSpan.TicksPerSecond);

        var payload = SessionHostBridge.BuildCreateRoomPayload(session);

        Assert.Equal(42.0, (double)payload["start_pos"]!);
    }

    [Fact]
    public void BuildCreateRoomPayload_ZeroPosition_WhenPositionTicksIsNull()
    {
        var session = CreateSession(positionTicks: null);

        var payload = SessionHostBridge.BuildCreateRoomPayload(session);

        Assert.Equal(0.0, (double)payload["start_pos"]!);
    }

    [Fact]
    public void BuildCreateRoomPayload_IncludesUserNameAndDeviceName()
    {
        var session = CreateSession(userName: "Bob", deviceName: "Living Room TV");

        var payload = SessionHostBridge.BuildCreateRoomPayload(session);

        Assert.Equal("Bob (Living Room TV)", payload["user_name"]!.ToString());
    }

    [Fact]
    public void BuildPlayerEventPayload_Paused_UsesPauseAction()
    {
        var payload = SessionHostBridge.BuildPlayerEventPayload(isPaused: true, positionSeconds: 10.5);

        Assert.Equal("pause", payload["action"]!.ToString());
        Assert.Equal(10.5, (double)payload["position"]!);
    }

    [Fact]
    public void BuildPlayerEventPayload_Playing_UsesPlayAction()
    {
        var payload = SessionHostBridge.BuildPlayerEventPayload(isPaused: false, positionSeconds: 3.0);

        Assert.Equal("play", payload["action"]!.ToString());
    }

    [Fact]
    public void BuildStateUpdatePayload_Paused_UsesPausedPlayState()
    {
        var payload = SessionHostBridge.BuildStateUpdatePayload(isPaused: true, positionSeconds: 1.0);

        Assert.Equal("paused", payload["play_state"]!.ToString());
    }

    [Fact]
    public void BuildStateUpdatePayload_Playing_UsesPlayingPlayState()
    {
        var payload = SessionHostBridge.BuildStateUpdatePayload(isPaused: false, positionSeconds: 1.0);

        Assert.Equal("playing", payload["play_state"]!.ToString());
    }

    [Fact]
    public void BuildPingPayload_CarriesAClientTimestamp()
    {
        // The heartbeat exists so an idle bridge keeps generating inbound
        // frames and the session server's zombie reaper doesn't evict it from
        // its room. The body just needs a client_ts, mirroring the web client.
        var before = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        var payload = SessionHostBridge.BuildPingPayload();
        var after = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        var clientTs = payload["client_ts"];
        Assert.NotNull(clientTs);
        var value = (long)clientTs!;
        Assert.InRange(value, before, after);
    }

    [Fact]
    public void BuildAuthPayload_EmptySecret_FallsBackToUserIdAndName()
    {
        var config = new PluginConfiguration { JwtSecret = string.Empty };

        var payload = SessionHostBridge.BuildAuthPayload("user-1", "Alice", config);

        Assert.Null(payload["token"]);
        Assert.Equal("user-1", payload["user_id"]!.ToString());
        Assert.Equal("Alice", payload["user_name"]!.ToString());
    }

    [Fact]
    public void BuildAuthPayload_TooShortSecret_FallsBackWithoutThrowing()
    {
        // Regression test: a non-empty but sub-128-bit secret used to reach
        // SessionServerAuth.CreateToken and throw ArgumentOutOfRangeException
        // (IDX10653) from the JWT library. It must now be treated the same
        // as an empty secret instead of crashing.
        var config = new PluginConfiguration { JwtSecret = "short10ch" };

        var payload = SessionHostBridge.BuildAuthPayload("user-1", "Alice", config);

        Assert.Null(payload["token"]);
        Assert.Equal("user-1", payload["user_id"]!.ToString());
    }

    [Fact]
    public void BuildAuthPayload_UsableSecret_IncludesToken()
    {
        var config = new PluginConfiguration { JwtSecret = "0123456789012345678901234567890123456789" };

        var payload = SessionHostBridge.BuildAuthPayload("user-1", "Alice", config);

        Assert.NotNull(payload["token"]);
        Assert.False(string.IsNullOrEmpty(payload["token"]!.ToString()));
    }
}
