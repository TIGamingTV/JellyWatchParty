using System.Collections.Concurrent;
using MediaBrowser.Controller.Library;
using MediaBrowser.Controller.Session;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace OpenWatchParty.Plugin.Services;

/// <summary>
/// Owns all active <see cref="SessionHostBridge"/> instances — one per
/// Jellyfin session an admin has chosen to bridge in as an OpenWatchParty
/// room host. Subscribes to <see cref="ISessionManager"/>'s playback events
/// for the lifetime of the server process and routes them to whichever
/// bridge (if any) matches the reporting session.
/// </summary>
public sealed class HostBridgeManager : IHostedService
{
    private readonly ISessionManager _sessionManager;
    private readonly ILogger<HostBridgeManager> _logger;
    private readonly ConcurrentDictionary<string, SessionHostBridge> _bridges = new();

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
    }

    /// <summary>
    /// Active Jellyfin sessions that could be started as a bridge host
    /// (i.e. currently playing something and not already bridged).
    /// </summary>
    public IReadOnlyList<BridgeableSessionInfo> GetEligibleSessions()
    {
        return _sessionManager.Sessions
            .Where(s => s.NowPlayingItem != null && !_bridges.ContainsKey(s.Id))
            .Select(s => new BridgeableSessionInfo(s.Id, s.UserName, s.DeviceName, s.Client, s.NowPlayingItem?.Name))
            .ToList();
    }

    /// <summary>
    /// All currently active host bridges.
    /// </summary>
    public IReadOnlyList<BridgeStatus> GetActiveBridges()
    {
        return _bridges
            .Select(kvp => new BridgeStatus(kvp.Key, kvp.Value.UserName, kvp.Value.RoomId, kvp.Value.Connected))
            .ToList();
    }

    /// <summary>
    /// Starts bridging the given session's playback into a new
    /// OpenWatchParty room, with that session as host.
    /// </summary>
    public async Task<BridgeStatus> StartBridgeAsync(string sessionId)
    {
        if (_bridges.ContainsKey(sessionId))
        {
            throw new InvalidOperationException($"Session '{sessionId}' is already bridged.");
        }

        var session = _sessionManager.Sessions.FirstOrDefault(s => s.Id == sessionId)
            ?? throw new InvalidOperationException($"No active session with id '{sessionId}'.");

        var config = Plugin.Instance?.Configuration
            ?? throw new InvalidOperationException("OpenWatchParty plugin is not configured.");
        if (string.IsNullOrWhiteSpace(config.SessionServerUrl))
        {
            throw new InvalidOperationException("Session Server URL is not configured.");
        }

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
            "[OpenWatchParty] Started host bridge for session {SessionId} ({UserName})",
            sessionId,
            bridge.UserName);

        return new BridgeStatus(sessionId, bridge.UserName, bridge.RoomId, bridge.Connected);
    }

    /// <summary>
    /// Stops an active host bridge, closing the room it was hosting.
    /// </summary>
    public async Task StopBridgeAsync(string sessionId)
    {
        if (_bridges.TryRemove(sessionId, out var bridge))
        {
            await bridge.DisposeAsync().ConfigureAwait(false);
            _logger.LogInformation("[OpenWatchParty] Stopped host bridge for session {SessionId}", sessionId);
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
    }

    private async Task RunAndLogAsync(Task task, string sessionId)
    {
        try
        {
            await task.ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "[OpenWatchParty] Host bridge error for session {SessionId}", sessionId);
        }
    }
}
