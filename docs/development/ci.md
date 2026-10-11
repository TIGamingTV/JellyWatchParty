---
title: CI/CD
parent: Development
nav_order: 5
---

# Continuous Integration

JellyWatchParty uses GitHub Actions for continuous integration and security scanning.

## Branching model

`develop` is the integration branch — PRs land there first. `main` only receives
merges from `develop` (or hotfix branches) and is what releases are cut from.
See [Release]({{ '/development/release/' | relative_url }}) for the full flow.

## Workflows

### CI Workflow (`ci.yml`)

Runs on every push and pull request to `main` and `develop` branches.

```
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   Rust Tests    │  │ Discord Bot     │  │   .NET Tests    │  │   JS Lint       │
│  (formatting,   │  │ Tests (fmt,     │  │  (build, test)  │  │  (syntax check) │
│  clippy, test)  │  │ clippy, test)   │  │                 │  │                 │
└────────┬────────┘  └────────┬────────┘  └─────────────────┘  └─────────────────┘
         │                    │
         ▼                    ▼
┌─────────────────┐  ┌─────────────────┐
│  Build Server   │  │ Build Discord   │
│  (Docker image) │  │ Bot (Docker)    │
└─────────────────┘  └─────────────────┘
```

#### Jobs

| Job | Steps | Duration |
|-----|-------|----------|
| **Rust Tests** | Format check, Clippy, Unit tests | ~3 min |
| **Discord Bot Tests** | Format check, Clippy, Unit tests (`src/integrations/discord-bot`) | ~2 min |
| **.NET Tests** | Build, Unit tests | ~2 min |
| **JavaScript Lint** | Syntax validation | ~30s |
| **Build Server** | Docker multi-stage build (not pushed) | ~5 min |
| **Build Discord Bot Image** | Docker build of `discord-bot.Dockerfile` (not pushed) | ~3 min |

### Security Workflow (`security.yml`)

Runs on every push/PR and weekly (Monday 00:00 UTC).

| Scan | Tool | Purpose |
|------|------|---------|
| **Cargo Audit** | `cargo-audit` | Rust dependency vulnerabilities (RustSec) |
| **Trivy** | `aquasecurity/trivy` | Container image CVEs |
| **CodeQL** | `github/codeql-action` | JavaScript static analysis |

Results are uploaded to the GitHub Security tab.

### Publish Workflow (`publish.yml`)

Handles Docker image publishing to GHCR and release artifacts. A `changes`
job (via `dorny/paths-filter`) detects whether the server (`src/server/**`,
`infra/docker/server.Dockerfile`), the Discord bot
(`src/integrations/discord-bot/**`, `infra/docker/discord-bot.Dockerfile`)
or the plugin (`src/plugins/jellyfin/**`, `src/clients/jellyfin-web/**`)
changed, so each push only rebuilds the components that actually changed.
A change to `docker-image.yml` rebuilds both images.

Both images are built by the reusable workflow `docker-image.yml`: one native
runner per platform (amd64, arm64) pushes by digest, then a merge job creates
the multi-arch manifest and its tags.

#### Triggers

| Event | Condition | Result |
|-------|-----------|--------|
| Push to `main` | Server / bot changed | That image tagged `beta` |
| Push to `develop` | Server / bot changed | That image tagged `dev` |
| Push to `develop` | Plugin/client changed | Plugin rebuilt (Jellyfin 12.x), rolling `develop-latest` pre-release updated with the zip, `manifest-dev.json` updated with the `targetAbi` entry |
| GitHub Release | Published | Both images tagged `X.Y.Z`, `X.Y`, `latest`; plugin built for Jellyfin 12.x, zip attached to the release, `manifest.json` updated with the `targetAbi` entry |

#### Jobs

| Job | Trigger | Description |
|-----|---------|--------------|
| **Detect Changes** | Push only | Computes `server`/`bot`/`plugin` path-filter outputs used to gate the jobs below |
| **Session Server Image** | Server changed, or release | Builds `jwp-session-server` (amd64, arm64) and pushes to GHCR |
| **Discord Bot Image** | Bot changed, or release | Builds `jwp-discord-bot` (the chat bot: Discord and Telegram; amd64, arm64) and pushes to GHCR |
| **Build Jellyfin Plugin** | Release only | Builds the plugin for `net10.0`/Jellyfin 12.x and creates a zip archive |
| **Upload Release Assets** | Release only | Attaches the plugin zip and Windows server to the GitHub Release (needs the server image, not the bot image) |
| **Update Plugin Manifest** | Release only | Adds the `targetAbi 12.0.0.0` entry to `manifest.json` |
| **Build Develop Plugin** | Push to `develop`, plugin/client changed | Same build as the release job |
| **Publish Develop Plugin & Manifest** | After the job above | Publishes the zip as an asset on the rolling `develop-latest` pre-release and updates the `targetAbi` entry in `manifest-dev.json` |

Jellyfin 10.11.x support has been dropped; only Jellyfin 12.x is built and
published. Users still on 10.11.x should install an older release
(`targetAbi 10.11.11.0`).

#### Plugin Repository

On release, the workflow automatically updates the [Jellyfin plugin repository](https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json):

1. Downloads the built plugin zip
2. Calculates its MD5 checksum
3. Updates `docs/jellyfin-plugin-repo/manifest.json` with a new version entry (`targetAbi 12.0.0.0`)
4. Commits and pushes to `main`
5. Triggers GitHub Pages deployment

Users can then install/update the plugin directly from Jellyfin's plugin interface.

On every push to `develop` that touches the plugin or client JS, the same
thing happens against a separate develop channel — see
[Release: Develop Plugin Channel]({{ '/development/release/' | relative_url }}#develop-plugin-channel) for how
testers install it.

#### Docker Images

Same tags for `jwp-session-server` and `jwp-discord-bot`:

```bash
# Latest stable release
docker pull ghcr.io/tigamingtv/jwp-session-server:latest
docker pull ghcr.io/tigamingtv/jwp-discord-bot:latest

# Specific version (release v0.1.0)
docker pull ghcr.io/tigamingtv/jwp-session-server:0.1.0

# Latest build from main (pre-release)
docker pull ghcr.io/tigamingtv/jwp-session-server:beta

# Latest build from develop
docker pull ghcr.io/tigamingtv/jwp-session-server:dev
docker pull ghcr.io/tigamingtv/jwp-discord-bot:dev
```

A new GHCR package starts out private; see
[Release: Package Visibility]({{ '/development/release/' | relative_url }}#package-visibility).

## Build Configuration

### Rust (Alpine + musl)

The Docker build uses Alpine with musl libc for smaller images:

```dockerfile
FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev
# ... build with musl target

FROM alpine:3.21
# ~26MB final image
```

**Note:** Local development uses glibc (standard Rust). The `.cargo/config.toml` configures the `mold` linker for faster local builds, but this is excluded from Docker builds via `.dockerignore`.

### .NET

The plugin targets `net10.0` (Jellyfin 12.x — see
`src/plugins/jellyfin/Directory.Build.props`), pulling NuGet packages from
nuget.org with a version pinned in `JellyfinPackageVersion`:

```xml
<PackageReference Include="Jellyfin.Controller" Version="$(JellyfinPackageVersion)" ExcludeAssets="runtime" />
<PackageReference Include="Jellyfin.Model" Version="$(JellyfinPackageVersion)" ExcludeAssets="runtime" />
```

The `.csproj` embeds `Web\**\*.js` as resources, so CI copies the client JS
files (including subdirectories: `utils/`, `ui/`, `playback/`, `chat/`,
`ws/`, `app/`) into `JellyWatchParty/Web/` before building, using the same
explicit file list as the release/develop plugin build jobs:

```yaml
- name: Copy JS files to plugin Web directory
  run: |
    mkdir -p JellyWatchParty/Web
    cp ../../clients/jellyfin-web/plugin.js JellyWatchParty/Web/plugin.js
    for f in state.js utils/time.js ... ; do
      mkdir -p "JellyWatchParty/Web/$(dirname "$f")"
      cp "../../clients/jellyfin-web/$f" "JellyWatchParty/Web/$f"
    done
```

**Note:** An explicit file list is used instead of `cp -r .../jellyfin-web/*`
so that `tests/*.test.js` and other non-shipped files never get embedded as
plugin resources.

## Troubleshooting

### CI Failures

**Rust formatting:**
```bash
cd src/server && cargo fmt
git add -u && git commit --amend --no-edit
```

**Clippy warnings:**
```bash
cd src/server && cargo clippy -- -D warnings
# Fix warnings or add #[allow(...)] with justification
```

**Docker build fails:**
```bash
# Test locally
docker build -f infra/docker/server.Dockerfile -t test ./src/server
docker build -f infra/docker/discord-bot.Dockerfile -t test-bot ./src/integrations/discord-bot
```

## Badges

The README includes CI status badges:

```markdown
[![CI](https://img.shields.io/github/actions/workflow/status/TIGamingTV/JellyWatchParty/ci.yml?branch=main)](https://github.com/TIGamingTV/JellyWatchParty/actions/workflows/ci.yml)
```

| Badge | Meaning |
|-------|---------|
| ![CI passing](https://img.shields.io/badge/CI-passing-brightgreen) | All checks pass |
| ![CI failing](https://img.shields.io/badge/CI-failing-red) | One or more checks failed |

## Security Alerts

View security findings:

1. Go to repository **Security** tab
2. Click **Code scanning alerts** or **Dependabot alerts**
3. Review and address as needed

Current alert policy:
- **CRITICAL/HIGH**: Must fix before release
- **MEDIUM**: Fix in next release
- **LOW/NOTE**: Track, fix when convenient

## Next Steps

- [Setup]({{ '/development/setup/' | relative_url }}) - Development environment
- [Contributing]({{ '/development/contributing/' | relative_url }}) - Contribution guidelines
- [Testing]({{ '/development/testing/' | relative_url }}) - Test documentation