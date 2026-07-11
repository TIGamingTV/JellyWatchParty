using System.Net.WebSockets;
using System.Text;
using MediaBrowser.Controller.Session;
using MediaBrowser.Model.Session;
using Microsoft.Extensions.Logging;
using Newtonsoft.Json.Linq;
using JellyWatchParty.Plugin.Configuration;

namespace JellyWatchParty.Plugin.Services;

/// <summary>
/// Attaches one Jellyfin session to an existing JellyWatchParty room as a
/// <b>receiver</b> (follower): it joins the room over its own WebSocket to the
/// Rust session server — exactly as a browser guest would — and translates the
/// host's inbound <c>player_event</c>/<c>state_update</c> messages into
/// <see cref="ISessionManager.SendPlaystateCommand(string, string, PlaystateRequest, CancellationToken)"/>
/// calls against the target session. This is how an official native client
/// (e.g. Android TV), which can't run the injected JWP web script, can still be
/// kept in sync with a room: the client already honours remote-control
/// playstate commands (Pause / Unpause / absolute Seek) over its own socket.
///
/// This is the receive-only counterpart to <see cref="SessionHostBridge"/>,
/// which drives a room *from* a session. A given session is a host or a
/// follower for a room, never both at once.
/// </summary>
public sealed class SessionFollowerBridge : IAsyncDisposable
{
    private const int ReceiveBufferSize = 8 * 1024;

    // A follower only corrects position when it has drifted noticeably from
    // the room, and never more than once per cooldown, so drift-correction
    // Seeks don't spam the native client mid-playback. These mirror the
    // philosophy of the server's own jitter/cooldown constants.
    private const double DriftThresholdSeconds = 2.0;
    private static readonly TimeSpan SeekCooldown = TimeSpan.FromSeconds(3);

    private readonly string _sessionId;
    private readonly string _userId;
    private readonly string _userName;
    private readonly string _roomId;
    private readonly PluginConfiguration _config;
    private readonly ISessionManager _sessionManager;
    private readonly ILogger _logger;
    private readonly ClientWebSocket _socket = new();
    private readonly CancellationTokenSource _cts = new();

    private Task? _receiveLoop;
    private bool? _lastCommandedPaused;
    private DateTime _lastSeekAt = DateTime.MinValue;

    public SessionFollowerBridge(
        SessionInfo session,
        string roomId,
        PluginConfiguration config,
        ISessionManager sessionManager,
        ILogger logger)
    {
        _sessionId = session.Id;
        _userId = session.UserId.ToString("N");
        _userName = $"{session.UserName} ({session.DeviceName})";
        _roomId = roomId;
        _config = config;
        _sessionManager = sessionManager;
        _logger = logger;
    }

    /// <summary>The room this session is following, or null once it closes.</summary>
    public string? RoomId { get; private set; }

    public string UserName => _userName;

    public bool Connected => _socket.State == WebSocketState.Open;

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        await _socket.ConnectAsync(new Uri(_config.SessionServerUrl), cancellationToken).ConfigureAwait(false);

        await SendAsync(SessionHostBridge.BuildAuthPayload(_userId, _userName, _config), "auth", room: null, cancellationToken)
            .ConfigureAwait(false);
        await SendAsync(BuildJoinRoomPayload(_userName), "join_room", _roomId, cancellationToken)
            .ConfigureAwait(false);

        // RoomId is set only once the server confirms the join with a
        // room_state message (see HandleServerMessageAsync) — not optimistically
        // here — so a rejected join (e.g. a password-protected room, which this
        // bridge does not support) does not surface as a phantom connected bridge.
        _receiveLoop = Task.Run(() => ReceiveLoopAsync(_cts.Token), CancellationToken.None);
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
            catch (OperationCanceledException)
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

                await HandleServerMessageAsync(messageBuilder.ToString(), cancellationToken).ConfigureAwait(false);
                messageBuilder.Clear();
            }
        }
        catch (OperationCanceledException)
        {
            // Expected on stop.
        }
        catch (WebSocketException ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Follower bridge for session {SessionId} lost its connection", _sessionId);
        }
    }

    private async Task HandleServerMessageAsync(string json, CancellationToken cancellationToken)
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
                // The join is confirmed. Mark ourselves ready *now* — a headless
                // follower has no video to buffer, and if it never readied the
                // server's all_ready gate (pending_play) would delay every host
                // "play" for the whole room by MAX_READY_WAIT_MS. `ready` persists
                // for the room's lifetime, so sending it once on join is enough.
                RoomId = _roomId;
                await SendAsync(BuildReadyPayload(), "ready", _roomId, cancellationToken).ConfigureAwait(false);
                var initial = ParseRoomEvent(message);
                if (initial.HasValue)
                {
                    await ApplyRoomStateAsync(initial.Value, cancellationToken).ConfigureAwait(false);
                }

                break;
            case "room_closed":
                RoomId = null;
                break;
            case "error":
                _logger.LogWarning(
                    "[JellyWatchParty] Session server rejected a message for follower session {SessionId}: {Message}",
                    _sessionId,
                    message["payload"]?["message"]);
                break;
            default:
                var roomState = ParseRoomEvent(message);
                if (roomState.HasValue)
                {
                    await ApplyRoomStateAsync(roomState.Value, cancellationToken).ConfigureAwait(false);
                }

                break;
        }
    }

    private async Task ApplyRoomStateAsync(RoomPlaybackState roomState, CancellationToken cancellationToken)
    {
        // Correct position first so play/unpause lands at the right spot.
        if (roomState.PositionSeconds is { } target)
        {
            var session = _sessionManager.Sessions.FirstOrDefault(s => s.Id == _sessionId);
            var current = TicksToSeconds(session?.PlayState?.PositionTicks);
            if (ShouldSeek(target, current, DriftThresholdSeconds, DateTime.UtcNow - _lastSeekAt, SeekCooldown))
            {
                _lastSeekAt = DateTime.UtcNow;
                await SendPlaystateAsync(PlaystateCommand.Seek, SecondsToTicks(target), cancellationToken)
                    .ConfigureAwait(false);
            }
        }

        if (roomState.IsPaused is { } paused && paused != _lastCommandedPaused)
        {
            _lastCommandedPaused = paused;
            await SendPlaystateAsync(ResolvePlayPauseCommand(paused), seekPositionTicks: null, cancellationToken)
                .ConfigureAwait(false);
        }
    }

    private async Task SendPlaystateAsync(PlaystateCommand command, long? seekPositionTicks, CancellationToken cancellationToken)
    {
        var request = new PlaystateRequest
        {
            Command = command,
            SeekPositionTicks = seekPositionTicks,
        };

        try
        {
            // An empty controlling-session id skips Jellyfin's control-permission
            // path entirely (see SessionManager.SendPlaystateCommand) and simply
            // relays the command to the target session's socket.
            await _sessionManager.SendPlaystateCommand(string.Empty, _sessionId, request, cancellationToken)
                .ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            _logger.LogWarning(
                ex,
                "[JellyWatchParty] Failed to send {Command} to follower session {SessionId}",
                command,
                _sessionId);
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

    // --- Pure helpers (unit-tested; no socket or session manager needed) ---

    /// <summary>
    /// The play/pause and position a room event carries, as far as a follower
    /// cares. Either field may be absent (the event didn't mention it).
    /// </summary>
    internal readonly record struct RoomPlaybackState(bool? IsPaused, double? PositionSeconds);

    internal static JObject BuildJoinRoomPayload(string userName) => new() { ["user_name"] = userName };

    // The server's ready handler keys off the connection's client id and the
    // envelope room only; the payload body is unused, so an empty object is fine.
    internal static JObject BuildReadyPayload() => new();

    /// <summary>
    /// Extracts the follower-relevant playback state from an inbound server
    /// message. Handles the three message shapes that carry room state:
    /// <c>player_event</c> (<c>action</c> + <c>position</c>), <c>state_update</c>
    /// (<c>play_state</c> + <c>position</c>), and the <c>room_state</c> snapshot
    /// sent on join (nested <c>state.play_state</c> + <c>state.position</c>).
    /// Returns null for messages that carry no playback state.
    /// </summary>
    internal static RoomPlaybackState? ParseRoomEvent(JObject message)
    {
        var payload = message["payload"] as JObject;
        if (payload == null)
        {
            return null;
        }

        switch (message["type"]?.ToString())
        {
            case "player_event":
                return new RoomPlaybackState(
                    PausedFromAction(payload["action"]?.ToString()),
                    ReadDouble(payload["position"]));
            case "state_update":
                return new RoomPlaybackState(
                    PausedFromPlayState(payload["play_state"]?.ToString()),
                    ReadDouble(payload["position"]));
            case "room_state":
                var inner = payload["state"] as JObject;
                if (inner == null)
                {
                    return null;
                }

                return new RoomPlaybackState(
                    PausedFromPlayState(inner["play_state"]?.ToString()),
                    ReadDouble(inner["position"]));
            default:
                return null;
        }
    }

    internal static PlaystateCommand ResolvePlayPauseCommand(bool isPaused) =>
        isPaused ? PlaystateCommand.Pause : PlaystateCommand.Unpause;

    internal static long SecondsToTicks(double seconds) => (long)(seconds * TimeSpan.TicksPerSecond);

    /// <summary>
    /// A follower seeks only when it is out of the room by more than
    /// <paramref name="thresholdSeconds"/> and the previous seek is older than
    /// <paramref name="cooldown"/>, so routine progress jitter and rapid
    /// consecutive updates don't spam the client.
    /// </summary>
    internal static bool ShouldSeek(
        double targetSeconds,
        double currentSeconds,
        double thresholdSeconds,
        TimeSpan sinceLastSeek,
        TimeSpan cooldown) =>
        Math.Abs(targetSeconds - currentSeconds) > thresholdSeconds && sinceLastSeek >= cooldown;

    private static bool? PausedFromAction(string? action) => action switch
    {
        "pause" or "buffering" => true,
        "play" => false,
        _ => null,
    };

    private static bool? PausedFromPlayState(string? playState) => playState switch
    {
        "paused" => true,
        "playing" => false,
        _ => null,
    };

    private static double? ReadDouble(JToken? token) =>
        token is { Type: JTokenType.Float or JTokenType.Integer } ? (double)token : null;
}
