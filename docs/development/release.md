---
title: Release
parent: Development
nav_order: 4
---

# Release Process

## Branching Model

Releases are tagged from `main`. Fixes and self-contained features are
merged into `main` through pull requests. Larger, multi-PR work is
integrated on `develop` first, where every push publishes `dev` test builds,
and then reaches `main` through a `develop` → `main` pull request; `main` is
merged back into `develop` afterwards. The `github-actions` bot commits the
manifest updates described below directly to `main`.

```
fix/feature branch ──PR──────────────────> main ──tag──> GitHub Release
large work ──PR──> develop ──PR──────────> main
                      └─ pushes publish rolling develop builds
```

## Versioning

Releases use the four-part Jellyfin plugin version format
`MAJOR.MINOR.PATCH.BUILD`, tagged with a `v` prefix (for example `v4.1.4.0`).
The fourth part is normally `0`.

- **MAJOR**: breaking changes (e.g. dropping a Jellyfin generation)
- **MINOR**: new features
- **PATCH**: fixes

The release tag is the plugin version: the publish workflow stamps it into
the assembly and the manifest. Develop builds use `0.0.<run number>` (see
[Develop Builds](#develop-builds)) and are never tagged.

### Version locations

| File | Field | Notes |
|------|-------|-------|
| `src/plugins/jellyfin/JellyWatchParty/JellyWatchPartyPlugin.csproj` | `Version`, `AssemblyVersion`, `FileVersion` | Bump to the next release version. CI overrides them with the tag on release, but local builds use these values |
| `src/server/Cargo.toml` | `version` | Crate version, independent of the release tag |
| `src/integrations/discord-bot/Cargo.toml` | `version` | Crate version, independent of the release tag |

`Plugin.PluginVersion` is read from the assembly; there is nothing to edit in
`Plugin.cs`.

## Release Checklist

- [ ] Everything to ship is on `main` (pending `develop` work merged through
      a `develop` → `main` PR) and CI is green there
- [ ] Docs updated
- [ ] csproj version bumped
- [ ] Release notes written (the release body becomes the `changelog` of the
      new entry in the plugin repository manifest)
- [ ] Manual testing done

## Release Steps

### 1. Bump the version

Bump the csproj version in a PR into `main` (usually as the last commit of
the release's PR):

```bash
git commit -am "chore(plugin): bump version to 4.2.0.0"
```

### 2. Tag and publish the release (on `main`)

```bash
git checkout main
git pull origin main
gh release create v4.2.0.0 --target main --title "v4.2.0.0" --notes-file notes.md
```

`gh release create` creates the tag on `main` if it doesn't exist. In the web UI:
Releases > Draft a new release > choose or create the tag > write the notes >
**Publish release**. The tag must be `v` + 2-4 numeric parts, or the manifest
update fails.

The Publish workflow then:

- Builds `ghcr.io/tigamingtv/jwp-session-server` and
  `ghcr.io/tigamingtv/jwp-discord-bot` for `linux/amd64` and `linux/arm64`
  and tags them `latest`
- Builds `JellyWatchParty-<tag>.zip` (Jellyfin 12.x, `net10.0`) and
  `jwp-session-server-windows-<tag>.zip`, and attaches both to the release
- Prepends the new version (`targetAbi 12.0.0.0`) to
  `docs/jellyfin-plugin-repo/manifest.json` on `main` and redeploys the site

### 3. Merge back into `develop`

Open a PR from `main` into `develop` so `develop` carries the release commit
and the manifest update.

## Build Locally

```bash
just build server     # src/server/target/release/session-server
just build discord    # src/integrations/discord-bot/target/release/jwp-discord-bot
just build plugin     # src/plugins/jellyfin/JellyWatchParty/dist/ (JellyWatchPartyPlugin.dll, meta.json, ...)
just release          # JellyWatchParty-release.zip (plugin + Linux server binary)
```

`just build plugin` writes a dev `meta.json` (version `1.0.0.0`) for the
local Jellyfin container; it is not what the release ships.

## Docker Images

`.github/workflows/publish.yml` runs `.github/workflows/docker-image.yml` once
per image:

| Image | Built from | Rebuilt on push when these change |
|-------|------------|-----------------------------------|
| `ghcr.io/tigamingtv/jwp-session-server` | `src/server`, `infra/docker/server.Dockerfile` | `src/server/**`, the Dockerfile, `docker-image.yml` |
| `ghcr.io/tigamingtv/jwp-discord-bot` | `src/integrations/discord-bot`, `infra/docker/discord-bot.Dockerfile` | `src/integrations/discord-bot/**`, the Dockerfile, `docker-image.yml` |

A release always rebuilds both.

### Tags

| Tag | Content | Updated |
|-----|---------|---------|
| `latest` | Last release | On release |
| `beta` | Last build from `main` | Push to `main` that changed the image |
| `dev` | Last build from `develop` | Push to `develop` that changed the image |

There are no per-version tags. The workflow requests semver tags, but
`docker/metadata-action` can't parse four-part release tags such as
`v4.1.4.0`, so it produces none; GHCR currently holds only `latest`, `beta`
and `dev` for both images. To pin a version, pin the image digest.

```bash
docker pull ghcr.io/tigamingtv/jwp-session-server:latest
docker pull ghcr.io/tigamingtv/jwp-discord-bot:latest
```

`infra/docker/prod/docker-compose.yml` uses these images; `JWP_TAG` picks the
tag for both (default `latest`):

```bash
JWP_TAG=dev docker compose -f infra/docker/prod/docker-compose.yml --profile discord pull
JWP_TAG=dev docker compose -f infra/docker/prod/docker-compose.yml --profile discord up -d
```

### Package Visibility

GHCR creates a package as **private** the first time it is pushed. After the
first publish of a new image, open the repository's **Packages** > the
package > **Package settings** > **Change visibility** and make it
**Public**, or nobody can pull it without logging in.

### Build images locally

```bash
docker build -f infra/docker/server.Dockerfile --build-arg BUILD_MODE=release \
  -t jwp-session-server:local ./src/server
docker build -f infra/docker/discord-bot.Dockerfile \
  -t jwp-discord-bot:local ./src/integrations/discord-bot
```

## Develop Builds

Every push to `develop` that touches the relevant code publishes a rolling
build:

| Component | Where it lands | Version |
|-----------|----------------|---------|
| Session server | `ghcr.io/tigamingtv/jwp-session-server:dev` | Tag stays `dev` |
| Chat bot | `ghcr.io/tigamingtv/jwp-discord-bot:dev` | Tag stays `dev` |
| Jellyfin plugin | `JellyWatchParty-dev.zip` on the [`develop-latest`](https://github.com/TIGamingTV/JellyWatchParty/releases/tag/develop-latest) prerelease, listed in `manifest-dev.json` | `0.0.<run number>` |

`manifest-dev.json` only ever lists the latest develop build.

### Develop Plugin Channel

The develop builds have their own Jellyfin plugin repository, "JellyWatchParty
(Develop)":

1. Dashboard > Plugins > Repositories
2. Add `https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest-dev.json`
3. Catalog > "JellyWatchParty (Develop)" > Install
4. Restart Jellyfin

Both repositories publish the same plugin GUID. Develop versions (`0.0.x`) are
always lower than any release (`4.x`), so with a release installed Jellyfin
never offers a develop build as an update. To follow the develop channel,
uninstall the release first (and remove the stable repository if you don't
want it to update you back); to return to releases, do the reverse.

## Plugin Distribution

Users install from the plugin repository (`manifest.json`) or the release
zip; see [Installation]({{ '/installation/' | relative_url }}#2-install-the-jellyfin-plugin).

## Hotfix Process

1. Branch from `main` (or the release tag):
   ```bash
   git checkout -b hotfix/v4.2.1.0 v4.2.0.0
   ```
2. Fix, bump the csproj version, commit.
3. PR into `main`, release `v4.2.1.0`, then merge `main` back into `develop`.

## Deprecation Policy

- Announce deprecations in the release notes.
- Keep a deprecated feature for at least one minor version.
- Document the migration when removing it.

## Rollback

1. Tell users to stay on (or reinstall) the previous version; for Docker,
   pin the previous image by digest.
2. Mark the GitHub release as a prerelease or delete it. Its entry stays in
   `manifest.json` until removed by hand.
3. Ship a hotfix release.
4. Post the status on the related issue.

## Release Notes Template

```markdown
## What's New

Short summary.

## Breaking Changes

What changed and how to migrate.

## Upgrading

See https://tigamingtv.github.io/JellyWatchParty/deployment/#updating

## Changes

- ...
```

## Next Steps

- [CI/CD]({{ '/development/ci/' | relative_url }})
- [Testing]({{ '/development/testing/' | relative_url }})
- [Deployment]({{ '/deployment/' | relative_url }})
