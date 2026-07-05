using System.Net.WebSockets;
using System.Text;
using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Session;
using Microsoft.Extensions.Logging;
using Newtonsoft.Json.Linq;
using OpenWatchParty.Plugin.Configuration;

namespace OpenWatchParty.Plugin.Services;

/// <summary>
/// Bridges one Jellyfin session's playback into one OpenWatchParty room,
/// with that session acting as the room's host. Opens its own WebSocket
/// connection to the Rust session server and speaks the same client
/// protocol a browser host would (see docs/ARCHITECTURE.md) — the resulting
/// room is indistinguishable from a browser-hosted one to guests.
/// </summary>
public sealed class SessionHostBridge : IAsyncDisposable
{
    private const int ReceiveBufferSize = 8 * 1024;

    private readonly string _sessionId;
    private readonly string _userId;
    private readonly string _userName;
    private readonly PluginConfiguration _config;
    private readonly ILogger _logger;
    private readonly ClientWebSocket _socket = new();
    private readonly CancellationTokenSource _cts = new();

    private Task? _receiveLoop;
    private bool? _lastIsPaused;

    public SessionHostBridge(SessionInfo session, PluginConfiguration config, ILogger logger)
    {
        _sessionId = session.Id;
        _userId = session.UserId.ToString("N");
        _userName = $"{session.UserName} ({session.DeviceName})";
        _config = config;
        _logger = logger;
    }

    public string? RoomId { get; private set; }

    public string UserName => _userName;

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
        if (_socket.State == WebSocketState.Open)
        {
            try
            {
                await _socket.CloseAsync(WebSocketCloseStatus.NormalClosure, "Bridge stopped", CancellationToken.None)
                    .ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is WebSocketException or ObjectDisposedException)
            {
                // Socket already closing/closed — nothing to do.
            }
        }

        if (_receiveLoop != null)
        {
            try
            {
                await _receiveLoop.ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is OperationCanceledException)
            {
                // Expected on stop.
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        await StopAsync().ConfigureAwait(false);
        _socket.Dispose();
        _cts.Dispose();
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
            _logger.LogWarning(ex, "[OpenWatchParty] Host bridge for session {SessionId} lost its connection", _sessionId);
        }
    }

    private void HandleServerMessage(string json)
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
                break;
            case "error":
                _logger.LogWarning(
                    "[OpenWatchParty] Session server rejected a message for bridged session {SessionId}: {Message}",
                    _sessionId,
                    message["payload"]?["message"]);
                break;
            case "room_closed":
                RoomId = null;
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
        await _socket.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, cancellationToken)
            .ConfigureAwait(false);
    }

    private static double TicksToSeconds(long? ticks) => (ticks ?? 0) / (double)TimeSpan.TicksPerSecond;

    internal static JObject BuildAuthPayload(string userId, string userName, PluginConfiguration config)
    {
        if (!string.IsNullOrEmpty(config.JwtSecret))
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

        return payload;
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
