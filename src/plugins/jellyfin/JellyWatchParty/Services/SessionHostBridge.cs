using System.Net.WebSockets;
using System.Text;
using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Session;
using Microsoft.Extensions.Logging;
using Newtonsoft.Json.Linq;
using JellyWatchParty.Plugin.Configuration;

namespace JellyWatchParty.Plugin.Services;

/// <summary>
/// Bridges one Jellyfin session's playback into one JellyWatchParty room,
/// with that session acting as the room's host. Opens its own WebSocket
/// connection to the Rust session server and speaks the same client
/// protocol a browser host would (see docs/ARCHITECTURE.md) — the resulting
/// room is indistinguishable from a browser-hosted one to guests.
/// </summary>
public sealed class SessionHostBridge : IAsyncDisposable
{
    private const int ReceiveBufferSize = 8 * 1024;

    // The session server drops connections with no inbound traffic after its
    // ZOMBIE_TIMEOUT_MS (60s) reaper window. A bridged session only emits
    // WebSocket frames when its playback state changes, so without a steady
    // heartbeat a paused or otherwise idle bridge is silently reaped and
    // removed from its room mid-session. Ping well inside that window,
    // mirroring the web client's keepalive (see implem.md §1.9).
    private static readonly TimeSpan HeartbeatInterval = TimeSpan.FromSeconds(20);

    // How long Start waits for the session server to confirm (room_state) or
    // refuse (error) the room. Servers that never answer don't block it.
    internal static readonly TimeSpan StartAnswerTimeout = TimeSpan.FromSeconds(5);

    private readonly string _sessionId;
    private readonly string _userId;
    private readonly string _userName;
    private readonly PluginConfiguration _config;
    private readonly ILogger _logger;
    private readonly ClientWebSocket _socket = new();
    private readonly CancellationTokenSource _cts = new();

    // ClientWebSocket forbids concurrent SendAsync calls; the heartbeat loop
    // can otherwise overlap an event-driven send, so all sends serialize here.
    private readonly SemaphoreSlim _sendLock = new(1, 1);

    // Completes with null once the room exists, or with the server's error.
    private readonly TaskCompletionSource<string?> _started = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly Action? _onEnded;

    private Task? _receiveLoop;
    private Task? _heartbeatLoop;
    private bool? _lastIsPaused;
    private int _disposed;

    /// <summary>
    /// Creates a bridge for <paramref name="session"/>. <paramref name="onEnded"/>
    /// is called if the room closes or the bridge is removed from it, so the
    /// owner can drop the bridge.
    /// </summary>
    public SessionHostBridge(SessionInfo session, PluginConfiguration config, ILogger logger, Action? onEnded = null)
    {
        _sessionId = session.Id;
        _userId = session.UserId.ToString("N");
        _userName = $"{session.UserName} ({session.DeviceName})";
        _config = config;
        _logger = logger;
        _onEnded = onEnded;
    }

    public string? RoomId { get; private set; }

    public string UserName => _userName;

    /// <summary>The Jellyfin user (id, "N" format) whose session this is.</summary>
    public string OwnerUserId => _userId;

    public bool Connected => _socket.State == WebSocketState.Open;

    public async Task StartAsync(SessionInfo session, CancellationToken cancellationToken)
    {
        await _socket.ConnectAsync(new Uri(_config.SessionServerUrl), cancellationToken).ConfigureAwait(false);

        await SendAsync(BuildAuthPayload(_userId, _userName, _config), "auth", room: null, cancellationToken)
            .ConfigureAwait(false);
        await SendAsync(BuildCreateRoomPayload(session), "create_room", room: null, cancellationToken)
            .ConfigureAwait(false);

        _lastIsPaused = session.PlayState?.IsPaused;
        _receiveLoop = Task.Run(() => ReceiveLoopAsync(_cts.Token), CancellationToken.None);
        _heartbeatLoop = Task.Run(() => HeartbeatLoopAsync(_cts.Token), CancellationToken.None);

        // Surface a refusal (e.g. the device is already in a watch party) to
        // the caller instead of reporting a bridge that has no room.
        var finished = await Task.WhenAny(_started.Task, Task.Delay(StartAnswerTimeout, cancellationToken)).ConfigureAwait(false);
        if (finished == _started.Task && await _started.Task.ConfigureAwait(false) is { } error)
        {
            throw new InvalidOperationException(error);
        }
    }

    public async Task OnPlaybackProgressAsync(bool isPaused, long? positionTicks, CancellationToken cancellationToken)
    {
        if (RoomId == null || !Connected)
        {
            return;
        }

        var positionSeconds = TicksToSeconds(positionTicks);

        if (_lastIsPaused != isPaused)
        {
            _lastIsPaused = isPaused;
            await SendAsync(BuildPlayerEventPayload(isPaused, positionSeconds), "player_event", RoomId, cancellationToken)
                .ConfigureAwait(false);
        }
        else
        {
            await SendAsync(BuildStateUpdatePayload(isPaused, positionSeconds), "state_update", RoomId, cancellationToken)
                .ConfigureAwait(false);
        }
    }

    public async Task StopAsync()
    {
        _cts.Cancel();
        // Leave the room explicitly: a plain close would keep this bridge in
        // the room (as host) for the server's 90 s reconnect grace.
        if (_socket.State == WebSocketState.Open && RoomId is { } room)
        {
            try
            {
                await SendAsync(new JObject(), "leave_room", room, CancellationToken.None).ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is WebSocketException or ObjectDisposedException or InvalidOperationException)
            {
                // Going away anyway.
            }
        }

        if (_socket.State == WebSocketState.Open)
        {
            // Take the send lock so the close frame can't race an in-flight
            // heartbeat ping (concurrent sends throw on ClientWebSocket).
            await _sendLock.WaitAsync().ConfigureAwait(false);
            try
            {
                await _socket.CloseAsync(WebSocketCloseStatus.NormalClosure, "Bridge stopped", CancellationToken.None)
                    .ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is WebSocketException or ObjectDisposedException)
            {
                // Socket already closing/closed — nothing to do.
            }
            finally
            {
                _sendLock.Release();
            }
        }

        foreach (var loop in new[] { _receiveLoop, _heartbeatLoop })
        {
            if (loop != null)
            {
                try
                {
                    await loop.ConfigureAwait(false);
                }
                catch (Exception ex) when (ex is OperationCanceledException)
                {
                    // Expected on stop.
                }
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        // Safe to call more than once (a config change and a failed start can
        // both dispose the same bridge).
        if (Interlocked.Exchange(ref _disposed, 1) == 1)
        {
            return;
        }

        await StopAsync().ConfigureAwait(false);
        _socket.Dispose();
        _cts.Dispose();
        _sendLock.Dispose();
    }

    private async Task ReceiveLoopAsync(CancellationToken cancellationToken)
    {
        var buffer = new byte[ReceiveBufferSize];
        var messageBuilder = new StringBuilder();

        try
        {
            while (_socket.State == WebSocketState.Open && !cancellationToken.IsCancellationRequested)
            {
                var result = await _socket.ReceiveAsync(new ArraySegment<byte>(buffer), cancellationToken)
                    .ConfigureAwait(false);
                if (result.MessageType == WebSocketMessageType.Close)
                {
                    break;
                }

                messageBuilder.Append(Encoding.UTF8.GetString(buffer, 0, result.Count));
                if (!result.EndOfMessage)
                {
                    continue;
                }

                HandleServerMessage(messageBuilder.ToString());
                messageBuilder.Clear();
            }
        }
        catch (OperationCanceledException)
        {
            // Expected on stop.
        }
        catch (WebSocketException ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Host bridge for session {SessionId} lost its connection", _sessionId);
        }
    }

    private async Task HeartbeatLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            while (!cancellationToken.IsCancellationRequested)
            {
                await Task.Delay(HeartbeatInterval, cancellationToken).ConfigureAwait(false);
                if (_socket.State != WebSocketState.Open)
                {
                    continue;
                }

                try
                {
                    await SendAsync(BuildPingPayload(), "ping", room: null, cancellationToken).ConfigureAwait(false);
                }
                catch (Exception ex) when (ex is WebSocketException or ObjectDisposedException or InvalidOperationException)
                {
                    // Connection is going away; the receive loop handles teardown.
                    break;
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected on stop.
        }
    }

    /// <summary>Outcome of the start: null once the room exists, else the server's error.</summary>
    internal Task<string?> StartOutcome => _started.Task;

    internal void HandleServerMessage(string json)
    {
        JObject message;
        try
        {
            message = JObject.Parse(json);
        }
        catch (Newtonsoft.Json.JsonException)
        {
            return;
        }

        switch (message["type"]?.ToString())
        {
            case "room_state":
                RoomId = message["room"]?.ToString();
                _started.TrySetResult(null);
                break;
            case "error":
                var error = message["payload"]?["message"]?.ToString() ?? "The session server refused the request";
                _logger.LogWarning(
                    "[JellyWatchParty] Session server rejected a message for bridged session {SessionId}: {Message}",
                    _sessionId,
                    error);
                if (RoomId == null)
                {
                    _started.TrySetResult(error);
                }

                break;
            case "room_closed":
                // Closed, or an admin removed this bridge: nothing left to do.
                RoomId = null;
                _onEnded?.Invoke();
                break;
        }
    }

    private async Task SendAsync(JObject payload, string type, string? room, CancellationToken cancellationToken)
    {
        var envelope = new JObject
        {
            ["type"] = type,
            ["payload"] = payload,
            ["ts"] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
        };
        if (room != null)
        {
            envelope["room"] = room;
        }

        var bytes = Encoding.UTF8.GetBytes(envelope.ToString(Newtonsoft.Json.Formatting.None));

        await _sendLock.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            await _socket.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, cancellationToken)
                .ConfigureAwait(false);
        }
        finally
        {
            _sendLock.Release();
        }
    }

    private static double TicksToSeconds(long? ticks) => (ticks ?? 0) / (double)TimeSpan.TicksPerSecond;

    // Keepalive. The server echoes ping->pong, but the point is the inbound
    // frame itself: it refreshes the connection's last-seen timestamp so the
    // server's zombie reaper leaves an otherwise-idle bridge in its room.
    internal static JObject BuildPingPayload() =>
        new() { ["client_ts"] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() };

    internal static JObject BuildAuthPayload(string userId, string userName, PluginConfiguration config)
    {
        if (config.HasUsableJwtSecret)
        {
            return new JObject { ["token"] = SessionServerAuth.CreateToken(userId, userName, config) };
        }

        return new JObject { ["user_id"] = userId, ["user_name"] = userName };
    }

    internal static JObject BuildCreateRoomPayload(SessionInfo session)
    {
        var payload = new JObject
        {
            ["user_name"] = $"{session.UserName} ({session.DeviceName})",
            ["start_pos"] = TicksToSeconds(session.PlayState?.PositionTicks),
        };

        var itemId = session.NowPlayingItem?.Id;
        if (itemId.HasValue && itemId.Value != Guid.Empty)
        {
            payload["media_id"] = itemId.Value.ToString("N");
        }

        AddBridgeDeviceId(payload, session.DeviceId);
        return payload;
    }

    /// <summary>
    /// Tells the session server which Jellyfin device this connection stands
    /// in for, so its admin panel can show it as a plugin bridge and won't
    /// bridge the same device a second time.
    /// </summary>
    internal static void AddBridgeDeviceId(JObject payload, string? deviceId)
    {
        if (!string.IsNullOrEmpty(deviceId))
        {
            payload["bridge_device_id"] = deviceId;
        }
    }

    internal static JObject BuildPlayerEventPayload(bool isPaused, double positionSeconds) =>
        new()
        {
            ["action"] = isPaused ? "pause" : "play",
            ["position"] = positionSeconds,
        };

    internal static JObject BuildStateUpdatePayload(bool isPaused, double positionSeconds) =>
        new()
        {
            ["position"] = positionSeconds,
            ["play_state"] = isPaused ? "paused" : "playing",
        };
}
