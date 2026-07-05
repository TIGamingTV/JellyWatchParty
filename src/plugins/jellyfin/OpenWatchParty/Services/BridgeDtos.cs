namespace OpenWatchParty.Plugin.Services;

/// <summary>
/// A Jellyfin session that could be bridged in as an OpenWatchParty room
/// host (i.e. it's currently playing something).
/// </summary>
public sealed record BridgeableSessionInfo(
    string SessionId,
    string UserName,
    string DeviceName,
    string Client,
    string? NowPlayingItemName);

/// <summary>
/// The current state of a host bridge, for the admin config page.
/// </summary>
public sealed record BridgeStatus(
    string SessionId,
    string UserName,
    string? RoomId,
    bool Connected);
