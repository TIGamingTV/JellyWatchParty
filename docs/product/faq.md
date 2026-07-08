---
title: FAQ
parent: Getting Started
nav_order: 4
---

# Frequently Asked Questions

## General

### What is JellyWatchParty?
JellyWatchParty is a Jellyfin plugin that lets multiple users watch the same video in sync. When the host plays, pauses, or seeks, everyone follows automatically.

### Is it free?
Yes, JellyWatchParty is open source and free to use.

### Does it work with Plex or Emby?
No, JellyWatchParty is designed specifically for Jellyfin. It uses Jellyfin's plugin API and web interface.

### Do all participants need Jellyfin accounts?
Yes, everyone needs access to the same Jellyfin server and the media library.

## Setup

### Why do I need a separate session server?
The session server handles real-time WebSocket communication between clients. Jellyfin's plugin architecture doesn't support WebSocket endpoints, so a separate lightweight server is needed.

### Can I run everything on one machine?
Yes, you can run Jellyfin and the session server on the same machine. They use different ports (8096 for Jellyfin, 3000 for session server by default).

### Is Docker required?
No, but it's recommended. You can build and run the Rust session server manually if preferred.

### Why do I need to add a script tag manually?
Since Jellyfin 10.9, plugins cannot automatically inject scripts for security reasons. The manual step ensures administrators explicitly approve script injection.

## Usage

### Who controls playback?
The person who creates the room (the host) controls playback. Their play, pause, and seek actions are mirrored to all participants.

### Can participants control playback?
Not currently. Only the host can control playback. Democratic mode is planned for a future release.

### What happens if the host leaves?
If the host's connection just drops briefly (network blip, backgrounded app), the room stays open for 90 seconds waiting for them to reconnect — participants won't notice. If the host doesn't come back within that window (or explicitly leaves) and other participants are still in the room, the earliest-joined remaining participant is automatically promoted to host and the room stays open. The room only closes if no one is left.

### Can I host from Fladder or another Android TV app?
Yes, via [Host Bridge](../technical/host-bridge.md) — any logged-in user with browser access to the server can bridge a currently-playing native session in as the room host, even though that client can't run the Watch Party UI itself. Guests still join normally.

### Can I chat with other viewers?
Yes! A text chat is available in the Watch Party panel. Messages are sent to all room participants in real time via the session server. The last 50 messages are replayed to anyone who joins or reconnects, so late joiners have context.

### Can I make a room private?
Yes. When creating a room you can set an optional password; anyone joining must enter it correctly. Rooms without a password are open to anyone who can see the room list (i.e. anyone with access to your Jellyfin server).

### Does everyone need the same video quality?
No. Each client transcodes independently based on their connection and device. Sync is based on playback position, not video quality.

## Sync Quality

### How accurate is the sync?
Typically within 100-200ms. The system uses clock synchronization and drift correction to maintain sync.

### Why do I see slight speed changes?
JellyWatchParty adjusts playback speed (0.85x-2.0x) to gradually correct drift without jarring seeks. It only starts adjusting once drift passes 0.3 seconds, and stops once it's back under 0.1 seconds, so small jitter doesn't cause constant flickering. This is imperceptible in most cases.

### What if I'm several seconds behind?
If drift exceeds 2.0 seconds, the client automatically seeks to the correct position instead of adjusting speed.

### Does buffering affect sync?
Yes, buffering can cause temporary desync. The system waits for all clients to be "ready" before starting playback, and continuously corrects drift afterward.

## Technical

### What ports are used?
- **8096** - Jellyfin web interface (default)
- **3000** - Session server WebSocket (default)

### What protocol is used?
WebSocket with JSON messages. See the [Protocol Documentation](../technical/protocol.md) for details.

### Is the connection encrypted?
It can be. Use WSS (WebSocket Secure) with HTTPS for encrypted connections. See [Security Guide](../operations/security.md).

### How is authentication handled?
Optional JWT tokens can be configured. When enabled, clients must authenticate with Jellyfin before joining rooms. See [Security Guide](../operations/security.md).

## Troubleshooting

### The Watch Party button doesn't appear
1. Check that the plugin is installed (Dashboard > Plugins)
2. Verify the script tag is in Custom HTML (Dashboard > General)
3. Hard refresh the page (Ctrl+F5)

See [Troubleshooting Guide](../operations/troubleshooting.md#watch-party-button-not-visible) for detailed solutions.

### I can't connect to the session server
1. Ensure the session server is running
2. Check that port 3000 is open in your firewall
3. Verify the WebSocket URL is correct

See [Troubleshooting Guide](../operations/troubleshooting.md#cannot-connect-to-session-server) for detailed solutions.

### We're out of sync
1. Wait a few seconds - sync corrects automatically
2. If persistent, have the host pause and play again
3. Check network quality (high latency = worse sync)

See [Troubleshooting Guide](../operations/troubleshooting.md#sync-issues) for detailed solutions.

### The room keeps closing
1. Check if the host is having connection issues
2. Verify the session server is stable

See [Troubleshooting Guide](../operations/troubleshooting.md#room-closes-unexpectedly) for detailed solutions.

## Support

### Where can I report bugs?
Open an issue on the [GitHub repository](https://github.com/TIGamingTV/JellyWatchParty/issues).

### How can I contribute?
See the [Contributing Guide](../development/contributing.md) for how to submit pull requests.

### Where can I get help?
- GitHub Issues for bugs and feature requests
- GitHub Discussions for questions
- Jellyfin community forums
