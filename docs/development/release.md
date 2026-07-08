---
title: Release
parent: Development
nav_order: 4
---

# Release Process

## Branching Model

`develop` is the integration branch: PRs from feature/fix branches land there
first. `main` only ever receives merges from `develop` (or a `release/*` /
`hotfix/*` branch cut from it) via pull request — no direct pushes. This is
enforced by branch protection on `main` (pull request required; the
`github-actions` bot is exempted so the automated manifest-update commits
described below can still land).

```
feature/fix branches ──PR──> develop ──PR──> main ──tag──> GitHub Release
                                │
                                └─ every push (that touches server or
                                   plugin/client code) publishes a
                                   rolling "dev" build of both
```

## Versioning

JellyWatchParty uses [Semantic Versioning](https://semver.org/):

```
MAJOR.MINOR.PATCH
```

- **MAJOR**: Breaking changes
- **MINOR**: New features (backwards compatible)
- **PATCH**: Bug fixes (backwards compatible)

Builds published from `develop` (Docker `dev` tag, plugin develop channel)
use their own non-conflicting scheme — see
[Develop Builds](#develop-builds) below — and are never meant to be
tagged as a release themselves.

## Release Checklist

### Pre-Release

- [ ] All tests pass
- [ ] Documentation updated
- [ ] CHANGELOG updated
- [ ] Version numbers updated
- [ ] Manual testing completed

### Version Locations

Update version in:

1. **Rust (`Cargo.toml`)**:
   ```toml
   [package]
   version = "0.2.0"
   ```

2. **C# Plugin (`.csproj`)**:
   ```xml
   <Version>0.2.0</Version>
   ```

3. **Plugin metadata (`Plugin.cs`)** if applicable

### CHANGELOG Format

```markdown
# Changelog

## [0.2.0] - 2024-01-15

### Added
- New feature description

### Changed
- Change description

### Fixed
- Bug fix description

### Security
- Security fix description

## [0.1.0] - 2024-01-01

Initial release.
```

## Build Process

### Build All Components

```bash
just build
```

### Build Individually

```bash
# Rust session server
cd src/server
cargo build --release

# C# plugin
cd src/plugins/jellyfin/JellyWatchParty
dotnet build -c Release
```

### Build Artifacts

| Component | Output Location |
|-----------|-----------------|
| Session Server | `src/server/target/release/session-server` |
| Session Server (Windows) | `jwp-session-server-windows-vX.Y.Z.zip` (CI-built, attached to GitHub Release) |
| Plugin DLL | `src/plugins/jellyfin/JellyWatchParty/bin/Release/net9.0/JellyWatchParty.dll` |

## Release Steps

Release branches fork from `develop`, land on `main` via pull request, get
tagged there, and merge back into `develop` so both branches stay in sync.

### 1. Create Release Branch

```bash
git checkout develop
git pull origin develop
git checkout -b release/v0.2.0
```

### 2. Update Versions

Update all version numbers as listed above.

### 3. Update CHANGELOG

### 4. Commit Changes

```bash
git add -A
git commit -m "Release v0.2.0"
git push origin release/v0.2.0
```

### 5. Open a Pull Request into `main`

Open the PR from `release/v0.2.0` into `main`, get it reviewed, and merge it
(branch protection requires this — there is no direct push to `main`).

### 6. Create Tag (on `main`, after the merge)

```bash
git checkout main
git pull origin main
git tag -a v0.2.0 -m "Version 0.2.0"
git push origin v0.2.0
```

### 7. Create GitHub Release

Using the GitHub CLI (recommended):

```bash
gh release create v0.2.0 --title "v0.2.0"
```

Or via GitHub UI:

1. Go to GitHub > Releases > New Release
2. Select tag `v0.2.0`
3. Title: `v0.2.0`
4. Description: Copy from CHANGELOG
5. Click **Publish release**

The workflow will automatically:
- Build and push Docker images to GHCR
- Build and attach the Jellyfin plugin zip
- Build and attach a standalone Windows session server binary
- Update `manifest.json` for the plugin repository

### 8. Merge Back into `develop`

Keep `develop` up to date with the release commit (version bumps, changelog)
via a PR from `main` into `develop`:

```bash
git checkout develop
git pull origin develop
git merge main
git push origin develop
```

### 9. Clean Up

```bash
git branch -d release/v0.2.0
```

## Docker Images

Docker images are automatically built and pushed to GitHub Container Registry (GHCR).

### Available Tags

| Tag | Description | Updated |
|-----|-------------|---------|
| `latest` | Latest stable release | On release |
| `vX.Y.Z` | Specific version (e.g., `v0.1.0`) | On release |
| `vX.Y` | Minor version (e.g., `v0.1`) | On release |
| `beta` | Latest build from `main` | On push to `main` (server changed) |
| `dev` | Latest build from `develop` | On push to `develop` (server changed) |

### Pull Images

```bash
# Latest stable
docker pull ghcr.io/tigamingtv/jwp-session-server:latest

# Specific version
docker pull ghcr.io/tigamingtv/jwp-session-server:v0.1.0

# Latest from main
docker pull ghcr.io/tigamingtv/jwp-session-server:beta

# Latest from develop
docker pull ghcr.io/tigamingtv/jwp-session-server:dev
```

### Build Locally (optional)

```bash
docker build -t jwp-session-server:local ./src/server
```

## Develop Builds

Every push to `develop` that touches the relevant code publishes a rolling
build, so testers always have the latest in-progress version of both
components without waiting for a tagged release.

| Component | Where it lands | Version scheme |
|-----------|-----------------|----------------|
| Session Server | Docker image `ghcr.io/tigamingtv/jwp-session-server:dev` | Tag stays `dev`, content changes each push |
| Jellyfin Plugin | Rolling pre-release [`develop-latest`](https://github.com/TIGamingTV/JellyWatchParty/releases/tag/develop-latest), tracked via `manifest-dev.json` | `0.0.<GitHub run number>` (always increasing, always below `1.0`) |

### Develop Plugin Channel

The plugin's develop builds are exposed as a second, separate Jellyfin
plugin repository so they can be installed/updated like a beta channel,
without touching the stable `manifest.json` feed:

1. Go to Dashboard > Plugins > Repositories
2. Add: `https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest-dev.json`
3. Go to Catalog > Find "JellyWatchParty (Develop)" > Install
4. Restart Jellyfin

Because dev versions are always `0.0.x` (below any real `1.x` release), it's
safe to have both the stable and dev repositories added at once — Jellyfin
will prefer whichever is numerically higher, so a stable release always wins
over the dev channel once one exists past `0.0.x`. Uninstall the dev-channel
entry if you no longer want to test pre-release builds.

## Automated Releases

Releases are fully automated via GitHub Actions (`.github/workflows/publish.yml`).

### What Happens on Release

When you create a GitHub Release:

1. **Docker Image**: Built for amd64 and arm64, pushed to GHCR with version + `latest` tags
2. **Jellyfin Plugin**: Built, zipped, and attached to the release
3. **Windows Session Server**: Built natively on `windows-latest`, zipped with a
   `session-server.exe` and a short usage README, and attached to the release —
   no Docker or Rust install required to run it
4. **Plugin Repository**: `manifest.json` updated with new version and deployed to GitHub Pages

### What Happens on Push to `main`

When server code changes (`src/server/**`) are pushed to `main`:

1. **Docker Image**: Built and pushed with `beta` tag
2. Allows testers to always have the latest development version

### What Happens on Push to `develop`

1. **Server changed**: Docker image built and pushed with the `dev` tag
2. **Plugin/client changed**: plugin rebuilt, attached to the rolling
   `develop-latest` pre-release, and `manifest-dev.json` updated — see
   [Develop Builds](#develop-builds)

A `changes` job in `publish.yml` (via `dorny/paths-filter`) detects which of
the two actually changed, so an unrelated change doesn't trigger a rebuild
of the other component.

### Plugin Distribution

Users can install the plugin in three ways:

#### Via Jellyfin UI (Recommended, stable)

1. Go to Dashboard > Plugins > Repositories
2. Add: `https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json`
3. Go to Catalog > Find "JellyWatchParty" > Install
4. Restart Jellyfin

#### Via Jellyfin UI (develop/beta channel)

See [Develop Plugin Channel](#develop-plugin-channel) above.

#### Via Direct Download

1. Go to [Releases](https://github.com/TIGamingTV/JellyWatchParty/releases)
2. Download `JellyWatchParty-vX.Y.Z.zip`
3. Extract to Jellyfin plugins folder
4. Restart Jellyfin

## Hotfix Process

For critical bug fixes:

1. Branch from the release tag:
   ```bash
   git checkout -b hotfix/v0.2.1 v0.2.0
   ```

2. Apply fix and commit

3. Update patch version

4. Follow normal release process with new tag `v0.2.1` (PR into `main`, tag, release, then merge back into `develop`)

## Deprecation Policy

- Announce deprecations in release notes
- Maintain for at least one minor version
- Provide migration guide when removing features

## Rollback Process

If a release has critical issues:

1. **Immediate**: Advise users to use previous version
2. **GitHub**: Mark release as pre-release or delete
3. **Fix**: Create hotfix release
4. **Communicate**: Update issue/discussion with status

## Release Communication

### Channels

- GitHub Releases (primary)
- GitHub Discussions (announcements)
- Jellyfin forums (if applicable)

### Template

```markdown
## What's New

Brief summary of changes.

## Highlights

- Feature 1
- Feature 2

## Breaking Changes

List any breaking changes and migration steps.

## Installation

See [Installation Guide](docs/operations/installation.md).

## Upgrading

See [Upgrade Procedure](docs/operations/deployment.md#upgrade-procedure).

## Changelog

```

## Next Steps

- [Contributing](contributing.md) - How to contribute
- [Testing](testing.md) - Testing before release
- [Deployment](../operations/deployment.md) - Production deployment