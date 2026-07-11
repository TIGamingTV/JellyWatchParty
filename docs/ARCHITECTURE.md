# Architecture Overview

## Three components

```
┌─────────────────────┐      injects <script>       ┌──────────────────────┐
│  Jellyfin Server     │ ───────────────────────────▶│  Jellyfin Web (HTML)  │
│  + JellyWatchParty     │   via ScriptInjectionMiddle- │  running in browser   │
│  Plugin (C#)          │   ware, into index.html      │  (or JMP desktop app) │
└──────────┬───────────┘                              └──────────┬───────────┘
           │ serves config page,                                  │ runs injected JS,
           │ /JellyWatchParty/Token,                                │ opens WebSocket
           │ /JellyWatchParty/ClientScript                          │
           │                                                       ▼
           │                                          ┌──────────────────────┐
           │  (for browsers: NO network calls           │  Session Server (Rust) │
           │   between plugin backend and               │  warp-based WS server  │
           │   session server — only the browser        │  rooms/host/broadcast  │
           │   talks to it. One admin-triggered          └──────────────────────┘
           │   exception: HostBridgeManager can                     ▲
           │   open its own WebSocket to bridge a                    │
           │   native session in as a room host —                   │
           │   see "Jellyfin plugin" section below)  ────────────────┘
           ▼
   Jellyfin Dashboard config UI
```

## 1. Rust session server — `src/server/`

Built with `warp`. Key files:

- `src/main.rs` — entry point; reads `HOST`/`PORT`/`ALLOWED_ORIGINS` env vars,
  spawns the zombie-cleanup background task, builds routes, starts the server.
  Default port `3000` (overridable via `PORT` env var — the user's deployment
  maps the host port to `3238`, purely a Docker port-mapping detail with no
  code-level assumption baked in).
- `src/routes.rs` — warp filter definitions: `/ws` (WebSocket upgrade, with
  origin-checking and — as of Round 10 — a `client_id` query param) and
  `/health`.
- `src/ws/connection.rs` — per-connection lifecycle: registers or reattaches
  a client, sends `client_hello` + `room_list`, reads incoming messages in a
  loop, and on disconnect schedules teardown (see `room/reconnect.rs`).
- `src/ws/dispatch.rs` — routes incoming `IncomingMessage`s by type to
  handlers in `src/ws/handlers/`.
- `src/ws/handlers/` — one file per message type: `create.rs`, `join.rs`,
  `leave` (in `room/leave.rs`), `playback`, `sync`, etc.
- `src/room/` — room lifecycle: `close.rs` (host starts a new room, old one
  closes), `leave.rs` (client leaves/disconnects, room torn down if empty or
  host left), `reconnect.rs` (Round 10 addition — grace-period disconnect +
  reattachment room_state resend).
- `src/types.rs` — core data structures: `Client` (per-connection state,
  keyed by client_id in the `Clients` map), `Room` (keyed by room_id in the
  `Rooms` map), message enums.
- `src/tasks.rs` — background zombie-connection cleanup (checks `last_seen`
  against a timeout, currently ~60s check interval).
- `src/messaging.rs` — helpers for sending to one client / broadcasting to a
  room / broadcasting the room list to everyone.
- `src/auth.rs` — JWT validation (optional — can be disabled via config,
  in which case all clients connect as "anonymous"/"Anonymous").

State is `Arc<RwLock<HashMap<...>>>` for both `Clients` and `Rooms` — no
external database, everything is in-memory and lost on restart.

## 2. Jellyfin plugin — `src/plugins/jellyfin/JellyWatchParty/`

C# plugin targeting the Jellyfin plugin ABI. Key files:

- `JellyWatchPartyPlugin.csproj` — package metadata, Jellyfin SDK version
  pins (`Jellyfin.Controller`, `Jellyfin.Model`, currently `10.11.11`).
- `Plugin.cs` — plugin entry point, registers config page.
- `Configuration/PluginConfiguration.cs` — plugin settings, notably
  `SessionServerUrl` (a plain string, e.g. `wss://host/ws` — used verbatim,
  no validation/transformation applied server-side).
- `Controllers/JellyWatchPartyController.cs` — HTTP endpoints:
  - `/JellyWatchParty/Token` — issues a JWT (or a no-auth response) and
    echoes back the configured `session_server_url` for the client JS to use
  - `/JellyWatchParty/ClientScript` — serves `plugin.js` (the loader)
  - serves the rest of the client JS files from **embedded resources only**
    (no disk fallback) — see `Web/` folder produced at build time, resource
    name pattern `JellyWatchParty.Plugin.Web.<path-with-dots-for-slashes>`
  - `/JellyWatchParty/Bridge/*` (any logged-in user — see below) — lists
    bridgeable/active sessions and starts/stops a host bridge, or attaches a
    session to a room as a receiver (`Bridge/{sessionId}/Follow`)
- `ScriptInjectionMiddleware.cs` — intercepts requests for
  `/web/index.html` and injects a `<script>` tag pointing at
  `/JellyWatchParty/ClientScript` before the response is served. Its
  `ServiceRegistrator` also registers `HostBridgeManager` as a singleton +
  hosted service.
- `Services/SessionServerAuth.cs` — JWT minting, shared by the `/Token`
  endpoint and the host bridge (which mints a token for the bridged
  session's owner, not the current HTTP caller).
- `Services/HostBridgeManager.cs` — hosted service; subscribes to
  `ISessionManager`'s playback events for the server's lifetime and owns
  all currently-active `SessionHostBridge` instances, keyed by Jellyfin
  session id. `GetEligibleSessions()` excludes sessions whose `Client`
  starts with `"Jellyfin Web"`, `"Jellyfin Desktop"`, or `"Jellyfin Media
  Player"` (prefix match — `Client` includes a trailing version, e.g.
  `"Jellyfin Web 10.11.11"`, `"Jellyfin Desktop 3.0.0-dev"`, confirmed
  against a live server) — those already run the injected script and can
  host via the normal "Create Room" button, so they're left out of the
  bridge picker to avoid clutter.
- `Services/SessionHostBridge.cs` — one bridge: owns a `ClientWebSocket` to
  the session server for one Jellyfin session, translating its
  `PlaybackStart`/`PlaybackProgress`/`PlaybackStopped` events into
  `create_room`/`player_event`/`state_update` messages.
- `Services/SessionFollowerBridge.cs` — the receive-only counterpart: owns a
  `ClientWebSocket` that `join_room`s an existing room and translates the
  host's inbound `player_event`/`state_update` into
  `ISessionManager.SendPlaystateCommand` calls (Pause/Unpause/absolute Seek)
  against the session, so a native client (e.g. official Android TV) can
  follow a room. Managed alongside host bridges by `HostBridgeManager`.

**Critical fact**: for the browser/config-page path, this plugin has zero
outbound network calls to the session server — it only ever hands the
browser a token and a URL, and the browser does the talking. All the JS
files under `Web/` need to be **embedded into the compiled DLL at build
time** — there is no way to "hot-patch" a running installation by dropping
files next to the DLL; a genuine rebuild is required for any JS change to
take effect (see Round 6's root cause).

**One deliberate exception** (Round 16, moved from the admin dashboard into
the in-player widget in Round 17): `Services/HostBridgeManager.cs` and
`Services/SessionHostBridge.cs` let any logged-in user bridge a
currently-playing Jellyfin session (e.g. Fladder on Android TV, which can't
run the injected browser script at all) into a new JellyWatchParty room as
its host. This *is* outbound network calls from the plugin backend —
`HostBridgeManager` is a hosted service that subscribes to
`ISessionManager`'s playback events, and for each bridged session
`SessionHostBridge` opens its own `ClientWebSocket` to the session server
and speaks the exact same client protocol a browser host would (`auth` →
`create_room` → `player_event`/`state_update`). The resulting room is
indistinguishable from a browser-hosted one to guests, who still join it
themselves from their own Jellyfin Web room list exactly as before —
nothing is pushed to guests, and this only makes the *host* side work for
native clients. `Services/SessionServerAuth.cs` holds the shared
JWT-minting logic used by both this bridge and the `/JellyWatchParty/Token`
endpoint. The `/JellyWatchParty/Bridge/*` endpoints are gated with plain
`[Authorize]` (not an admin-only policy) — session info (username, device,
now-playing title) is deliberately not treated as private within a server,
and any user can start/stop a bridge from the same panel where they'd
create or join a room. Driven from `src/clients/jellyfin-web/ui/bridge.js`,
rendered inside the existing lobby panel (`ui/render.js`'s `renderLobby`) —
not the Jellyfin admin config page.

## 3. Injected JS client — `src/clients/jellyfin-web/`

This is what actually creates the UI and drives sync in the browser. Not a
bundled SPA — plain files, loaded dynamically by `plugin.js` fetching each
one by path from `/JellyWatchParty/Client/{path}`. The authoritative list of
files (and their load order) lives in `infra/just/common.just` as the
`client_js_files` variable — **must be kept in sync** with whatever the
publish workflow's copy step embeds (see Round 6).

Key files:
- `plugin.js` — the only file directly injected via `<script src=...>`; a
  loader that sequentially fetches and evals the rest.
- `state.js` — global mutable state object (`JellyWatchParty.state`):
  connection state, room state, `isHost`, `syncStatus`, etc.
- `ws/connection.js` — opens/manages the WebSocket connection; as of
  Round 10, generates and persists a UUID (`localStorage`) and sends it as
  `?client_id=...` on every connection attempt.
- `ws/auth.js` — fetches the JWT + session server URL from
  `/JellyWatchParty/Token`.
- `ws/handlers/*.js` — one file per incoming message type from the server
  (`room.js`, `sync.js`, `playback.js`, `clock.js`).
- `playback/bind.js` — attaches listeners to the host's `<video>` element
  (play/pause/seek), emits them as outgoing WS messages (logged as
  `[JWP:HOST] action=... pos=...`).
- `playback/sync.js` — the non-host drift-correction loop: compares local
  video position against the host's broadcast state, sets `state.syncStatus`
  to `'synced'` or `'syncing'` accordingly. **This is the loop that must
  actually be running for sync to work at all** — if it never starts (e.g.
  no compatible video element found), nothing corrects drift, but prior to
  Round 11's fix, the UI still claimed "In sync" by default.
- `utils/video.js` — `getVideo()`, locates the active `<video>` DOM element.
  This is the piece most likely to fail on non-standard players.
- `ui/render.js` — injects the header button (`headerRight.prepend/append`)
  and OSD button; currently uses the same Material Icon (`groups`) as
  Jellyfin's native SyncPlay button (Round 7 — icon collision, fix
  recommended but not confirmed applied).
- `ui/indicators.js` — renders the sync status dot/label (Round 11 — being
  reworked to not lie about unknown status).
- `ui/bridge.js` (Round 17) — renders the "Host From Another Device"
  section in the lobby panel (`ui/render.js`'s `renderLobby`); calls
  `/JellyWatchParty/Bridge/*` directly with the user's own Jellyfin access
  token (`ApiClient.accessToken()`), same pattern as `ws/auth.js`'s token
  fetch. Reference-only for other sessions — starting/stopping a bridge
  doesn't push anything to anyone; guests still join the resulting room
  from the normal room list above it.
- `chat/` — in-room text chat, separate from sync logic.

## Build & release pipeline

- `infra/just/common.just` — shared variables (`plugin_dir`, `client_dir`,
  `client_js_files` list, `ft_abi`).
- `infra/just/build.just` — `just build plugin`, `just build server`, etc.
  for local builds (requires dotnet/cargo locally, or via the dev Docker
  setup in `infra/docker/dev/`).
- `.github/workflows/publish.yml` — CI/CD: on push to `src/server/**`
  builds+pushes the Docker image; on GitHub Release, builds the plugin,
  zips it, attaches it to the release, and updates
  `docs/jellyfin-plugin-repo/manifest.json` (served via GitHub Pages).
- `.github/workflows/docs.yml` — deploys `docs/` via GitHub Pages
  (Jekyll), which is also how the plugin manifest becomes publicly fetchable.
- **Docs layout (post docs-rework)**: the published site is now a flat set
  of top-level pages (`docs/index.md`, `installation.md`, `features.md`,
  `core-structure.md`, `user-guide.md`, `configuration.md`,
  `troubleshooting.md`, `deployment.md`, `security.md`) plus two collapsed
  sections for deeper/contributor content: `docs/technical/` (protocol,
  server, client, plugin, sync, host-bridge) and `docs/development/`
  (setup, contributing, testing, release, ci). The old `product/` and
  `operations/` directories and `technical/architecture.md`/`api.md` no
  longer exist — their content was merged into the pages above. This file
  and `PROGRESS.md` remain excluded from the build (see `_config.yml`).
- `docs/jellyfin-plugin-repo/manifest.json` — the Jellyfin plugin repository
  manifest; must contain valid `System.Version`-parseable version strings
  (N.N, N.N.N, or N.N.N.N — nothing else) or it will crash Jellyfin's
  Catalog page for *every* repository configured, not just this one.

## Deployment topology (this user's setup)

```
Internet ──HTTPS/WSS──▶ nginx (DDNS domain, TLS) ──┬──▶ Jellyfin (8096)
                                                     └──▶ session server (3238)
                                                          via /ws location block
```

- Jellyfin plugin config → Session Server URL:
  `wss://jellyfin.homeserver-tom-tykwer.dedyn.io/ws`
- Session server env: `ALLOWED_ORIGINS=https://jellyfin.homeserver-tom-tykwer.dedyn.io`
- No port-forward needed for the session server itself once the nginx `/ws`
  proxy is in place — only nginx's 443 needs to be internet-reachable.

## File-Transformation Integration (implementation detail)

The public installation guide (`docs/installation.md`, "Enable the Client
Script" > Option A) covers the admin-facing summary: install
[jellyfin-plugin-file-transformation](https://github.com/IAmParadox27/jellyfin-plugin-file-transformation)
and JellyWatchParty auto-injects its client `<script>` tag into
`index.html`, no Custom HTML step needed. This section is the
implementation detail behind that, for contributors.

`FileTransformationIntegration.cs` detects whether file-transformation
is installed and, if so, registers a transformation that injects the
client `<script>` tag before `</body>`. If file-transformation isn't
installed, `ScriptInjectionMiddleware` still handles injection the
normal way and the admin can also always fall back to the manual
Custom HTML method.

**Registration payload:**

```csharp
var payload = new {
    id = new Guid(Plugin.PluginGuid),
    fileNamePattern = @"^index\.html$",
    callbackAssembly = typeof(FileTransformationIntegration).Assembly.FullName,
    callbackClass = typeof(FileTransformationIntegration).FullName,
    callbackMethod = nameof(TransformIndexHtml)
};
```

**Registration via reflection** — Jellyfin loads plugins in separate
`AssemblyLoadContext`s, so direct type references to the
file-transformation plugin are impossible:

```csharp
Assembly? ftAssembly = AssemblyLoadContext.All
    .SelectMany(ctx => ctx.Assemblies)
    .FirstOrDefault(asm => asm.FullName?.Contains(".FileTransformation") ?? false);

if (ftAssembly != null)
{
    Type? pluginInterface = ftAssembly.GetType("Jellyfin.Plugin.FileTransformation.PluginInterface");
    MethodInfo? registerMethod = pluginInterface?.GetMethod("RegisterTransformation");
    registerMethod?.Invoke(null, new object?[] { payload });
}
```

**Transformation callback** — idempotent (won't double-inject if the
script tag is already present):

```csharp
public static string TransformIndexHtml(object payload)
{
    string? contents = payload?.GetType()
        .GetProperty("contents")?
        .GetValue(payload)?
        .ToString();

    if (string.IsNullOrEmpty(contents) || contents.Contains("/JellyWatchParty/ClientScript"))
    {
        return contents ?? string.Empty;
    }

    int bodyEndIndex = contents.LastIndexOf("</body>", StringComparison.OrdinalIgnoreCase);
    if (bodyEndIndex >= 0)
    {
        return contents.Insert(bodyEndIndex, "<script src=\"/JellyWatchParty/ClientScript\"></script>\n");
    }

    return contents;
}
```

**Files:** `src/plugins/jellyfin/JellyWatchParty/FileTransformationIntegration.cs`
(registration + callback), `JellyWatchPartyPlugin.csproj` (Newtonsoft.Json
dependency needed for the payload shape).

**Error handling:** any failure (plugin not installed, incompatible
version, exception during registration) is logged and falls back to the
manual Custom HTML method — never a hard failure.

## Jellyfin SyncPlay Reference

Jellyfin's own built-in synchronized-playback feature, SyncPlay,
informed some of JellyWatchParty's design decisions. Kept here as a
reference for contributors, not published on the public docs site.

**Key differences from JellyWatchParty:**

| Aspect | Jellyfin SyncPlay | JellyWatchParty |
|--------|-------------------|----------------|
| Architecture | Integrated into Jellyfin | Standalone plugin + server |
| Transport | REST API + Jellyfin messages | Dedicated WebSocket |
| Time sync | Min-delay selection | EMA smoothing |
| Server | C# (same as Jellyfin) | Rust |
| Client support | All official clients | Browser/Jellyfin Desktop for guests; any client can host via Host Bridge |

### Source code locations

Server (C#, [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin)):
`Emby.Server.Implementations/SyncPlay/` (`SyncPlayManager.cs`,
`Group.cs`), `MediaBrowser.Controller/SyncPlay/` (interfaces, group
states, playback/queue requests).

Client (JS/TS, [jellyfin/jellyfin-web](https://github.com/jellyfin/jellyfin-web)):
`src/plugins/syncPlay/` — `Manager.js`/`Controller.js` (orchestration),
`PlaybackCore.js`/`QueueCore.js`, `timeSync/{TimeSync,TimeSyncCore,TimeSyncServer}.js`.

### Time synchronization: min-delay selection, not EMA

SyncPlay uses an NTP-like algorithm but picks the **best** sample
instead of smoothing across samples:

```javascript
// Four timestamps per ping: requestSent, requestReceived (server),
// responseSent (server), responseReceived
offset = ((requestReceived - requestSent) + (responseSent - responseReceived)) / 2;
delay = (responseReceived - requestSent) - (responseSent - requestReceived);
```

It keeps a sliding window of 8 measurements, sorts by round-trip delay,
and takes the lowest-delay sample as the best estimate (rationale:
jitter adds delay, it doesn't reduce it — so the fastest sample is
probably the least distorted one). Polling is greedy (1000ms) for the
first 3 pings, then drops to 60000ms. JellyWatchParty instead uses
continuous EMA smoothing (α=0.4) with 10s polling — simpler to reason
about, trades off some resistance to single-sample outliers.

### Drift correction: SpeedToSync + SkipToSync

```javascript
// SpeedToSync — smooth catch-up within thresholds
if (drift >= minDelaySpeedToSync && drift <= maxDelaySpeedToSync) {
    // adjust playback rate, 0.2x-2.0x range, over speedToSyncDuration
}
// SkipToSync — hard seek above threshold
if (drift > minDelaySkipToSync) {
    player.seek(estimatedServerPosition);
}
```

Comparable in shape to JellyWatchParty's hysteresis + sqrt-curve rate
adjustment (see `docs/technical/sync.md`), but SyncPlay's thresholds are
user-configurable (`minDelaySpeedToSync`/`maxDelaySpeedToSync`/
`minDelaySkipToSync` in `Settings.js`) rather than fixed constants, and
its rate range (0.2x-2.0x) is wider on the low end than JellyWatchParty's
(0.85x-2.0x).

### Known SyncPlay limitations (from upstream GitHub issues)

1. Transcoding delay — users requiring transcoding tend to run ~2s behind
2. Sync correction can misfire when precise sync isn't actually needed
3. Occasional further desync after a pause/resume cycle

### References

- [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin),
  [jellyfin/jellyfin-web](https://github.com/jellyfin/jellyfin-web)
- Key PRs: [#1011](https://github.com/jellyfin/jellyfin-web/pull/1011)
  (original implementation), [#3976](https://github.com/jellyfin/jellyfin-web/pull/3976)
  (moved to plugin architecture)
- Known issues: [#4972](https://github.com/jellyfin/jellyfin-web/issues/4972),
  [#6210](https://github.com/jellyfin/jellyfin-web/issues/6210) (desync when transcoding)
