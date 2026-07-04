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
