namespace JellyWatchParty.Plugin.Services;

/// <summary>
/// A Jellyfin session that could be bridged in as an JellyWatchParty room
/// host (i.e. it's currently playing something).
/// </summary>
public sealed record BridgeableSessionInfo(
    string SessionId,
    string UserName,
    string DeviceName,
    string Client,
    string? NowPlayingItemName);

/// <summary>
/// The current state of a bridge, for the in-player widget's status display.
/// <paramref name="Role"/> is "host" (the session drives a room) or
/// "receiver" (the session follows a room).
/// </summary>
public sealed record BridgeStatus(
    string SessionId,
    string UserName,
    string? RoomId,
    bool Connected,
    string Role);
