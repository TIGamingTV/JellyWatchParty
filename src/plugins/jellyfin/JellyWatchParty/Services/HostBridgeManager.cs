using System.Collections.Concurrent;
using MediaBrowser.Controller.Library;
using MediaBrowser.Controller.Session;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using JellyWatchParty.Plugin.Configuration;

namespace JellyWatchParty.Plugin.Services;

/// <summary>
/// Owns all active <see cref="SessionHostBridge"/> instances — one per
/// Jellyfin session an admin has chosen to bridge in as an JellyWatchParty
/// room host. Subscribes to <see cref="ISessionManager"/>'s playback events
/// for the lifetime of the server process and routes them to whichever
/// bridge (if any) matches the reporting session.
/// </summary>
public sealed class HostBridgeManager : IHostedService
{
    // Clients that render Jellyfin Web's index.html and already run the
    // injected JWP script (see docs/ARCHITECTURE.md's native-client
    // constraint) — these can already host a room themselves via the
    // normal "Create Room" button, so they're excluded from the bridge
    // picker to avoid clutter/confusion. Everything else (Fladder,
    // Swiftfin, Infuse, official mobile/TV apps, ...) is a genuine
    // native-client candidate.
    //
    // SessionInfo.Client includes a trailing version (e.g. "Jellyfin Web
    // 10.11.11", "Jellyfin Desktop 3.0.0-dev" — the latter being the
    // renamed Jellyfin Media Player), so this matches on prefix rather
    // than exact equality.
    private static readonly string[] InjectedClientPrefixes =
    {
        "Jellyfin Web",
        "Jellyfin Desktop",
        "Jellyfin Media Player"
    };

    private static bool IsInjectedClient(string? client) =>
        !string.IsNullOrEmpty(client)
        && InjectedClientPrefixes.Any(prefix => client.StartsWith(prefix, StringComparison.OrdinalIgnoreCase));

    private readonly ISessionManager _sessionManager;
    private readonly ILogger<HostBridgeManager> _logger;
    private readonly ConcurrentDictionary<string, SessionHostBridge> _bridges = new();
    private readonly ConcurrentDictionary<string, SessionFollowerBridge> _followers = new();

    public HostBridgeManager(ISessionManager sessionManager, ILogger<HostBridgeManager> logger)
    {
        _sessionManager = sessionManager;
        _logger = logger;
    }

    public Task StartAsync(CancellationToken cancellationToken)
    {
        _sessionManager.PlaybackStart += OnPlaybackProgress;
        _sessionManager.PlaybackProgress += OnPlaybackProgress;
        _sessionManager.PlaybackStopped += OnPlaybackStopped;
        return Task.CompletedTask;
    }

    public async Task StopAsync(CancellationToken cancellationToken)
    {
        _sessionManager.PlaybackStart -= OnPlaybackProgress;
        _sessionManager.PlaybackProgress -= OnPlaybackProgress;
        _sessionManager.PlaybackStopped -= OnPlaybackStopped;

        foreach (var sessionId in _bridges.Keys.ToList())
        {
            if (_bridges.TryRemove(sessionId, out var bridge))
            {
                await bridge.StopAsync().ConfigureAwait(false);
            }
        }

        foreach (var sessionId in _followers.Keys.ToList())
        {
            if (_followers.TryRemove(sessionId, out var follower))
            {
                await follower.StopAsync().ConfigureAwait(false);
            }
        }
    }

    /// <summary>
    /// Active Jellyfin sessions that could be started as a bridge host
    /// (i.e. currently playing something, not already bridged, and not
    /// already running the injected JWP client itself).
    /// </summary>
    public IReadOnlyList<BridgeableSessionInfo> GetEligibleSessions()
    {
        return _sessionManager.Sessions
            .Where(s => s.NowPlayingItem != null
                && !_bridges.ContainsKey(s.Id)
                && !_followers.ContainsKey(s.Id)
                && !IsInjectedClient(s.Client))
            .Select(s => new BridgeableSessionInfo(s.Id, s.UserName, s.DeviceName, s.Client, s.NowPlayingItem?.Name))
            .ToList();
    }

    /// <summary>
    /// All currently active bridges — both hosts (a session driving a room)
    /// and receivers (a session following a room).
    /// </summary>
    public IReadOnlyList<BridgeStatus> GetActiveBridges()
    {
        var hosts = _bridges
            .Select(kvp => new BridgeStatus(kvp.Key, kvp.Value.UserName, kvp.Value.RoomId, kvp.Value.Connected, "host"));
        var followers = _followers
            .Select(kvp => new BridgeStatus(kvp.Key, kvp.Value.UserName, kvp.Value.RoomId, kvp.Value.Connected, "receiver"));
        return hosts.Concat(followers).ToList();
    }

    /// <summary>
    /// Starts bridging the given session's playback into a new
    /// JellyWatchParty room, with that session as host.
    /// </summary>
    public async Task<BridgeStatus> StartBridgeAsync(string sessionId)
    {
        var (session, config) = PrepareBridge(sessionId);

        var bridge = new SessionHostBridge(session, config, _logger);
        if (!_bridges.TryAdd(sessionId, bridge))
        {
            throw new InvalidOperationException($"Session '{sessionId}' is already bridged.");
        }

        try
        {
            await bridge.StartAsync(session, CancellationToken.None).ConfigureAwait(false);
        }
        catch
        {
            _bridges.TryRemove(sessionId, out _);
            await bridge.DisposeAsync().ConfigureAwait(false);
            throw;
        }

        _logger.LogInformation(
            "[JellyWatchParty] Started host bridge for session {SessionId} ({UserName})",
            sessionId,
            bridge.UserName);

        return new BridgeStatus(sessionId, bridge.UserName, bridge.RoomId, bridge.Connected, "host");
    }

    /// <summary>
    /// Starts following the given JellyWatchParty room from the given Jellyfin
    /// session (a "receiver"): the session's playback is driven to match the
    /// room's host via remote-control playstate commands. The session must
    /// already be playing the room's item.
    /// </summary>
    public async Task<BridgeStatus> StartFollowerAsync(string sessionId, string roomId)
    {
        if (string.IsNullOrWhiteSpace(roomId))
        {
            throw new InvalidOperationException("A room id is required to attach a receiver.");
        }

        var (session, config) = PrepareBridge(sessionId);

        var follower = new SessionFollowerBridge(session, roomId, config, _sessionManager, _logger);
        if (!_followers.TryAdd(sessionId, follower))
        {
            throw new InvalidOperationException($"Session '{sessionId}' is already bridged.");
        }

        try
        {
            await follower.StartAsync(CancellationToken.None).ConfigureAwait(false);
        }
        catch
        {
            _followers.TryRemove(sessionId, out _);
            await follower.DisposeAsync().ConfigureAwait(false);
            throw;
        }

        _logger.LogInformation(
            "[JellyWatchParty] Started receiver bridge for session {SessionId} ({UserName}) following room {RoomId}",
            sessionId,
            follower.UserName,
            roomId);

        return new BridgeStatus(sessionId, follower.UserName, follower.RoomId, follower.Connected, "receiver");
    }

    /// <summary>
    /// Validates that a session can be bridged (exists, not already bridged in
    /// either role, and the plugin has a session-server URL) and returns the
    /// session and configuration to build a bridge from.
    /// </summary>
    private (SessionInfo Session, PluginConfiguration Config) PrepareBridge(string sessionId)
    {
        if (_bridges.ContainsKey(sessionId) || _followers.ContainsKey(sessionId))
        {
            throw new InvalidOperationException($"Session '{sessionId}' is already bridged.");
        }

        var session = _sessionManager.Sessions.FirstOrDefault(s => s.Id == sessionId)
            ?? throw new InvalidOperationException($"No active session with id '{sessionId}'.");

        var config = Plugin.Instance?.Configuration
            ?? throw new InvalidOperationException("JellyWatchParty plugin is not configured.");
        if (string.IsNullOrWhiteSpace(config.SessionServerUrl))
        {
            throw new InvalidOperationException("Session Server URL is not configured.");
        }

        return (session, config);
    }

    /// <summary>
    /// Stops an active bridge for the given session — closing the room it was
    /// hosting, or detaching it as a receiver.
    /// </summary>
    public async Task StopBridgeAsync(string sessionId)
    {
        if (_bridges.TryRemove(sessionId, out var bridge))
        {
            await bridge.DisposeAsync().ConfigureAwait(false);
            _logger.LogInformation("[JellyWatchParty] Stopped host bridge for session {SessionId}", sessionId);
        }

        if (_followers.TryRemove(sessionId, out var follower))
        {
            await follower.DisposeAsync().ConfigureAwait(false);
            _logger.LogInformation("[JellyWatchParty] Stopped receiver bridge for session {SessionId}", sessionId);
        }
    }

    private void OnPlaybackProgress(object? sender, PlaybackProgressEventArgs e)
    {
        if (!_bridges.TryGetValue(e.Session.Id, out var bridge))
        {
            return;
        }

        _ = RunAndLogAsync(
            bridge.OnPlaybackProgressAsync(e.IsPaused, e.PlaybackPositionTicks, CancellationToken.None),
            e.Session.Id);
    }

    private void OnPlaybackStopped(object? sender, PlaybackStopEventArgs e)
    {
        if (_bridges.TryRemove(e.Session.Id, out var bridge))
        {
            _ = RunAndLogAsync(bridge.DisposeAsync().AsTask(), e.Session.Id);
        }

        // A receiver has nothing left to control once its session stops playing.
        if (_followers.TryRemove(e.Session.Id, out var follower))
        {
            _ = RunAndLogAsync(follower.DisposeAsync().AsTask(), e.Session.Id);
        }
    }

    private async Task RunAndLogAsync(Task task, string sessionId)
    {
        try
        {
            await task.ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Host bridge error for session {SessionId}", sessionId);
        }
    }
}
