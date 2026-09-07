---
title: Jellyfin 12 Migration
nav_order: 10
---

# Jellyfin 12 Migration

Jellyfin's next release drops the leading `10.` from its version numbers:
`10.11.x` becomes `12.x`. At the time of writing the latest preview is
`v12.0-rc3`.

**JellyWatchParty does not support Jellyfin 12 yet.** This page records what
was found when the plugin was checked against `v12.0-rc3`, what has already
been changed in preparation, and the checklist to follow when Jellyfin 12.0 is
released.

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
`Jellyfin.Controller 12.0.0-rc3` package.

### The server API this plugin uses is unchanged

Every Jellyfin type the plugin touches is byte-identical between `v10.11.11`
and `v12.0-rc3`: `ISessionManager` (its playback events, `Sessions` and
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
  `MediaBrowser.Providers`) and binding is by simple name, so it resolves.
- `System.IdentityModel.Tokens.Jwt` and `Microsoft.IdentityModel.Tokens`
  (6.35.0) *are* shipped by the plugin. Jellyfin 12 has no IdentityModel
  reference of its own, so there is no conflict, but 6.x is old for .NET 10.

## What has been prepared

These changes are already in place. All of them are correct on 10.11 today and
none of them wait for Jellyfin 12.

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
- **The framework and ABI live in one place each.**
  `src/plugins/jellyfin/Directory.Build.props` holds `TargetFramework` and
  `JellyfinPackageVersion`; `.github/workflows/` hold `DOTNET_VERSION` and
  `JELLYFIN_TARGET_ABI`.

## The switch, when Jellyfin 12.0 is released

1. **Retarget.** In `src/plugins/jellyfin/Directory.Build.props`, set
   `TargetFramework` to `net10.0` and `JellyfinPackageVersion` to the 12.x
   release. No source changes are expected — see the API findings above.
2. **CI.** Set `DOTNET_VERSION` to `10.0.x` in `.github/workflows/ci.yml` and
   `.github/workflows/publish.yml`.
3. **Dependencies.** Pin `Newtonsoft.Json` to the version the server ships
   (13.0.4 as of 12.0-rc3), and bump `System.IdentityModel.Tokens.Jwt` and
   `Microsoft.IdentityModel.Tokens` to 8.x. Re-test
   `SessionServerAuth.CreateToken` and the `/JellyWatchParty/Token` endpoint.
4. **Manifest / ABI.** Set `JELLYFIN_TARGET_ABI` to `12.0.0.0`. Decide the
   release shape at that point:
   - *Clean cutover* — simplest; 10.11 users stay on the last 10.11 release.
   - *Dual ABI* — two build jobs and two sets of manifest entries, one per
     `targetAbi`, so both server generations keep getting updates. This is what
     other plugins in the ecosystem do.
5. **File Transformation.** Confirm whether a `targetAbi 12.x` build exists. If
   not, the verification described above is what keeps script injection
   working, via the fallback middleware — confirm that path explicitly on a
   Jellyfin 12 server.
6. **Docs.** Update the supported-version statements in `README.md` and this
   page, and add a changelog entry.

## Testing the authentication fix without a Jellyfin 12 server

Jellyfin 12's behaviour is reproducible on 10.11: turn `EnableLegacyAuthorization`
off in the server configuration and reload the web client. Before the fix,
`/JellyWatchParty/Token` returns 401 and no Watch Party UI appears. After it,
the request carries `Authorization: MediaBrowser …` and returns 200.
