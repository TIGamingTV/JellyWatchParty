using MediaBrowser.Model.Session;
using Newtonsoft.Json.Linq;
using JellyWatchParty.Plugin.Services;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Tests for SessionFollowerBridge's pure translation helpers — the parts that
/// map inbound room messages to remote-control playstate commands, verifiable
/// without a WebSocket or a live session.
/// </summary>
public class SessionFollowerBridgeTests
{
    [Fact]
    public void BuildJoinRoomPayload_IncludesUserName()
    {
        var payload = SessionFollowerBridge.BuildJoinRoomPayload("Alice (Shield TV)");

        Assert.Equal("Alice (Shield TV)", payload["user_name"]!.ToString());
    }

    [Fact]
    public void ParseRoomEvent_PlayerEventPlay_IsPlayingWithPosition()
    {
        var message = JObject.Parse("{\"type\":\"player_event\",\"payload\":{\"action\":\"play\",\"position\":42.5}}");

        var state = SessionFollowerBridge.ParseRoomEvent(message);

        Assert.NotNull(state);
        Assert.False(state!.Value.IsPaused);
        Assert.Equal(42.5, state.Value.PositionSeconds);
    }

    [Fact]
    public void ParseRoomEvent_PlayerEventPause_IsPaused()
    {
        var message = JObject.Parse("{\"type\":\"player_event\",\"payload\":{\"action\":\"pause\",\"position\":10}}");

        var state = SessionFollowerBridge.ParseRoomEvent(message);

        Assert.True(state!.Value.IsPaused);
        Assert.Equal(10.0, state.Value.PositionSeconds);
    }

    [Fact]
    public void ParseRoomEvent_StateUpdate_MapsPlayStateAndPosition()
    {
        var message = JObject.Parse("{\"type\":\"state_update\",\"payload\":{\"play_state\":\"paused\",\"position\":7.25}}");

        var state = SessionFollowerBridge.ParseRoomEvent(message);

        Assert.True(state!.Value.IsPaused);
        Assert.Equal(7.25, state.Value.PositionSeconds);
    }

    [Fact]
    public void ParseRoomEvent_RoomState_ReadsNestedState()
    {
        var message = JObject.Parse("{\"type\":\"room_state\",\"payload\":{\"state\":{\"play_state\":\"playing\",\"position\":100}}}");

        var state = SessionFollowerBridge.ParseRoomEvent(message);

        Assert.False(state!.Value.IsPaused);
        Assert.Equal(100.0, state.Value.PositionSeconds);
    }

    [Fact]
    public void ParseRoomEvent_UnknownType_ReturnsNull()
    {
        var message = JObject.Parse("{\"type\":\"participants_update\",\"payload\":{\"participant_count\":3}}");

        Assert.Null(SessionFollowerBridge.ParseRoomEvent(message));
    }

    [Fact]
    public void ParseRoomEvent_MissingPayload_ReturnsNull()
    {
        var message = JObject.Parse("{\"type\":\"player_event\"}");

        Assert.Null(SessionFollowerBridge.ParseRoomEvent(message));
    }

    [Fact]
    public void ParseRoomEvent_PositionAbsent_LeavesPositionNull()
    {
        var message = JObject.Parse("{\"type\":\"player_event\",\"payload\":{\"action\":\"play\"}}");

        var state = SessionFollowerBridge.ParseRoomEvent(message);

        Assert.False(state!.Value.IsPaused);
        Assert.Null(state.Value.PositionSeconds);
    }

    [Theory]
    [InlineData(true, PlaystateCommand.Pause)]
    [InlineData(false, PlaystateCommand.Unpause)]
    public void ResolvePlayPauseCommand_MapsPausedFlag(bool isPaused, PlaystateCommand expected)
    {
        Assert.Equal(expected, SessionFollowerBridge.ResolvePlayPauseCommand(isPaused));
    }

    [Fact]
    public void SecondsToTicks_ConvertsUsingTicksPerSecond()
    {
        Assert.Equal(42 * TimeSpan.TicksPerSecond, SessionFollowerBridge.SecondsToTicks(42));
    }

    [Fact]
    public void ShouldSeek_WithinThreshold_DoesNotSeek()
    {
        // 1s drift < 2s threshold, even well past the cooldown.
        Assert.False(SessionFollowerBridge.ShouldSeek(
            targetSeconds: 100, currentSeconds: 99, thresholdSeconds: 2,
            sinceLastSeek: TimeSpan.FromSeconds(10), cooldown: TimeSpan.FromSeconds(3)));
    }

    [Fact]
    public void ShouldSeek_BeyondThresholdButInCooldown_DoesNotSeek()
    {
        // 10s drift, but only 1s since the last seek (< 3s cooldown).
        Assert.False(SessionFollowerBridge.ShouldSeek(
            targetSeconds: 100, currentSeconds: 90, thresholdSeconds: 2,
            sinceLastSeek: TimeSpan.FromSeconds(1), cooldown: TimeSpan.FromSeconds(3)));
    }

    [Fact]
    public void ShouldSeek_BeyondThresholdAndPastCooldown_Seeks()
    {
        Assert.True(SessionFollowerBridge.ShouldSeek(
            targetSeconds: 100, currentSeconds: 90, thresholdSeconds: 2,
            sinceLastSeek: TimeSpan.FromSeconds(5), cooldown: TimeSpan.FromSeconds(3)));
    }
}
