---
title: Technical Reference
nav_order: 10
has_children: true
---

# Technical Reference

Implementation-level detail for contributors and advanced readers:

- [Protocol]({{ '/technical/protocol/' | relative_url }}): the WebSocket messages between clients and the session server
- [Server]({{ '/technical/server/' | relative_url }}): session server internals
- [Client]({{ '/technical/client/' | relative_url }}): the injected web client's modules
- [Plugin]({{ '/technical/plugin/' | relative_url }}): the Jellyfin plugin, script injection and its REST API
- [Sync Algorithms]({{ '/technical/sync/' | relative_url }}): clock sync, scheduling and drift correction
- [Host Bridge]({{ '/technical/host-bridge/' | relative_url }}): bridging native clients from the Watch Party panel
- [Integration API]({{ '/technical/integration-api/' | relative_url }}): the session server's API for the Discord and Telegram bots

For a high-level overview of how the system fits together, see
[Core Structure]({{ '/core-structure/' | relative_url }}). To write your own client, see
[Third-Party Clients]({{ '/development/third-party-clients/' | relative_url }}).
