using System.Xml.Serialization;
using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Dto;
using Microsoft.Extensions.Logging;
using Moq;
using JellyWatchParty.Plugin.Configuration;
using JellyWatchParty.Plugin.Controllers;
using JellyWatchParty.Plugin.Services;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Panel bridging is opt-in behind a master switch, and users may only
/// bridge their own sessions.
/// </summary>
public class PanelBridgingTests
{
    [Fact]
    public void MasterSwitch_DefaultsOff_AndGatesBothRoles()
    {
        var config = new PluginConfiguration
        {
            AllowThirdPartyClientHost = true,
            AllowSupportedClientReceiver = true,
        };

        Assert.False(config.EnablePanelBridging);
        Assert.False(config.PanelHostAllowed);
        Assert.False(config.PanelReceiverAllowed);

        config.EnablePanelBridging = true;
        Assert.True(config.PanelHostAllowed);
        Assert.True(config.PanelReceiverAllowed);

        config.AllowSupportedClientReceiver = false;
        Assert.True(config.PanelHostAllowed);
        Assert.False(config.PanelReceiverAllowed);
    }

    [Fact]
    public void UpgradedConfig_WithOldFlagsOn_KeepsPanelBridgingOff()
    {
        // A configuration file written before the master switch existed.
        const string xml = """
            <?xml version="1.0"?>
            <PluginConfiguration xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
              <AllowThirdPartyClientHost>true</AllowThirdPartyClientHost>
              <AllowSupportedClientReceiver>true</AllowSupportedClientReceiver>
            </PluginConfiguration>
            """;
        var serializer = new XmlSerializer(typeof(PluginConfiguration));
        using var reader = new StringReader(xml);
        var config = (PluginConfiguration)serializer.Deserialize(reader)!;

        Assert.True(config.AllowThirdPartyClientHost);
        Assert.False(config.EnablePanelBridging);
        Assert.False(config.PanelHostAllowed);
        Assert.False(config.PanelReceiverAllowed);
    }

    [Fact]
    public void MasterSwitch_RoundTripsThroughXml()
    {
        var serializer = new XmlSerializer(typeof(PluginConfiguration));
        using var writer = new StringWriter();
        serializer.Serialize(writer, new PluginConfiguration { EnablePanelBridging = true });
        using var reader = new StringReader(writer.ToString());
        var config = (PluginConfiguration)serializer.Deserialize(reader)!;

        Assert.True(config.EnablePanelBridging);
        Assert.DoesNotContain("PanelHostAllowed", writer.ToString(), StringComparison.Ordinal);
    }

    [Fact]
    public void MayControl_OwnSessionsOnly_ExceptAdmins()
    {
        var alice = Guid.NewGuid();
        var bob = Guid.NewGuid();

        Assert.True(JellyWatchPartyController.MayControl(alice, false, alice));
        Assert.False(JellyWatchPartyController.MayControl(alice, false, bob));
        Assert.False(JellyWatchPartyController.MayControl(null, false, bob));
        Assert.False(JellyWatchPartyController.MayControl(alice, false, null));
        Assert.True(JellyWatchPartyController.MayControl(alice, true, bob));
        Assert.True(JellyWatchPartyController.MayControl(null, true, null));
    }

    private static SessionInfo Session(string id, Guid userId)
    {
        return new SessionInfo(Mock.Of<ISessionManager>(), Mock.Of<ILogger>())
        {
            Id = id,
            UserId = userId,
            UserName = "User-" + id,
            DeviceName = "Device-" + id,
            DeviceId = "device-" + id,
            Client = "Android TV",
            NowPlayingItem = new BaseItemDto { Id = Guid.NewGuid(), Name = "Movie" },
        };
    }

    private static HostBridgeManager Manager(params SessionInfo[] sessions)
    {
        var sessionManager = new Mock<ISessionManager>();
        sessionManager.Setup(m => m.Sessions).Returns(sessions);
        return new HostBridgeManager(sessionManager.Object, Mock.Of<ILogger<HostBridgeManager>>());
    }

    [Fact]
    public void GetEligibleSessions_FiltersByOwner()
    {
        var alice = Guid.NewGuid();
        var bob = Guid.NewGuid();
        var manager = Manager(Session("a1", alice), Session("b1", bob), Session("a2", alice));

        var mine = manager.GetEligibleSessions(alice);
        Assert.Equal(new[] { "a1", "a2" }, mine.Select(s => s.SessionId).ToArray());
        Assert.Equal(3, manager.GetEligibleSessions().Count);
    }

    [Fact]
    public void Owners_AreLookedUpFromSessions()
    {
        var alice = Guid.NewGuid();
        var manager = Manager(Session("a1", alice));

        Assert.Equal(alice, manager.GetSessionUserId("a1"));
        Assert.Null(manager.GetSessionUserId("gone"));
        Assert.Null(manager.GetBridgeOwner("a1"));
        Assert.Empty(manager.GetActiveBridges(alice.ToString("N")));
    }

    [Fact]
    public async Task ApplyConfiguration_WithNothingRunning_IsANoOp()
    {
        var manager = Manager();
        await manager.ApplyConfigurationAsync(new PluginConfiguration());
        Assert.Empty(manager.GetActiveBridges());
    }

    [Fact]
    public void Payloads_CarryTheBridgedDeviceId()
    {
        var session = Session("a1", Guid.NewGuid());

        var create = SessionHostBridge.BuildCreateRoomPayload(session);
        Assert.Equal("device-a1", create["bridge_device_id"]!.ToString());

        var join = SessionFollowerBridge.BuildJoinRoomPayload("Alice (TV)", "device-a1");
        Assert.Equal("device-a1", join["bridge_device_id"]!.ToString());
        Assert.Null(SessionFollowerBridge.BuildJoinRoomPayload("Alice (TV)")["bridge_device_id"]);

        session.DeviceId = null!;
        Assert.Null(SessionHostBridge.BuildCreateRoomPayload(session)["bridge_device_id"]);
    }
}

/// <summary>
/// Claims as Jellyfin's CustomAuthenticationHandler builds them, and the
/// bridges' handling of the session server's answers.
/// </summary>
public class PanelBridgingRuntimeTests
{
    private static System.Security.Claims.ClaimsPrincipal JellyfinPrincipal(Guid userId, string role)
    {
        var claims = new[]
        {
            new System.Security.Claims.Claim(System.Security.Claims.ClaimTypes.Name, "alice"),
            new System.Security.Claims.Claim(System.Security.Claims.ClaimTypes.Role, role),
            new System.Security.Claims.Claim("Jellyfin-UserId", userId.ToString("N")),
            new System.Security.Claims.Claim("Jellyfin-IsApiKey", "False"),
        };
        return new System.Security.Claims.ClaimsPrincipal(new System.Security.Claims.ClaimsIdentity(claims, "CustomAuthentication"));
    }

    [Fact]
    public void Claims_AreReadTheWayJellyfinWritesThem()
    {
        var id = Guid.NewGuid();
        var user = JellyfinPrincipal(id, "User");
        Assert.Equal(id, JellyWatchPartyController.UserIdOf(user));
        Assert.False(JellyWatchPartyController.IsAdmin(user));
        Assert.True(JellyWatchPartyController.IsAdmin(JellyfinPrincipal(id, "Administrator")));
        // An API key has no user.
        Assert.Null(JellyWatchPartyController.UserIdOf(JellyfinPrincipal(Guid.Empty, "Administrator")));
    }

    private static SessionInfo Session() => new(Mock.Of<ISessionManager>(), Mock.Of<ILogger>())
    {
        Id = "s1",
        UserId = Guid.NewGuid(),
        UserName = "Alice",
        DeviceName = "TV",
        DeviceId = "tv-1",
        Client = "Fladder",
    };

    [Fact]
    public async Task HostBridge_ReportsARefusalAndEndsWithItsRoom()
    {
        var ended = 0;
        var bridge = new SessionHostBridge(Session(), new PluginConfiguration(), Mock.Of<ILogger>(), () => ended++);

        bridge.HandleServerMessage("""{"type":"error","payload":{"message":"This device is already in a watch party","reason":"device_already_bridged"}}""");
        Assert.Equal("This device is already in a watch party", await bridge.StartOutcome);

        var ok = new SessionHostBridge(Session(), new PluginConfiguration(), Mock.Of<ILogger>(), () => ended++);
        ok.HandleServerMessage("""{"type":"room_state","room":"r1","payload":{}}""");
        Assert.Null(await ok.StartOutcome);
        Assert.Equal("r1", ok.RoomId);
        // Later errors don't count as a failed start.
        ok.HandleServerMessage("""{"type":"error","payload":{"message":"Rate limit exceeded"}}""");
        Assert.Null(await ok.StartOutcome);

        ok.HandleServerMessage("""{"type":"room_closed","room":"r1","payload":{"reason":"An admin removed you from the room","removed":true}}""");
        Assert.Null(ok.RoomId);
        Assert.Equal(1, ended);

        // Disposing twice is safe.
        await ok.DisposeAsync();
        await ok.DisposeAsync();
        await bridge.DisposeAsync();
    }

    [Fact]
    public async Task FollowerBridge_ReportsARefusedJoinAndEndsWithItsRoom()
    {
        var ended = 0;
        var follower = new SessionFollowerBridge(
            Session(), "r1", new PluginConfiguration(), Mock.Of<ISessionManager>(), Mock.Of<ILogger>(), () => ended++);

        await follower.HandleServerMessageAsync("""{"type":"error","payload":{"message":"Incorrect password","reason":"wrong_password"}}""", CancellationToken.None);
        Assert.Equal("Incorrect password", await follower.StartOutcome);

        await follower.HandleServerMessageAsync("""{"type":"room_closed","payload":{"reason":"Closed by an admin"}}""", CancellationToken.None);
        Assert.Equal(1, ended);

        await follower.DisposeAsync();
        await follower.DisposeAsync();
    }
}
