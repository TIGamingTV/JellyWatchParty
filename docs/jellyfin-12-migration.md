---
title: Jellyfin 12 Migration
nav_order: 10
---

# Jellyfin 12 Migration

Jellyfin's next release drops the leading `10.` from its version numbers:
`10.11.x` becomes `12.x`. At the time of writing the latest preview is
`v12.0-rc7`.

**Jellyfin 12 is not released yet, so JellyWatchParty does not support it
yet either.** This page records what was found checking the plugin against
the 12.0 release candidates, and describes the build that is ready to ship
the moment 12.0 is stable.

The plugin now **builds one assembly per Jellyfin generation from a single
source tree**: `net9.0` against the `10.11.x` packages, and `net10.0` against
the `12.x` packages, via .NET multi-targeting
(`src/plugins/jellyfin/Directory.Build.props`). CI tests both on every push,
and the release pipeline publishes both as separate `targetAbi` entries in
the same plugin manifest, so a single GitHub release updates both server
generations. Merging this once Jellyfin 12.0 ships is a two-line version
bump, not a rewrite — see "The switch" below.

## What changed in Jellyfin 12

- **.NET 10.** Jellyfin 12 targets `net10.0`, and `Jellyfin.Controller 12.x`
  on NuGet ships `lib/net10.0` only — a `net9.0` project cannot reference it.
- **Legacy authorization is off.** `EnableLegacyAuthorization` defaults to
  `false` for new installs, and a migration
  (`20260531160000_DisableLegacyAuthorization`) force-disables it on upgrade.
- **Version scheme.** `10.11.x` → `12.x`; the first digit is now the major
  version.
- **Plugin guidance for the RC.** Jellyfin's own release notes tell RC testers
  to disable external plugins and reinstall them from the unstable plugin
  repository.

## Findings

Checked by diffing JellyWatchParty against the `v12.0-rc3` sources of
`jellyfin/jellyfin` and `jellyfin/jellyfin-web`, plus the published
`Jellyfin.Controller` package for every RC through `12.0.0-rc7`. Nothing in
the findings below has changed across those RCs — `net10.0` as the target
framework, the API surface, and the dependency versions are all stable from
rc3 onward.

### The server API this plugin uses is unchanged

Every Jellyfin type the plugin touches is byte-identical between `v10.11.11`
and `v12.0-rc7`: `ISessionManager` (its playback events, `Sessions` and
`SendPlaystateCommand`), `PlaybackProgressEventArgs`, `PlaybackStopEventArgs`,
`IPluginServiceRegistrator`, `BasePlugin<T>`, `IHasWebPages`, `PluginPageInfo`,
`BasePluginConfiguration`, `IScheduledTask`, `TaskTriggerInfo`,
`TaskTriggerInfoType`, `PlaystateRequest` and `PlaystateCommand`.

Two removals in Jellyfin 12 do not affect us:
`SessionInfo.NowPlayingQueueFullItems` and
`MediaSourceInfo.PlaybackPositionTicks`.

**So the switch is a retarget, not a rewrite.**

### The web client anchors survive

Against `jellyfin-web` 12.0.0:

- `window.ApiClient` is still assigned (`lib/jellyfin-apiclient/ServerConnections.js`)
  and still exposes `accessToken()`, `serverAddress()`, `appName()`,
  `appVersion()`, `deviceId()` and `deviceName()`.
- The video OSD and item-details pages are still the *legacy* controllers, so
  `.videoOsdBottom .buttons` and `.mainDetailButtons` still exist.
- The home page is React but still renders `#indexPage`
  (`apps/modern/routes/home.tsx`) and `.homeSectionsContainer`.
- Plugin configuration pages are unchanged: the `isPluginpage` handling in
  `components/viewContainer.js` is identical to 10.11, and `emby-input`,
  `emby-button`, `emby-checkbox` and `emby-select` all still exist.

The app folders were renamed (`stable` → `legacy`, `experimental` → `modern`),
so a real smoke test is still required — but nothing the plugin hooks into was
removed.

### Blocker: legacy authentication

This was the one thing that made the plugin silently non-functional on
Jellyfin 12, and it has **already been fixed** (see "What has been prepared").

`ws/auth.js` authenticated to `/JellyWatchParty/Token` using only the
`X-Emby-Token` header. Jellyfin gates that header behind
`EnableLegacyAuthorization`, which Jellyfin 12 disables everywhere. The result
was a 401, so `auth_enabled`, `user_id`, `session_server_url`,
`hide_native_sync_button`, `allow_third_party_host` and
`allow_supported_receiver` were never read, and the entire feature did nothing
but write a `console.warn`.

The token transports Jellyfin accepts unconditionally, on 10.11 and 12 alike,
are the `Authorization: MediaBrowser Token="…"` header and the `?ApiKey=`
query parameter. Note this was already a latent bug on 10.11: any admin who
turned legacy authorization off broke the plugin.

### Blocker: `targetAbi` is a floor, not a match

Jellyfin selects plugin packages with `Version.Parse(targetAbi) <= serverVersion`.
A build declaring `targetAbi 10.11.11.0` is therefore offered to, and installs
cleanly on, a Jellyfin 12 server. There is no mechanism to say "10.11 only", so
publishing a Jellyfin 12 build means adding entries with
`targetAbi 12.0.0.0` — Jellyfin will pick the highest entry the server
satisfies.

### File Transformation has no Jellyfin 12 build

The File Transformation plugin is at 2.5.11.0, `targetAbi 10.11.11.0`. Because
of the floor rule above, Jellyfin 12 users can still install it. If it loads
but does not work, JellyWatchParty's reflection-based probe cannot tell the
difference — see "What has been prepared" for the mitigation.

Re-check <https://www.iamparadox.dev/jellyfin/plugins/manifest.json> for a
`targetAbi 12.x` build before the switch.

### Dependencies

- `Newtonsoft.Json` is referenced with `ExcludeAssets="runtime"`, i.e. the
  plugin uses the server's copy. Jellyfin 12 still ships it (13.0.4, via
  `MediaBrowser.Providers`) and binding is by simple name, so it resolves;
  the plugin's own reference (13.0.3, compile-time only) does not need to
  match exactly.
- `System.IdentityModel.Tokens.Jwt` and `Microsoft.IdentityModel.Tokens`
  (6.35.0) *are* shipped by the plugin, targeting `net10.0` same as `net9.0`.
  Jellyfin 12 has no IdentityModel reference of its own, so there is no
  conflict. 6.x is old for .NET 10 but the JWT API surface this plugin uses
  (`JwtSecurityTokenHandler`, `JwtSecurityToken`, `SigningCredentials`) is
  unchanged in 8.x — bumping is a worthwhile follow-up, not a blocker, so it
  was left alone to keep this change small.

## What has been prepared

The plugin now multi-targets both Jellyfin generations from one source tree
and one test suite. All of the following is already in place and already
covered by CI on every push:

- **`src/plugins/jellyfin/Directory.Build.props` builds both `net9.0` and
  `net10.0`.** Each framework has its own `JellyfinPackageVersion`/
  `JellyfinTargetAbi`: `10.11.11` for `net9.0`, and the latest 12.x release
  candidate for `net10.0` (see the top of this page for which one — bump this
  one property when 12.0 goes stable, see "The switch" below).
- **CI (`ci.yml`) tests both frameworks.** `DOTNET_VERSIONS` installs both
  SDKs side by side, and `dotnet test` runs the whole suite once per
  framework automatically because the test project multi-targets too — no
  Jellyfin 12 server or RC package upgrade needed to catch a regression on
  either generation.
- **The release pipeline (`publish.yml`) publishes both.** `build-plugin` and
  `build-plugin-dev` are matrix jobs, one leg per generation
  (`net9.0`/`10.11.11.0` and `net10.0`/`12.0.0.0`), each producing its own
  zip (`...-jellyfin10.zip` / `...-jellyfin12.zip`). The manifest-update steps
  add one `targetAbi` entry per zip under the same plugin version, so a
  single GitHub release updates both `manifest.json` and `manifest-dev.json`
  with both server generations at once — the dual-ABI shape other plugins in
  the ecosystem use, decided up front instead of deferred.
- **`ws/auth.js` no longer depends on legacy authentication.** It sends
  `Authorization: MediaBrowser …`, built from the accessors `ApiClient`
  exposes, and keeps `X-Emby-Token` alongside it for very old servers.
- **File Transformation is verified, not assumed.** `TransformIndexHtml` now
  records that it was actually called. When the request-level middleware stands
  down for File Transformation and the callback never runs on a rendered
  `200`, the middleware stops deferring from the next request and logs why —
  instead of deferring forever to a plugin that never injects the script while
  the log looks healthy.
- **The plugin logs the Jellyfin version it sees.** On startup it compares the
  running `MediaBrowser.Common` against the one this build was compiled
  against, and warns when the server is a whole major version ahead.

## The switch, when Jellyfin 12.0 is released

Because the dual-target build already exists, going live is a version bump
in one file, not a retarget:

1. In `src/plugins/jellyfin/Directory.Build.props`, in the `net10.0` property
   group, set `JellyfinPackageVersion` to the stable `12.0.0` release (the
   `JellyfinTargetAbi` there, `12.0.0.0`, does not need to change). No source
   changes are expected — the API has been identical across every RC.
2. Run the test suite (`dotnet test`, both frameworks) against the stable
   package to confirm nothing moved between the last RC and GA.
3. **File Transformation.** Check whether a `targetAbi 12.x` build exists at
   <https://www.iamparadox.dev/jellyfin/plugins/manifest.json>. If not, the
   verification described above is what keeps script injection working via
   the fallback middleware — confirm that path explicitly on a real Jellyfin
   12 server before relying on it.
4. Smoke-test the built `net10.0` plugin on a real Jellyfin 12 server:
   install, confirm the Watch Party UI appears, join a room, and check the
   version-compatibility log line reads as expected.
5. Update the supported-version statements in `README.md` and
   `docs/features.md`, and merge. The next tagged release publishes both ABIs
   from the same manifest.

## Testing the authentication fix without a Jellyfin 12 server

Jellyfin 12's behaviour is reproducible on 10.11: turn `EnableLegacyAuthorization`
off in the server configuration and reload the web client. Before the fix,
`/JellyWatchParty/Token` returns 401 and no Watch Party UI appears. After it,
the request carries `Authorization: MediaBrowser …` and returns 200.
