# Architecture Overview

## Three components

```
┌─────────────────────┐      injects <script>       ┌──────────────────────┐
│  Jellyfin Server     │ ───────────────────────────▶│  Jellyfin Web (HTML)  │
│  + OpenWatchParty     │   via ScriptInjectionMiddle- │  running in browser   │
│  Plugin (C#)          │   ware, into index.html      │  (or JMP desktop app) │
└──────────┬───────────┘                              └──────────┬───────────┘
           │ serves config page,                                  │ runs injected JS,
           │ /OpenWatchParty/Token,                                │ opens WebSocket
           │ /OpenWatchParty/ClientScript                          │
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

## 2. Jellyfin plugin — `src/plugins/jellyfin/OpenWatchParty/`

C# plugin targeting the Jellyfin plugin ABI. Key files:

- `OpenWatchPartyPlugin.csproj` — package metadata, Jellyfin SDK version
  pins (`Jellyfin.Controller`, `Jellyfin.Model`, currently `10.11.11`).
- `Plugin.cs` — plugin entry point, registers config page.
- `Configuration/PluginConfiguration.cs` — plugin settings, notably
  `SessionServerUrl` (a plain string, e.g. `wss://host/ws` — used verbatim,
  no validation/transformation applied server-side).
- `Controllers/OpenWatchPartyController.cs` — HTTP endpoints:
  - `/OpenWatchParty/Token` — issues a JWT (or a no-auth response) and
    echoes back the configured `session_server_url` for the client JS to use
  - `/OpenWatchParty/ClientScript` — serves `plugin.js` (the loader)
  - serves the rest of the client JS files from **embedded resources only**
    (no disk fallback) — see `Web/` folder produced at build time, resource
    name pattern `OpenWatchParty.Plugin.Web.<path-with-dots-for-slashes>`
  - `/OpenWatchParty/Bridge/*` (admin-only) — lists bridgeable/active
    sessions and starts/stops a host bridge; see below
- `ScriptInjectionMiddleware.cs` — intercepts requests for
  `/web/index.html` and injects a `<script>` tag pointing at
  `/OpenWatchParty/ClientScript` before the response is served. Its
  `ServiceRegistrator` also registers `HostBridgeManager` as a singleton +
  hosted service.
- `Services/SessionServerAuth.cs` — JWT minting, shared by the `/Token`
  endpoint and the host bridge (which mints a token for the bridged
  session's owner, not the current HTTP caller).
- `Services/HostBridgeManager.cs` — hosted service; subscribes to
  `ISessionManager`'s playback events for the server's lifetime and owns
  all currently-active `SessionHostBridge` instances, keyed by Jellyfin
  session id.
- `Services/SessionHostBridge.cs` — one bridge: owns a `ClientWebSocket` to
  the session server for one Jellyfin session, translating its
  `PlaybackStart`/`PlaybackProgress`/`PlaybackStopped` events into
  `create_room`/`player_event`/`state_update` messages.

**Critical fact**: for the browser/config-page path, this plugin has zero
outbound network calls to the session server — it only ever hands the
browser a token and a URL, and the browser does the talking. All the JS
files under `Web/` need to be **embedded into the compiled DLL at build
time** — there is no way to "hot-patch" a running installation by dropping
files next to the DLL; a genuine rebuild is required for any JS change to
take effect (see Round 6's root cause).

**One deliberate exception** (Round 16): `Services/HostBridgeManager.cs` and
`Services/SessionHostBridge.cs` let an admin bridge a currently-playing
Jellyfin session (e.g. Fladder on Android TV, which can't run the injected
browser script at all) into a new OpenWatchParty room as its host. This *is*
outbound network calls from the plugin backend — `HostBridgeManager` is a
hosted service that subscribes to `ISessionManager`'s playback events, and
for each bridged session `SessionHostBridge` opens its own `ClientWebSocket`
to the session server and speaks the exact same client protocol a browser
host would (`auth` → `create_room` → `player_event`/`state_update`). The
resulting room is indistinguishable from a browser-hosted one to guests, who
still join it themselves from their own Jellyfin Web room list exactly as
before — nothing is pushed to guests, and this only makes the *host* side
work for native clients. `Services/SessionServerAuth.cs` holds the shared
JWT-minting logic used by both this bridge and the `/OpenWatchParty/Token`
endpoint. New endpoints under `/OpenWatchParty/Bridge/*` (admin-only,
`RequiresElevation` policy) drive it from the config page.

## 3. Injected JS client — `src/clients/jellyfin-web/`

This is what actually creates the UI and drives sync in the browser. Not a
bundled SPA — plain files, loaded dynamically by `plugin.js` fetching each
one by path from `/OpenWatchParty/Client/{path}`. The authoritative list of
files (and their load order) lives in `infra/just/common.just` as the
`client_js_files` variable — **must be kept in sync** with whatever the
publish workflow's copy step embeds (see Round 6).

Key files:
- `plugin.js` — the only file directly injected via `<script src=...>`; a
  loader that sequentially fetches and evals the rest.
- `state.js` — global mutable state object (`OpenWatchParty.state`):
  connection state, room state, `isHost`, `syncStatus`, etc.
- `ws/connection.js` — opens/manages the WebSocket connection; as of
  Round 10, generates and persists a UUID (`localStorage`) and sends it as
  `?client_id=...` on every connection attempt.
- `ws/auth.js` — fetches the JWT + session server URL from
  `/OpenWatchParty/Token`.
- `ws/handlers/*.js` — one file per incoming message type from the server
  (`room.js`, `sync.js`, `playback.js`, `clock.js`).
- `playback/bind.js` — attaches listeners to the host's `<video>` element
  (play/pause/seek), emits them as outgoing WS messages (logged as
  `[OWP:HOST] action=... pos=...`).
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
