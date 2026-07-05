# Progress Log

Chronological record of every issue found and fixed (or partially fixed) in
the prior chat session, before this project was handed off to Claude Code.
Nothing here has been compiled or run — the prior session had no local
Docker/dotnet/cargo toolchain available, so all fixes were reasoned through
by reading source and delivered as full-file replacements pasted manually
into GitHub's web editor. **Compiling and testing everything below is the
first priority in this project.**

---

## Round 1 — Fork rebranding

The repo still referenced the original upstream project throughout. Fixed:

- `.github/workflows/publish.yml`:
  - `IMAGE_NAME` → `tigamingtv/owp-session-server`
  - plugin manifest `sourceUrl` → `https://github.com/TIGamingTV/OpenWatchParty/releases/...`
  - `targetAbi` → `"10.11.11.0"` (was `"10.10.0.0"`)
- `src/plugins/jellyfin/OpenWatchParty/OpenWatchPartyPlugin.csproj`:
  - `Authors`, `RepositoryUrl` → TIGamingTV
  - `Jellyfin.Controller` / `Jellyfin.Model` NuGet package versions bumped
    `10.11.6` → `10.11.11` to match the target server
- `infra/docker/dev/docker-compose.yml`: dev/test Jellyfin image
  `jellyfin/jellyfin:10.11.6` → `jellyfin/jellyfin:10.11.11`
- `docs/jellyfin-plugin-repo/manifest.json`: reset to `owner: TIGamingTV`,
  `versions: []` (empty — the publish workflow prepends new entries on future
  releases)
- `src/clients/jellyfin-web/manifest.json`, `README.md`: author/repo links
  updated to TIGamingTV

**Deliberately left unchanged**: `ft_abi` in `infra/just/common.just` (used
to auto-download a dev-testing dependency, the FileTransformation plugin) —
could not verify whether a `10.11.11`-tagged release asset exists for it.
Worth checking manually before relying on `just ft`.

---

## Round 2 — CI/CD "Publish" workflow doesn't trigger

`.github/workflows/publish.yml` only fires on:
```yaml
on:
  push:
    branches: [main]
    paths:
      - 'src/server/**'
  release:
    types: [published]
```
Commits that don't touch `src/server/**` and aren't a GitHub Release never
trigger it — this is expected, not a bug. CI (a separate workflow, no path
filter) still runs on every push.

**Recommended addition** (not yet applied): add `workflow_dispatch:` under
`on:` to get a manual "Run workflow" button in the Actions tab.

**To trigger a full build**: publish a GitHub Release (tag + Publish release)
— this builds the Docker image, the plugin, zips it, attaches it to the
release, and updates the plugin manifest, all in one action.

---

## Round 3 — Docker push denied

Error: `denied: permission_denied: write_package`.

**Cause**: repo's default `GITHUB_TOKEN` permissions were read-only at the
repo level (Settings), which silently overrides any `permissions: packages:
write` declared inside the workflow file itself — workflow permissions can
only narrow, never expand, what the repo settings allow.

**Fix**: repo → Settings → Actions → General → Workflow permissions → set to
**"Read and write permissions"**, then re-run the workflow.

Other possible (less likely) causes if that doesn't fix it: a
pre-existing GHCR package not linked to Actions access for this repo (check
package settings → Manage Actions access), or accidental uppercase in the
image path (GHCR requires all-lowercase).

---

## Round 4 — GitHub Pages 404 on plugin manifest

`https://tigamingtv.github.io/OpenWatchParty/jellyfin-plugin-repo/manifest.json`
returned 404 when Jellyfin tried to fetch it as a plugin repository source.

**Cause**: GitHub Pages either wasn't enabled, or wasn't set to deploy via
GitHub Actions (the `docs.yml` workflow uses `actions/deploy-pages`, which
requires repo → Settings → Pages → Source: **GitHub Actions**, not "Deploy
from a branch").

**Resolution**: eventually confirmed the manifest URL started serving JSON
correctly after enabling Pages with the correct source. If this regresses,
re-check that setting and confirm the "Deploy Documentation" workflow run is
green in the Actions tab.

---

## Round 5 — Manual plugin install (workaround while Pages was broken)

Documented for reference: extract the plugin release zip, create a folder
named `OpenWatchParty` directly inside Jellyfin's `plugins/` directory
(location varies by install type — Docker: `/config/plugins/OpenWatchParty`
inside the container), copy all extracted files (DLLs + `Web/` folder)
directly into it (not nested), restart Jellyfin. This bypasses
auto-updates — the plugin-repository method should be preferred once Pages
works.

---

## Round 6 — Plugin buttons never appeared (root cause found & fixed)

**Symptom**: plugin loaded, script tag was present in page source
(`ClientScript` endpoint returned content), but no header/OSD buttons ever
rendered. `document.getElementById('owp-global-btn')` returned `null`.

**Root cause**: `.github/workflows/publish.yml` had:
```yaml
- name: Copy JS files to plugin Web directory
  working-directory: src/plugins/jellyfin
  run: |
    mkdir -p OpenWatchParty/Web
    cp ../../clients/jellyfin-web/*.js OpenWatchParty/Web/
```
`*.js` is **not recursive** — it only copied the 2 files sitting directly in
that folder (`plugin.js`, `state.js`). Everything in subfolders (`ui/`,
`app/`, `ws/`, `playback/`, `chat/` — i.e. all the actual button/sync logic,
~25 files) never got embedded into the plugin DLL. `plugin.js` (a loader)
fetched those missing files at runtime, got 404s, its promise chain rejected,
and `init()` (which creates the buttons) never ran.

The authoritative file list lives in `infra/just/common.just` as
`client_js_files` — the local `just build plugin` command already copies
correctly; **only the GitHub Actions release path had this bug**.

**Fix applied** — replaced that step with:
```yaml
      - name: Copy JS files to plugin Web directory
        working-directory: src/plugins/jellyfin
        run: |
          mkdir -p OpenWatchParty/Web
          cp ../../clients/jellyfin-web/plugin.js OpenWatchParty/Web/plugin.js
          for f in state.js utils/time.js utils/video.js utils/misc.js utils/media.js utils/log.js ui/styles.js ui/indicators.js ui/toasts.js ui/cards.js ui/home.js ui/render.js playback/play.js playback/bind.js playback/sync.js chat/messages.js chat/input.js ws/send.js ws/auth.js ws/handlers/room.js ws/handlers/sync.js ws/handlers/playback.js ws/handlers/clock.js ws/connection.js app/lifecycle.js app/cleanup.js; do
            mkdir -p "OpenWatchParty/Web/$(dirname "$f")"
            cp "../../clients/jellyfin-web/$f" "OpenWatchParty/Web/$f"
          done
```
Confirmed applied and working — user confirmed the plugin buttons render
after a fresh release built with this fix.

---

## Round 7 — Icon collision with native SyncPlay

**Symptom**: tapping the "group" icon in the header started Jellyfin's own
native SyncPlay instead of OpenWatchParty.

**Root cause**: both OpenWatchParty's header button and OSD button
(`src/clients/jellyfin-web/ui/render.js`) use the exact same Material Icon
glyph (`groups`) that Jellyfin's own native SyncPlay button uses in the same
general header area — visually indistinguishable.

**Recommended fix (NOT YET CONFIRMED APPLIED)**:
- In `ui/render.js`, both occurrences of:
  ```js
  btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
  ```
  → change to a distinct icon, e.g. `live_tv`, `connected_tv`, or `theaters`.
- Also change button position to avoid sitting adjacent to native SyncPlay:
  ```js
  headerRight.prepend(btn);   // was: puts button at the START of header icons
  ```
  → change to:
  ```js
  headerRight.append(btn);    // puts it at the END, away from native SyncPlay
  ```

**Status**: recommended but not confirmed as applied by the user. Verify
current state of `ui/render.js` before assuming this is done.

---

## Round 8 — Reverse proxy / WebSocket connectivity (resolved)

Multiple stacked issues, resolved in sequence:

1. Plugin's "Session Server URL" config field is used **literally** — the
   C# backend passes it straight through to the browser's
   `new WebSocket(...)` call. A URL starting with `http://` is invalid for
   that API.
2. Mixed content: since Jellyfin is served over HTTPS, a plain `ws://`
   WebSocket gets blocked by the browser outright — needs `wss://`.
3. The configured URL was a private LAN IP (`192.168.178.109:3238`), which
   is unreachable from outside the home network — needed to be reachable via
   the same public domain nginx already serves Jellyfin on.

**Fix**: added an nginx location block reverse-proxying WebSocket traffic
through the same TLS-terminated domain:
```nginx
location /ws {
    proxy_pass http://192.168.178.109:3238;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_read_timeout 86400;
}
```
Then configured:
- Plugin config, Session Server URL: `wss://jellyfin.homeserver-tom-tykwer.dedyn.io/ws`
- Session server's `ALLOWED_ORIGINS` env var: `https://jellyfin.homeserver-tom-tykwer.dedyn.io`

**Clarified during this round**: the Jellyfin plugin's C# backend has zero
network calls to the session server anywhere in its code — it only ever
echoes the configured URL string back to the browser via the
`/OpenWatchParty/Token` response. The only real network hop that matters is
browser ↔ session server. This means the specific host port the container
uses (`3238` vs. the code's default of `3000`) is a pure implementation
detail for nginx's `proxy_pass` target — there is no separate "plugin talks
to port 3000" assumption anywhere to break.

**Also flagged**: if a router port-forward for `3238` was set up earlier
(to reach the LAN IP directly, bypassing nginx), it should be removed now
that the nginx `/ws` path works — leaving it open exposes the session server
over plain unencrypted `ws://` directly to the internet.

**Status**: confirmed working by the user ("now shows as online").

---

## Round 9 — Manifest version-string crash (side-tracked, not fully resolved)

Jellyfin log showed:
```
System.ArgumentException: Version string portion was too short or too long. (Parameter 'input')
   at Emby.Server.Implementations.Updates.InstallationManager.GetPackages(...)
```
This happens when a plugin-repository manifest's `version` field isn't
exactly N.N(.N)(.N) numeric-only (`System.Version.Parse`'s hard requirement)
— e.g. a stray `v` prefix or non-numeric suffix from a malformed release tag.

Investigated but not conclusively resolved which specific repository URL/
manifest entry caused it (the primary Pages-hosted manifest had
`versions: []` at the time, which can't have caused a parse error — the
crash likely came from a different repository URL entry added earlier during
troubleshooting, e.g. a raw.githubusercontent.com fallback with a real,
malformed version string in it).

**Recommended fix (NOT YET APPLIED)** — add a guard in
`.github/workflows/publish.yml` right after the tag's `v` prefix is
stripped:
```yaml
          VERSION="${VERSION#v}"  # Remove 'v' prefix if present

          if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+(\.[0-9]+){0,2}$ ]]; then
            echo "::error::Release tag '$VERSION' is not a valid numeric version (expected e.g. 1.0.1). Aborting manifest update."
            exit 1
          fi
```

**Status**: user moved on to manual plugin installs instead of chasing this
further. Worth revisiting: check Dashboard → Plugins → Repositories for any
extra/stale repository URLs beyond the GitHub Pages one, and check what
`version` string each one's manifest actually contains.

---

## Round 10 — Rooms died on any disconnect (root cause found & fixed, UNTESTED)

**Symptom**: two accounts joined a watch party; sync silently broke mid-
session. Server logs showed one client's original room disappearing
(`room_list` reporting 0 rooms) and the client being auto-attached to a
brand-new room ID as a non-host guest, with a hard seek backward.

**Root cause**, confirmed by reading `src/server/src/ws/connection.rs`:
```rust
let temp_id = uuid::Uuid::new_v4().to_string();
```
Every single WebSocket connection — including reconnects from the exact same
browser tab — got a **brand-new random UUID**. There was no session/identity
persistence at all. Combined with a 60-second "zombie" cleanup interval
(`src/server/src/tasks.rs`) and browsers throttling JS timers in backgrounded
tabs (which can delay ping intervals past that 60s window), any brief
disconnect — WiFi blip, tab backgrounded, laptop sleep — caused: instant
room teardown server-side, then a reconnect that looked like a total
stranger with zero memory of previously being host.

### Fix implemented (server + client, NOT YET COMPILED OR TESTED)

**New file**: `src/server/src/room/reconnect.rs`
- `schedule_disconnect(client_id, clients, rooms)` — instead of tearing a
  room down immediately on disconnect, waits a 20-second grace period
  (`RECONNECT_GRACE_SECS`) and only then checks whether the same client_id
  reconnected (detected via `mpsc::Sender::same_channel` identity
  comparison against a captured "stale" sender). If it reconnected, does
  nothing; if not, calls the original `handle_disconnect`.
- `resend_room_state(client_id, room_id, clients, rooms)` — re-sends a
  `room_state` message (same payload shape as the normal join flow) when a
  client successfully reattaches to a room it already belonged to, so the
  client's existing `room_state` handler naturally restores host/guest role
  without any special-casing.

**Modified**: `src/server/src/room/mod.rs` — registered/exported the new
module (`resend_room_state`, `schedule_disconnect`).

**Modified**: `src/server/src/ws/connection.rs` — `client_connection` now
takes an additional `requested_client_id: Option<String>` parameter. Logic:
- Validates the client-supplied ID looks like a plausible UUID (36 chars,
  hex + hyphens) via `is_plausible_client_id`; otherwise mints a fresh
  server-side UUID as before.
- If that ID already exists in the `clients` map (a reconnect within the
  grace period, or a stale entry the zombie reaper hasn't gotten to), it
  **reattaches**: swaps in the new mpsc sender, updates `last_seen`, and
  reads back the existing `room_id` — instead of registering as a brand-new
  stranger.
- If a room was rejoined, calls `resend_room_state` after the usual
  `client_hello` / `room_list` messages.
- On disconnect, calls `schedule_disconnect` (grace period) instead of the
  old immediate `handle_disconnect`.

**Modified**: `src/server/src/routes.rs` — added a `client_id_filter`
(`warp::query::<HashMap<String,String>>()` reading a `client_id` query
param), threaded through `build_ws_route` and into
`crate::ws::client_connection(...)`'s new parameter.

**Modified**: `src/server/src/tasks.rs` — the zombie-timeout cleanup loop
now calls `crate::room::schedule_disconnect(id, clients.clone(),
rooms.clone())` instead of `crate::room::handle_disconnect(&id, &clients,
&rooms)` directly, so a zombie-reaped client gets the same reconnect grace
window as a normal disconnect.

**Modified**: `src/clients/jellyfin-web/ws/connection.js` — added:
- `generateUuid()` — uses `crypto.randomUUID()` where available, with a
  manual fallback for non-secure contexts.
- `getPersistentClientId()` — reads/writes a UUID to
  `localStorage['owp_persistent_client_id']`; falls back to an in-memory
  `state.sessionOnlyClientId` if localStorage throws (private browsing
  etc.) — reconnects still benefit within the same page load in that case,
  just not across a full page refresh.
- `withClientId(baseUrl, clientId)` — appends `?client_id=...` or
  `&client_id=...` depending on whether the base URL already has a query
  string.
- In `connect()`: the WebSocket is now opened against
  `withClientId(wsUrl, getPersistentClientId())` instead of the raw
  `wsUrl`.

**Delivery method**: since the user has no local clone, all 6 files (1 new,
5 modified) were delivered as full-file replacement content to paste via
GitHub's web editor, plus a `.patch` file (`openwatchparty-reconnect-fix.patch`)
for reference/local application if a clone exists.

**Status**: **implemented but not compiled, not deployed, not tested.**
This is Priority #1 for this new project — get a local Rust toolchain (or
CI) actually compiling this before trusting any of it. Pay particular
attention to:
- Whether `mpsc::Sender::same_channel` exists in whatever tokio version is
  pinned (`Cargo.toml` had `tokio = { version = "1", ... }` — should be
  fine, but verify)
- Whether the borrow-checker is happy with the `rejoined_room_id` capture
  pattern in `connection.rs` (written carefully to drop the write lock
  before the later read-lock calls, but untested)
- The 20-second `RECONNECT_GRACE_SECS` constant is a guess — may need
  tuning based on real-world reconnect latency observed once deployed

---

## Round 11 — IN PROGRESS: official/native Jellyfin clients show false "In sync"

**Symptom reported by user**: connecting via browser-to-browser now works
correctly (after Round 10's fix — user confirmed "its now working, but only
with browser to browser"). Using "the official jellyfin client" shows a sync
status indicator claiming things are synced, but playback is not actually
syncing.

**Root cause found and PARTIALLY fixed this session**:
`src/clients/jellyfin-web/state.js` initialized:
```js
syncStatus: 'synced',  // 'synced' | 'syncing' | 'pending_play' - for UX indicator (UX-P3)
```
This is a **false default** — `'synced'` was the starting value before any
real drift-check comparison had ever run
(`src/clients/jellyfin-web/playback/sync.js`'s drift loop only ever
*overwrites* `syncStatus`, it never initializes it). So on any client where
that drift-check loop never actually engages — e.g. because
`utils.getVideo()` can't find/attach to whatever video element that
client's player actually uses — the UI shows "In sync" from the very first
frame, permanently, with zero indication anything is actually broken.

**Fix applied in this session (uncommitted anywhere yet — exists only in
the chat, needs to be recreated/reapplied in this new project)**:
- `state.js`: `syncStatus: 'synced'` → `syncStatus: 'unknown'`
- `ui/indicators.js`: rewritten with a `describeSyncStatus(status)` helper.
  `'unknown'` (and any other unrecognized value) now renders as a distinct
  **"Not synced yet"** state (new `unknown` dot class) instead of being
  silently treated the same as a confirmed `'synced'` status.

**Status: NOT YET packaged as a deliverable, NOT tested, NOT deployed.**
This is the immediate next step in this project.

**Root cause of the underlying sync failure is still UNCONFIRMED.** Leading
hypothesis, not yet verified: "official Jellyfin client" likely means either
(a) the Jellyfin Media Player desktop app (Electron/Qt wrapper — should
still load the same jellyfin-web frontend and thus the same injected script
and `<video>` element, so should theoretically work) or (b) official mobile
apps or a native player mode within one of these that bypasses the standard
HTML5 `<video>` element the injected script expects
(`src/clients/jellyfin-web/utils/video.js`'s `getVideo()`, and
`playback/bind.js`'s event binding) — in which case the injected-JS
architecture fundamentally cannot reach it, same limitation already
established for Neptune/Fladder/native TV apps earlier in the conversation.

### Next steps for this thread specifically
1. Clarify with the user exactly which "official Jellyfin client" they
   mean — Jellyfin Media Player desktop app? iOS/Android official Jellyfin
   app? Or Jellyfin Web itself in a specific browser?
2. Confirm whether `ScriptInjectionMiddleware` (server-side, injects the
   `<script>` tag into `index.html`) even executes for that client's
   request path — some clients may request a different bundle route, or
   may cache an old `index.html` without the injection.
3. If the script does load: instrument/log inside `utils/video.js`'s
   `getVideo()` and `playback/bind.js`'s binding logic to see if a `<video>`
   element is actually found and events actually fire on that client.
4. Package and deliver the `state.js` + `ui/indicators.js` fix from this
   round regardless, since it's a real correctness fix independent of the
   deeper investigation.

---

## Round 12 — IN PROGRESS (Claude Code): official desktop client still doesn't sync

**Confirmed by user**: Round 10's reconnect fix works — browser-to-browser
sync is solid now. Round 11 remains open: the official desktop client
(Jellyfin Media Player) still doesn't sync, in either host or guest role.

**Found**: Round 11's `state.js`/`ui/indicators.js` fix was never actually
applied to this repo (the "not yet packaged/delivered" note in Round 11 was
accurate — `state.js` still had `syncStatus: 'synced'` as the default).
**Applied for real this round**:
- `state.js`: `syncStatus` default → `'unknown'`
- `ui/indicators.js`: extracted a shared `describeSyncStatus(status)` helper
  (also removes duplicated dot/label logic between `updateSyncIndicator` and
  `buildSyncStatusIndicator`); unrecognized/`'unknown'` status now renders as
  a distinct "Not synced yet" gray dot instead of falsely "In sync".
- `ui/styles.js`: added `.owp-sync-dot.unknown` style (gray, `#9e9e9e`).

**Diagnostic finding this round**: user confirmed buttons/panel/room-join
*do* work on the desktop client (injected script loads and runs fine — rules
out `ScriptInjectionMiddleware` path-matching as the cause). User tried the
desktop client as both host and guest — sync failed both ways. Initial
hypothesis was Round 11's (b) — no real `<video>` element (native mpv-based
player bypassing HTML5 video) — **but a server-log capture during a repro
disproved this**: client `f08eaef1` (the desktop client, user "tom") showed
a genuine `<video>` element loading correctly
(`[CLIENT:...:VIDEO] event=buffering readyState=0` →
`event=ready readyState=4`), so `utils/video.js`'s `getVideo()` and event
binding are NOT the problem on this client.

**Actual root cause found**: reading the full log timeline for `f08eaef1`:
- `18:40:51` — last successful ping (already on the 30s "stable" cadence
  per `ws/connection.js`'s `schedulePing`, `samples=8`)
- then **~2 minutes of total silence** — no more pings at all, meaning its
  ping `setInterval` (or the whole JS engine) was suspended
- `18:42:56` — server's zombie reaper (`ZOMBIE_TIMEOUT_MS = 60_000` in
  `src/server/src/tasks.rs`) finally notices and starts the reconnect grace
  period (`schedule_disconnect` in `src/server/src/room/reconnect.rs`)
- `18:43:16` — grace period (`RECONNECT_GRACE_SECS`, was `20`) expires,
  server disconnects it
- `18:43:18` — client reconnects — **2 seconds too late**, treated as a
  brand-new stranger instead of reattaching to its room (Round 10's fix
  requires reconnecting *within* the grace window)

**Conclusion**: this is NOT a `<video>`-binding / injected-JS-architecture
ceiling at all. Jellyfin Media Player's embedded QtWebEngine appears to
suspend JS timers far more aggressively than a normal browser tab (a
multi-minute stall, not seconds), so the ping heartbeat dies, the connection
gets zombie-reaped, and by the time the JS engine wakes up and reconnects it
narrowly misses the grace window — falling out of the room entirely before
any sync logic downstream gets a chance to run. Same underlying class of
issue as Round 10, just with a much longer stall duration than that fix's
20s grace period accounted for.

**Fix applied this round**: `src/server/src/room/reconnect.rs` —
`RECONNECT_GRACE_SECS` `20` → `90`, to comfortably cover this client's
observed suspension length. **Not yet compiled, deployed, or retested.**
Tradeoff worth knowing: a client that's truly gone (crash, force-quit) now
takes up to 90s longer to be cleaned out of the room (stale participant
count, room briefly "held open") — acceptable for small-group home use, but
worth keeping in mind if it ever matters.

**Next step**: package/deploy the reconnect.rs change (plus the `state.js`/
`ui/indicators.js`/`ui/styles.js` sync-status fix from earlier this round)
and retest on the desktop client. If sync still fails even after surviving
the reconnect, revisit the `<video>`-element angle — but this specific log
capture shows a working video element and a connectivity/timer-throttling
problem as the actual cause, not the injected-JS ceiling discussed earlier
in the project.

---

## Round 13 — CONFIRMED: desktop client has no reachable `<video>` element (Round 12's conclusion corrected)

Both `RECONNECT_GRACE_SECS: 90` and the `syncStatus: 'unknown'` fix were
deployed (committed as `6aacfec` and `b3dc8ef`; a manifest version-guard fix
for Round 9 also landed as `7e1e183`). Retested on the desktop client:
**still doesn't sync**, and this time the user confirmed precisely what's
missing: connect/identify/`CLOCK` ping lines still show up fine, but
**zero** `[CLIENT:...:CLIENT]`, `[CLIENT:...:VIDEO]`, or
`[CLIENT:...:HOST]` lines ever appear — even though the desktop client
visibly showed as joined (participant count, chat panel) and video was
visibly playing on screen.

**Round 12's "confirmed working video element" conclusion was based on a
misattributed client ID.** Re-reading that same log capture: the
`buffering`→`ready` `VIDEO` events actually came from `ec5cb37c`
("tom_test"), a *different* client — not `f08eaef1` ("tom", the desktop
client, confirmed by the user both times). `f08eaef1` never produced a
single `CLIENT`/`VIDEO`/`HOST` line in *either* test capture, only `CLOCK`
pings. The connectivity/reconnect-timing bug Round 12 found and fixed was
real and worth keeping, but it was never the reason the desktop client
doesn't sync.

**Root cause, now confirmed by direct evidence rather than speculation**:
`ws/handlers/sync.js`'s `syncToRoom()` (line 32:
`if (!video || state.isHost || !msg.payload?.state) return;`) and
`ws/handlers/playback.js`'s `handlePlayerEvent()` (line 77:
`if (state.isHost || !video) return;`) both bail out **before** reaching
their `utils.log('CLIENT', ...)` calls whenever `video` is falsy. `video` is
computed once per incoming message via `utils/video.js`'s `getVideo()`
(`document.querySelector('video')`) at the top of `connection.js`'s
`handleMessage()`. Meanwhile `applyRoomState()` and `ui.render()` (which
don't touch `video` at all) still run fine, explaining why the room-join
UI/chat/participant-count works perfectly while video sync logic silently
no-ops with no log trace whatsoever.

**Conclusion**: `document.querySelector('video')` returns `null` in Jellyfin
Media Player's rendering context despite real video being visibly on
screen — i.e. whatever renders that picture is not a standard DOM `<video>`
element reachable from the page's own document. This matches Round 11's
original hypothesis (b): Jellyfin Media Player (Qt/QtWebEngine-based) most
likely renders actual playback through a native layer (commonly
libmpv-based for broader format/hardware-acceleration support) instead of
the browser's HTML5 video pipeline. This is the **same architectural
ceiling already documented for Neptune/Swiftfin/Fladder/native TV apps** in
`CLAUDE.md`'s "Known constraint" section — just now shown to also apply to
the official desktop client, not only fully-native mobile/TV apps.

**Not yet done — cheapest possible check before treating this as fully
architectural**: Jellyfin Media Player may have a settings toggle for its
player backend (something like "native/mpv player" vs. falling back to the
built-in web player). If such a setting exists and disabling it forces JMP
to use the standard HTML5 `<video>`-based web player, this whole problem
disappears with zero code changes. Worth checking Settings → Player (or
similar) in JMP before investing in Round 13.5 below. If confirmed via web
inspector (`document.querySelector('video')` from JMP's console, if
"Enable Web Inspector" is available in its settings) that no `<video>`
element exists regardless, the native-player theory is airtight.

**If no such toggle exists / doesn't help**, the two real paths forward
(discussed but not started, see "Cross-cutting architectural note" below):
1. Document Jellyfin Media Player as unsupported for injected-JS sync
   (same as Neptune/Swiftfin), and point users at Jellyfin Web in a regular
   browser instead, if acceptable.
2. Build the alternative server-side architecture: poll Jellyfin's
   `/Sessions` API and drive `Pause`/`Unpause`/`Seek` via its core
   remote-control API — reaches any client Jellyfin itself can remote-control
   (including native players), at the cost of coarser sync (hard seeks only,
   no smooth playback-rate drift correction) and no chat/self-service room
   UI (becomes more of an admin tool). Requires
   `EnableRemoteControlOfOtherUsers` permission. Not yet started; first
   suggested validation step (manually testing whether a remote Pause via
   Jellyfin's Dashboard → Active Devices actually affects Jellyfin Media
   Player's native player) has never been confirmed done.

---

## Round 14 — Native mpv-player adapter built for jellyfin-desktop/JMP (UNTESTED)

Researched (via web search/fetch of upstream `jellyfin/jellyfin-desktop`
source, since the old Qt-based Jellyfin Media Player is archived/deprecated
in favor of this CEF+mpv rewrite) exactly how that client avoids a DOM
`<video>` element: its player plugin
(`src/web/mpv-video-player.js`, extending `mpv-player-base.js`) registers
itself as `window._mpvVideoPlayerInstance` and routes all real playback
through a native bridge (`window.api.player.*`) instead of ever creating a
`<video>` tag. Confirmed no user-facing setting exists to disable this and
fall back to HTML5 video — native mpv playback is the entire reason this
client exists, not an optional mode.

**`mpv-player-base.js`'s API surface** (per upstream source, not locally
tested): `currentTime(val)` — getter/setter, **milliseconds not seconds**;
`paused()` — a method, not a property; `setPlaybackRate(value)` /
`getPlaybackRate()`; `pause()` / `resume()` / `unpause()`. Events fired via
an internal `this.events.trigger(...)`: `'playing'`, `'pause'`, `'unpause'`,
`'stopped'`, `'volumechange'`, `'error'` — **no confirmed equivalent to
HTML5's `'waiting'`/buffering event**, and the exact subscription mechanism
for those events wasn't confirmed from source alone.

**Fix implemented**: rather than guess at the event-subscription API,
`src/clients/jellyfin-web/utils/video.js` was rewritten to add a
polling-based adapter (`NATIVE_POLL_MS = 250`) that wraps
`window._mpvVideoPlayerInstance` behind an `HTMLMediaElement`-shaped object
(`currentTime`/`paused`/`playbackRate`/`readyState`/`networkState`/`seeking`
getters+setters, `play()`/`pause()` methods, `addEventListener`/
`removeEventListener`) so `playback/bind.js` and `playback/sync.js` work
completely unmodified — they can't tell the difference between a real
`<video>` and this adapter. `getVideo()` now: returns a real
`document.querySelector('video')` if one exists (browsers, and presumably
any client that does use HTML5 video); otherwise, if
`window._mpvVideoPlayerInstance` exists and exposes `currentTime`/`paused`
as functions, lazily builds and caches a singleton adapter around it
(rebuilding if the instance reference changes, e.g. on next-episode
autoplay); otherwise returns `null` as before. The adapter detects
play/pause transitions and seeks (jump between polls larger than 1s vs.
naturally-elapsed playback time) itself, since it can't rely on unconfirmed
native events for these.

**Known limitations of this fix, to watch for once tested**:
- No real buffering signal exists from `mpv-player-base.js`'s confirmed API,
  so `readyState`/`networkState` are coarse approximations (jumps straight
  to "ready" on first successful poll) — real mid-playback buffering stalls
  on this client won't be detected or reflected in the sync status.
- Seek detection is heuristic (position jumped >1s more than expected
  elapsed time) rather than a real `'seeked'` event, so a genuine 1+ second
  network hitch could be misread as a manual seek, or a true sub-second seek
  could be missed.
- `app/lifecycle.js`'s UI-detection interval (which calls `bindVideo()`)
  gates entirely on `document.visibilityState === 'visible'`
  (`startIntervals()`, `UI_CHECK_MS` interval). Not verified whether CEF
  reports `'visible'` correctly while jellyfin-desktop is actively playing —
  if it doesn't, this adapter would never even get invoked regardless of how
  correct it is. Worth checking if the fix still doesn't engage after
  deploying.
- Everything here is inferred from reading upstream `jellyfin-desktop`
  source via web search/fetch, not from a live REPL/console session against
  a real running instance — the base method names/signatures could be
  stale, version-specific, or subtly misread by the fetch summarization.
  **Treat this fix as a first attempt requiring real testing, not a
  confirmed correct implementation.**

**Deliberately avoided creating a new file** for this adapter (folded into
the existing `utils/video.js`) specifically to sidestep Round 6's exact
failure mode — a new client file silently missing from one of the three
places that need to know about it (`plugin.js`'s `loadAll()` script list,
`infra/just/common.just`'s `client_js_files`, and
`.github/workflows/publish.yml`'s copy step). No changes needed to any of
those three for this round.

**Status**: written, not compiled/deployed/tested. Next step: package,
deploy, retest on the desktop client, and check whether
`[CLIENT:...:VIDEO]`/`[CLIENT:...:HOST]`/`[CLIENT:...:SYNC]` log lines now
appear for it. If they still don't appear at all, check the
`document.visibilityState` gate above first before assuming the adapter
itself is wrong.

---

## Cross-cutting architectural note (from mid-conversation)

Discussed but not implemented: a fundamentally different, **server-side**
approach for reaching native clients (Neptune, Fladder, etc.) that doesn't
rely on injected JavaScript at all — using Jellyfin's core session
remote-control API (`GET /Sessions`, `POST /Sessions/{Id}/Playing/Pause`,
`.../Unpause`, `.../Seek`) to have a small background service poll active
sessions and nudge two or more into alignment. Tradeoffs noted: coarser sync
(no smooth playback-rate drift correction, only hard Pause/Seek jumps), no
chat/room self-service UI (becomes an admin tool), and permission
requirements (`EnableRemoteControlOfOtherUsers`). The suggested first step —
manually testing whether Neptune/Fladder even honor a remote Pause command
via Jellyfin's Dashboard → Active Devices UI — was never confirmed as done.
Worth revisiting if broadening client support becomes a priority again.

**Superseded by Round 15** — the manual test above turned out to be
unnecessary; source inspection proved Fladder can't receive Jellyfin's
remote-control commands at all (no delivery channel exists client-side).
See Round 15 for the actual findings and the current recommended path.

---

## Round 12 — Develop-first branching + a real develop build for both components

The user asked for a proper `develop`-before-`main` workflow, with a full
"develop" build/image for **both** the session server and the Jellyfin
plugin (previously only the session server got a rolling develop build; the
plugin had no develop-channel equivalent at all).

**Found while investigating (all fixed):**

- `.github/workflows/ci.yml` triggered on branches `[main, dev]`, but the
  actual branch is `develop` — CI silently never ran on pushes/PRs against
  it. Fixed to `[main, develop]`.
- `ci.yml`'s `.NET Tests` job only copied top-level `*.js` files into
  `OpenWatchParty/Web/`, missing every subdirectory module (`utils/`, `ui/`,
  `playback/`, `chat/`, `ws/`, `app/`). Since the `.csproj` embeds
  `Web\**\*.js` as resources, test builds were silently missing most of the
  client JS. Fixed to use the same explicit file list as the release build
  job (not `cp -r`, to avoid embedding `tests/*.test.js` as plugin
  resources).
- `docs/development/ci.md` and `release.md` had drifted from what the
  workflows actually do (wrong branch name, missing `dev` tag, `cp -r`
  documented but not implemented). Rewritten to match reality.

**Added:**

- `publish.yml` now starts with a `changes` job (`dorny/paths-filter`)
  that detects whether `src/server/**` or the plugin/client
  (`src/plugins/jellyfin/**`, `src/clients/jellyfin-web/**`) changed, so
  each push only rebuilds what's relevant. The Docker build job is now
  gated on `needs.changes.outputs.server == 'true' || release event`
  (previously the entire workflow was gated on a `src/server/**` path
  filter, which is why the plugin never had a develop build path at all).
- New `build-plugin-dev` job: on push to `develop` with plugin/client
  changes, builds the plugin, versions it `0.0.<run_number>` (always valid
  for `System.Version`, always below any real `1.x` release), zips it, and
  publishes/overwrites it on a rolling pre-release tagged `develop-latest`
  via `softprops/action-gh-release` (confirmed via its docs: assets are
  overwritten by default when the tag already exists — this is the
  standard rolling/nightly-release pattern, not verified by actually
  running it yet).
- New `update-dev-manifest` job: writes the new version into
  `docs/jellyfin-plugin-repo/manifest-dev.json` (new file, seeded with an
  empty `versions` array) and pushes to `main`, mirroring the existing
  release-time `update-plugin-manifest` job. This is a **second, separate**
  Jellyfin plugin repository URL testers can add
  (`.../jellyfin-plugin-repo/manifest-dev.json`) to get the develop channel
  without touching the stable feed.

**Branch protection on `main` (PR-required, develop-first) — NOT done by
me.** No `gh` CLI and no `GITHUB_TOKEN`/`GH_TOKEN` were available in the
assistant sandbox, so this had to stay a manual step for the user. Note the
important wrinkle if/when it's set up: both `update-plugin-manifest`
(release) and the new `update-dev-manifest` (develop push) jobs push
directly to `main` using the workflow's `GITHUB_TOKEN` — a protection rule
that blocks all direct pushes without an exception for the
`github-actions` bot (or repo admins) will break both of those jobs.

**Status**: none of this has been run for real yet (no push to `develop`
with plugin changes, no release cut since). First real test should be a
small plugin/client-only change pushed to `develop` — confirm the
`build-plugin-dev` and `update-dev-manifest` jobs both fire and skip
correctly (i.e. a server-only change should *not* trigger them, and vice
versa), and that `manifest-dev.json` ends up valid and installable in
Jellyfin.

---

## Round 13 — Dev plugin channel built correctly but never went live

First real test of Round 12: pushed the CI/CD infra work plus the pending
video adapter fix to `develop`. `build-plugin-dev` and `update-dev-manifest`
both fired correctly (and only for the plugin/client change, as intended —
`build`/`merge` correctly skipped since no `src/server/**` changed).
`manifest-dev.json` on `main` ended up with a correct `0.0.10` entry
pointing at the `develop-latest` pre-release. But
`https://tigamingtv.github.io/OpenWatchParty/jellyfin-plugin-repo/manifest-dev.json`
kept serving the empty seed content — nothing showed up in Jellyfin's
catalog after adding the repo.

**Root cause**: pushes authenticated with the workflow's own `GITHUB_TOKEN`
do not trigger other workflows (GitHub's anti-recursion protection) — so
the bot's `git push` to `main` inside `update-dev-manifest` never triggered
`docs.yml`, and Pages never rebuilt. The stable channel has the exact same
latent bug in the pre-existing `update-plugin-manifest` (release) job — it
just hadn't been noticed because a later human-authored commit that also
touched `docs/**` happened to trigger a rebuild that picked up the
already-committed (but never-deployed-on-its-own) manifest change.

**Fix**: both jobs now explicitly call the GitHub REST API
(`POST /repos/{repo}/actions/workflows/docs.yml/dispatches`) right after
their `git push`, using the same `GITHUB_TOKEN` — an explicit
`workflow_dispatch` API call is not subject to the same anti-recursion
suppression as passive push events, so this reliably forces the Pages
rebuild. Required adding `actions: write` to `publish.yml`'s top-level
`permissions`.

**Status**: fix written, not yet observed working (needs another push to
`develop` with a plugin/client change, or a release, to confirm the
dispatch call actually lands and Pages redeploys within a minute or two
afterward). The already-committed `0.0.10` entry on `main` should get
picked up as a side effect the next time *any* commit touching `docs/**`
lands on `main` — did not wait to confirm this manually.

---

## Round 14 — Drift-correction hysteresis (reduce visible playbackRate flicker)

Now that sync itself works, the user flagged a UX issue: `playback/sync.js`
was constantly nudging `video.playbackRate` away from 1x, which looked
noticeable/weird even when barely out of sync.

**Root cause**: the old controller had a single 40ms `DRIFT_DEADZONE_SEC`
threshold, re-evaluated every `syncLoop` tick (`SYNC_LOOP_MS` = 500ms), with
a `sqrt(drift) * DRIFT_GAIN` correction curve. Because the curve is steep
near zero (e.g. 100ms drift → already 1.16x) and there was no gap between
"start correcting" and "stop correcting," the rate was essentially always
slightly off 1x, visibly flickering as drift oscillated across the single
threshold.

**Fix**: replaced the single deadzone with a hysteresis (Schmitt-trigger)
controller — two thresholds instead of one:
- `DRIFT_CORRECTION_ENTER_SEC` (0.3s) — drift must exceed this to *start* a
  correction burst.
- `DRIFT_CORRECTION_EXIT_SEC` (0.1s) — drift must fall back under this
  (tighter) bound to *stop* the burst and snap back to exactly 1x.

Between bursts, playback sits untouched at 1x regardless of small jitter.
New `state.isDriftCorrecting` flag (`state.js`) tracks which side of the
hysteresis band the controller is currently on; it's force-reset to `false`
whenever `syncLoop` exits early (paused, buffering, is-host, not in room,
hard-seek), so a fresh drift event always restarts at the wider ENTER
threshold rather than resuming mid-burst. The existing sqrt/gain correction
curve, hard-seek threshold (`DRIFT_SOFT_MAX_SEC` = 2.0s), and rate clamps
were left unchanged — only the entry/exit gating changed.

Updated `docs/technical/sync.md`, `docs/technical/client.md`, and
`docs/operations/configuration.md` (all referenced the old
`DRIFT_DEADZONE_SEC` constant directly).

**Status**: implemented, not yet tested against a live room — needs a
multi-client watch session to confirm the correction bursts feel less
jittery in practice and that `DRIFT_CORRECTION_ENTER_SEC`/`EXIT_SEC` (0.3s /
0.1s) are reasonably tuned rather than too aggressive or too lax.

---

## Round 15 — Investigated native-client sync for Fladder (Android TV); no build started

The user wants OpenWatchParty to eventually reach clients that don't run
Jellyfin Web/injected JS, starting specifically with **Fladder on Android
TV** (`DonutWare/Fladder`, GPLv3, Flutter, package `nl.jknaapen.fladder`,
leanback/Android-TV support confirmed in its manifest). This round is pure
investigation — no code was written, nothing shipped. Recorded here so a
future session doesn't have to re-derive it.

### Recommended path: wait for Fladder's own SyncPlay PR

**[DonutWare/Fladder#735](https://github.com/DonutWare/Fladder/pull/735)**
is an in-progress, unmerged PR implementing native Jellyfin SyncPlay support
in Fladder itself — it adds a `web_socket_channel` dependency and a full
`lib/providers/websocket/` + `lib/providers/syncplay/` stack. Once merged,
Fladder (including its Android TV build) gets real cross-client sync for
free, with better fidelity than anything we could bolt on externally
(exact seek, no drift-correction hacks). **This is the preferred outcome
— check whether it has merged/released before building any of the below.**
Worth watching the PR / helping test it rather than duplicating the work.

The user has pushed back on relying on Jellyfin's own SyncPlay before
(reason: dissatisfaction with vanilla SyncPlay's UX is *why* OpenWatchParty
exists as a separate plugin) — but that objection was about SyncPlay inside
Jellyfin Web, not about Fladder's native implementation of it, so it's kept
as the top recommendation here rather than discarded.

### Why the originally-discussed "admin remote-control bridge" (see the
cross-cutting note above) does not work for Fladder

Confirmed by cloning `DonutWare/Fladder` and reading source, not by testing
against a live device:

- Fladder has **no WebSocket/SignalR connection to the Jellyfin server** at
  all in the currently released app (`pubspec.yaml` has no such dependency;
  confirmed by grep across `lib/`). It reports its own playback position via
  one-way periodic `POST /Sessions/Playing/Progress`
  (`lib/models/playback/direct_playback_model.dart:101`,
  `transcode_playback_model.dart:100`), and its "control panel" screen
  (`lib/providers/control_panel/control_activity_provider.dart`) only reads
  `GET /Sessions` to display other users' activity — it never listens.
- Jellyfin's remote-control commands (`Pause`/`Unpause`/`Seek`/
  `GeneralCommand`) are delivered by the **server pushing them down that
  same socket** the client is supposed to have open. No socket ⇒ no
  delivery path. This isn't a permissions problem
  (`EnableRemoteControlOfOtherUsers`) — the command has nowhere to land.
- This is *exactly* the gap PR #735 closes (it adds the missing socket,
  specifically to implement SyncPlay).

### Fallback if #735 stalls: Fladder already exposes an OS-level media-session control surface

Found in `lib/wrappers/media_control_wrapper.dart`: `MediaControlsWrapper
extends BaseAudioHandler` (the `audio_service` package), and its
`play()`/`pause()`/`seek(Duration)` overrides (lines 312, 333, 546) drive
the real player and report position to Jellyfin — registered for **video
playback**, not just audio. Concretely, on Android this surfaces as a
standard `MediaSession` for the app (package `nl.jknaapen.fladder`),
controllable by anything holding a `MediaController` for it. Two fallback
designs were discussed, **not built**:

1. **Companion Android TV app** ("OWP Bridge"): a small app installed
   alongside Fladder, granted Notification Listener access (one-time,
   Settings → Apps → Special app access), that resolves a `MediaController`
   for `nl.jknaapen.fladder`, joins an OpenWatchParty room over the existing
   Rust WS protocol like any other client, and translates host events to
   `transportControls.play()/pause()/seekTo(ms)` (and the reverse via
   `MediaController.Callback`). Gives frame-accurate seek and position
   readback — same quality bar as the browser client. Cost: one install +
   one permission grant per TV box.
2. **ADB-only bridge, zero install**: with wireless debugging enabled once
   on the TV (a device setting, not an app or a Fladder change), a
   server-side service can `adb connect` and run
   `adb shell cmd media_session dispatch play|pause` to drive Fladder's
   media session directly (its `MEDIA_BUTTON` receiver is present in
   `AndroidManifest.xml`), and parse `adb shell dumpsys media_session` for
   approximate position. Real limitation: **no absolute-seek command exists
   over ADB** — only stepped fast-forward/rewind nudges — so drift
   correction/mid-episode joins would be coarse skips, not clean jumps, and
   position readback means parsing semi-stable `dumpsys` text rather than a
   real API.
- **True zero-touch (no device-side enablement of any kind) is not
  possible** — confirmed as an Android sandboxing boundary, not a design
  gap: some minimal on-device step (the client opening a socket itself, an
  installed bridge, or an enabled debug interface) is unavoidable for any
  external process to reach another app's running playback session.

### Other Android TV clients checked — none solve this out of the box

- **Official `jellyfin/jellyfin-androidtv`**: SyncPlay has been an open,
  unassigned feature request since August 2020
  ([issue #538](https://github.com/jellyfin/jellyfin-androidtv/issues/538))
  with no PR — effectively stalled. Jellyfin's own SyncPlay only works on
  the web-wrapper-based clients (regular Android, iOS, desktop); Android TV
  was explicitly excluded from that implementation.
- **Findroid**: no evidence found of SyncPlay support either way.

### Addendum: Fladder-as-host works today, with zero Fladder changes and no wait on #735

Realized after the above was written: everything discussed so far assumed
Fladder needed to *receive* something (a command, a bridge, ADB). But
Fladder already *pushes* everything needed for it to act as a room **host**
— it just needs to be observed, not controlled.

Confirmed in `lib/providers/video_player_provider.dart:80-114`:
- `updatePlaying(bool)` calls `model.updatePlaybackPosition(...)`
  **immediately** on every play/pause toggle (event-driven, no polling
  delay).
- Position ticks (`updatePosition`) only forward to Jellyfin once drift
  since the last reported position exceeds 10s — so during steady playback
  Jellyfin's view of Fladder's position refreshes roughly every ~10s, but a
  seek is picked up almost instantly (the jump itself blows past the 10s
  gate on the very next player tick).

All of this lands in Jellyfin core as a normal session/progress update. The
Jellyfin plugin already runs **in-process** inside Jellyfin (see
`ARCHITECTURE.md` §2), so it can subscribe directly to
`ISessionManager.PlaybackProgress`/`PlaybackStart`/`PlaybackStopped` —
no REST polling, no API key, just an event subscription — and get Fladder's
position/pause-state the instant Jellyfin itself receives it.

**Proposed design (not built):**
1. Plugin admin page lets the user pick which active Jellyfin session is
   "the host" for a given OpenWatchParty room (same session-list UI concept
   as the earlier admin-bridge idea).
2. Plugin subscribes to that session's progress events and translates them
   into the *same* `play`/`pause`/`seek` messages the Rust WS server
   already broadcasts to a room — Fladder looks like a normal host to every
   existing guest (browsers, and any future native clients), since it's the
   identical wire protocol on the guest side. No new client-side code
   needed at all.
3. Zero Fladder changes. Not blocked on `DonutWare/Fladder#735`.

**The asymmetry that remains:** this only solves Fladder-as-*host*.
Fladder-as-*guest* (following someone else's host) is still blocked for the
reasons in the main Round 15 writeup above — Fladder has no channel to
receive anything. So the practical shape this unlocks: someone watching via
Fladder can host a room that browser users join and stay synced to, but
Fladder itself can't yet be corrected by another host. Still a real,
shippable win, and doesn't require waiting on anything.

### Status / next action

Nothing built this round. Two independent next steps, either of which can
be picked up first:
1. **Fladder-as-guest**: check whether `DonutWare/Fladder#735` has
   merged/released before building the companion-app or ADB fallback — if
   merged, those fallbacks are probably no longer worth building.
2. **Fladder-as-host**: build the addendum design above (plugin subscribes
   to `ISessionManager` progress events for a chosen session, bridges them
   into an existing OpenWatchParty room as host broadcasts). Independent of
   #735, ships today, zero Fladder changes.

---

## Round 16 — Built Fladder-as-host: `HostBridgeManager`/`SessionHostBridge`, admin-gated

Implemented the Round 15 addendum design (host-only, per that round's own
scoping — Fladder-as-guest is still untouched and still blocked on
`DonutWare/Fladder#735`).

**What was built** (all under
`src/plugins/jellyfin/OpenWatchParty/Services/`):
- `SessionServerAuth.cs` — JWT-minting logic extracted out of
  `OpenWatchPartyController` (was private statics there) so it can be
  reused for a session owner other than the current HTTP caller.
- `SessionHostBridge.cs` — one `ClientWebSocket` per bridged Jellyfin
  session, speaking the exact same `auth`/`create_room`/`player_event`/
  `state_update` protocol a browser host uses (verified by reading
  `src/server/src/ws/handlers/*.rs` — no Rust changes were needed or made).
- `HostBridgeManager.cs` — an `IHostedService` singleton that subscribes to
  `ISessionManager.PlaybackStart/PlaybackProgress/PlaybackStopped` for the
  server's lifetime and owns all active bridges, keyed by Jellyfin session
  id. `PlaybackStopped` auto-tears-down that session's bridge (closing its
  room), mirroring "host disconnected."
- Four new admin-only (`[Authorize(Policy = "RequiresElevation")]`)
  endpoints on `OpenWatchPartyController`: `GET Bridge/Sessions`,
  `GET Bridge/Status`, `POST Bridge/{sessionId}/Start`,
  `POST Bridge/{sessionId}/Stop`.
- A new "Native Client Bridge" section in `Web/configPage.html`: lists
  currently-playing sessions with a "Start bridge" button, and active
  bridges with a "Stop" button. Explicitly reference-only on the viewer
  side per discussion with the user — it does **not** auto-join anyone's
  browser into anything; there is no push channel to browsers and the
  injected JS (`src/clients/jellyfin-web/`) was **not** touched at all.
  Receivers still join the resulting room themselves from their own room
  list, exactly as before.
- `docs/ARCHITECTURE.md` updated: the "zero outbound network calls" claim
  for the plugin backend now carries one named, deliberate exception
  (this bridge), rather than being left contradicted by the code.

**Verified against the actual Jellyfin 10.11.11 SDK, not assumed**: an
initial pass wrote code against event-arg shapes pulled from GitHub's
`master` branch, which turned out to not match the pinned 10.11.11 package
at all (e.g. `PlaybackProgressEventArgs` actually lives in
`MediaBrowser.Controller.Library`, not `.Session`, and its session-back-
reference property is `.Session`, not `.SessionInfo`; `GeneralCommand.Name`
is a closed `GeneralCommandType` enum with no custom/free-form slot, which
is why receiver auto-join was scoped out rather than attempted via that
route). This was caught by actually reflecting the real
`MediaBrowser.Controller.dll`/`MediaBrowser.Model.dll` from the local NuGet
cache with a throwaway console app, not by trusting web search results —
worth repeating that verification step for any future Jellyfin SDK
plugin work in this repo, since GitHub's `master` branch does not
necessarily match whatever version is actually pinned in
`OpenWatchPartyPlugin.csproj`.

**Build/test status**: `dotnet build` on the plugin project is clean (0
warnings, 0 errors). `dotnet test` on `OpenWatchParty.Tests` passes 48/48
(the pre-existing 34 plus 14 new tests covering `SessionHostBridge`'s pure
payload-builders and `HostBridgeManager.GetEligibleSessions` filtering).
Note: this sandbox's installed runtimes only include
`Microsoft.AspNetCore.App` 10.0.8, not the 9.0.x the plugin targets, so
`dotnet test` needs `DOTNET_ROLL_FORWARD=LatestMajor` set to actually run
here — an environment quirk, not a project-file change.

**What is explicitly NOT verified — no live Jellyfin + Fladder environment
was available in this sandbox**:
- Whether a real Fladder session on Android TV actually gets bridged in
  and stays in sync with a browser guest end-to-end.
- Whether the `[Authorize(Policy = "RequiresElevation")]` string actually
  resolves to Jellyfin's real admin-only policy at runtime (it's the
  documented pattern real plugins use, per `PluginsController` in Jellyfin
  core, but this repo has never exercised it before now).
- Whether `SessionInfo.Id`/`UserId` and `BaseItemDto.Id` behave as expected
  against a real running server (correct per SDK reflection, but reflection
  confirms shape, not runtime values).

**Known, accepted limitation carried into this round** (not a bug, a
scoping choice): if a bridge's `ClientWebSocket` connection drops
unexpectedly, there's no reconnect/backoff — it just goes silent until
someone notices and restarts it from the widget. The browser client's
reconnect sophistication (`ws/connection.js`) was deliberately not
replicated here.

### Status / next action

Fladder-as-host is code-complete and unit-tested but **unverified against
a live Jellyfin server**. Before calling this done:
1. Deploy to a real Jellyfin instance, start Fladder (or any client) on a
   library item, use the admin config page to start a bridge, and confirm
   a browser joining the resulting room actually sees synced playback.
2. Confirm a non-admin user genuinely cannot hit
   `/OpenWatchParty/Bridge/*` (verify the `RequiresElevation` policy
   assumption above against a real server, not just by reading Jellyfin's
   own source for precedent).
3. Fladder-as-guest is still fully unaddressed — see Round 15's own
   next-action list, unchanged by this round.

**Superseded by Round 17** — items 1 and 2 above no longer apply: the
picker moved out of the admin config page into the in-player widget, and
the endpoints are intentionally no longer admin-gated. See Round 17.

---

## Round 17 — Moved the host-bridge picker from the admin config page into the in-player widget

The user tried the Round 16 admin config page and couldn't find a natural
place for it in their actual workflow — the request was to move the
picker into the same widget used to create/join a room (the lobby panel
injected into Jellyfin Web), not keep it as a separate admin-dashboard
page. The user also explicitly said it's fine for any user to see this
list (session usernames/devices/now-playing titles are not treated as
private within their deployment), so the admin-only gate was dropped too.

**Changes:**
- `Controllers/OpenWatchPartyController.cs`: all four
  `/OpenWatchParty/Bridge/*` endpoints changed from
  `[Authorize(Policy = "RequiresElevation")]` to plain `[Authorize]` — any
  logged-in Jellyfin user can list sessions and start/stop a bridge now,
  same access level as `/OpenWatchParty/Token`.
- `Web/configPage.html`: the "Native Client Bridge" section added in
  Round 16 was removed entirely (moved, not duplicated).
- `src/clients/jellyfin-web/ui/bridge.js` (new): fetches
  `/OpenWatchParty/Bridge/Sessions` and `/Status` using the user's own
  Jellyfin `ApiClient.accessToken()` (same auth pattern `ws/auth.js`
  already uses for `/OpenWatchParty/Token`), renders "Start"/"Stop" rows,
  and POSTs to `/Bridge/{sessionId}/Start` or `/Stop`.
- `ui/render.js`'s `renderLobby` now has a "Host From Another Device"
  section under the existing "Available Rooms" list and "Create Room"
  button, refreshed on the same triggers as the room list (panel open,
  `render()` fast-path).
- `plugin.js` and `infra/just/common.just`'s `client_js_files` both
  updated to load/copy the new `ui/bridge.js` file — and, since that list
  is (per Round 6's note) duplicated verbatim in three places, also
  updated in `.github/workflows/ci.yml` and twice in
  `.github/workflows/publish.yml`. All five locations verified to now
  list `ui/bridge.js` consistently.

**Verification**: `dotnet build`/`dotnet test` re-run clean (48/48) after
the auth-attribute change. The new JS file was checked with `node --check`
for syntax... except Node.js is not installed in this sandbox, so that
could not actually be run — this is a genuine gap, not a "ran and passed"
claim. `ui/bridge.js` was written to closely mirror the existing
`ui/cards.js`/`ws/auth.js` patterns (same `ApiClient.accessToken()` fetch
idiom, same `.owp-room-item`/`.owp-btn` CSS classes already defined in
`ui/styles.js`) rather than inventing new patterns, to minimize the
chance of a mistake slipping through without being able to execute it.

### Status / next action

Same as Round 16's remaining items, minus the two superseded above:
1. Deploy to a real Jellyfin instance and confirm the widget actually
   renders correctly in Jellyfin Web, the session list populates, and
   starting a bridge produces a joinable room — this has only been
   reviewed by reading the code, never executed in a browser.
2. Fladder-as-guest is still fully unaddressed — unchanged from Round 15.

**Update — item 1 was tried and hit a real bug, now fixed.** The user
deployed the Round 17 build (dev plugin channel, Jellyfin restarted),
opened the widget, saw Fladder's session in "Host From Another Device",
and clicked Start. The session server logged a fresh client connecting and
authenticating (`Client connected... identified as tom`), but then nothing
— no `create_room`. Root cause, found via the session server logs plus
confirming nothing showed in Jellyfin's own logs either: **`GetBridgeableSessions`
and `GetBridgeStatus` returned JSON with PascalCase keys
(`SessionId`, `UserName`, ...), not the camelCase (`sessionId`,
`userName`, ...) `ui/bridge.js` was reading.** Jellyfin's controllers do
**not** auto-camelCase JSON output — the existing `/OpenWatchParty/Token`
endpoint already proved this (it manually spells out `user_id`,
`auth_enabled` as literal anonymous-object keys precisely because there's
no naming-policy conversion to lean on), but the new `BridgeableSessionInfo`/
`BridgeStatus` C# records were returned directly via `Ok(...)`, keeping
their PascalCase property names verbatim in the response. `ui/bridge.js`
read `session.sessionId`, got `undefined`, and passed the literal string
`"undefined"` as the session id — which is exactly what the Rust server's
error (`No active session with id 'undefined'`) reported, and precisely
matches the observed symptom (the WS client connected/authenticated fine,
since that part doesn't depend on this JSON, but `create_room` never
followed because `StartBridgeAsync("undefined")` threw before the bridge
code ever got that far — which also explains why Jellyfin's own logs
showed nothing: the exception was returned as a 400 response body, never
logged).

**Fix**: `OpenWatchPartyController`'s three affected endpoints now project
onto anonymous objects with explicit camelCase keys
(`sessionId`/`userName`/`deviceName`/`client`/`nowPlayingItemName` and
`sessionId`/`userName`/`roomId`/`connected`) at the HTTP boundary, matching
the existing `/Token` endpoint's convention, instead of serializing the
internal DTOs directly. `dotnet build`/`dotnet test` re-verified clean
(48/48) after the fix.

**Confirmed working end-to-end** (2026-07-05): the user's first attempt
after the fix still failed identically — turned out the dev-build update
hadn't actually been picked up yet (still running the pre-fix plugin
version). After properly updating to the rebuilt dev version (`0.0.21`,
published via the rolling `develop` build — `docs/jellyfin-plugin-repo/
manifest-dev.json` on GitHub Pages confirmed this build existed with a
changelog matching the fix commit) and restarting Jellyfin, starting a
bridge on a real Fladder session actually worked. **This is the first
confirmed-working end-to-end test of the whole Fladder-as-host feature**
— Round 16/17's "unverified against a live server" caveat is resolved for
the host-bridge-creation path specifically. Still not separately confirmed:
that a browser guest joining the resulting room actually stays in sync
with Fladder's playback over time (only "room gets created and is joinable"
was directly observed in this exchange).

**Lesson for future work on this plugin**: never assume ASP.NET Core/
Jellyfin auto-converts C# record/property casing for JSON responses.
Either project explicit anonymous objects with literal keys (as done
here and in `/Token`), or write a throwaway request against a real/dev
instance and inspect the actual response body before wiring client JS
against assumed field names. Also: when debugging "the fix didn't work"
reports against a rolling dev-build pipeline, check whether the *new*
build was actually installed (version number, timestamp) before assuming
the fix itself is wrong — plugin updates require an explicit Catalog
update **and** a Jellyfin restart, and are easy to skip without noticing.

**Follow-up**: the "Host From Another Device" list was showing every
active session with something playing, including plain Jellyfin Web
browser tabs and Jellyfin Media Player — both already run the injected
script and can host a room themselves via "Create Room", so listing them
in the bridge picker was just clutter (and mildly confusing, since
bridging one would mean two separate connections — the browser's own OWP
client and the backend bridge — racing to be host of different rooms for
the same underlying session). `HostBridgeManager.GetEligibleSessions()`
now excludes sessions whose `Client` is `"Jellyfin Web"` or `"Jellyfin
Media Player"` (case-insensitive), leaving only genuine native-client
candidates (Fladder, and anything else that isn't one of those two).
`dotnet build`/`dotnet test` clean (52/52, 4 new tests for this filter).

**Follow-up #2** (same day): the exact-match version above still failed
against the real server — the user checked Dashboard → Active Devices and
found the actual `Client` strings are `"Jellyfin Web 10.11.11"` and
`"Jellyfin Desktop 3.0.0-dev"` (Jellyfin Media Player appears to have been
renamed to "Jellyfin Desktop" in newer builds), both with a trailing
version that an exact-string `HashSet.Contains` check will never match.
Switched to a prefix match (`Client.StartsWith(...)`) against
`"Jellyfin Web"` / `"Jellyfin Desktop"` / `"Jellyfin Media Player"`, per
the user's own suggestion to treat the version as a wildcard. `dotnet
build`/`dotnet test` clean (54/54, 2 more tests covering the
version-suffixed strings actually seen in the field). **Confirmed working
against the live deployment** (2026-07-05) — after updating to the
rebuilt dev version and restarting, the Jellyfin Web/Desktop sessions no
longer appear in the "Host From Another Device" list, leaving only
Fladder.
