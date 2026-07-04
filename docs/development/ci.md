---
title: CI/CD
parent: Development
nav_order: 5
---

# Continuous Integration

OpenWatchParty uses GitHub Actions for continuous integration and security scanning.

## Branching model

`develop` is the integration branch — PRs land there first. `main` only receives
merges from `develop` (or hotfix branches) and is what releases are cut from.
See [Release](release) for the full flow.

## Workflows

### CI Workflow (`ci.yml`)

Runs on every push and pull request to `main` and `develop` branches.

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Rust Tests    │     │   .NET Tests    │     │   JS Lint       │
│  (formatting,   │     │  (build, test)  │     │  (syntax check) │
│  clippy, test)  │     │                 │     │                 │
└────────┬────────┘     └─────────────────┘     └─────────────────┘
         │
         ▼
┌─────────────────┐
│  Build Server   │
│  (Docker image) │
└─────────────────┘
```

#### Jobs

| Job | Steps | Duration |
|-----|-------|----------|
| **Rust Tests** | Format check, Clippy, Unit tests | ~3 min |
| **.NET Tests** | Build, Unit tests | ~2 min |
| **JavaScript Lint** | Syntax validation | ~30s |
| **Build Server** | Docker multi-stage build | ~5 min |

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
job (via `dorny/paths-filter`) detects whether `src/server/**` or the plugin
(`src/plugins/jellyfin/**`, `src/clients/jellyfin-web/**`) changed, so each
push only rebuilds the components that actually changed.

#### Triggers

| Event | Condition | Result |
|-------|-----------|--------|
| Push to `main` | Server changed | Docker image tagged `beta` |
| Push to `develop` | Server changed | Docker image tagged `dev` |
| Push to `develop` | Plugin/client changed | Plugin rebuilt, rolling `develop-latest` pre-release updated, `manifest-dev.json` updated |
| GitHub Release | Published | Docker image tagged `vX.Y.Z`, `vX.Y`, `latest`; plugin built, attached to the release, `manifest.json` updated |

#### Jobs

| Job | Trigger | Description |
|-----|---------|--------------|
| **Detect Changes** | Push only | Computes `server`/`plugin` path-filter outputs used to gate the jobs below |
| **Build & Push Docker Image** | Server changed, or release | Builds multi-platform image (amd64, arm64) and pushes to GHCR |
| **Build Jellyfin Plugin** | Release only | Builds plugin and creates zip archive |
| **Upload Release Assets** | Release only | Attaches plugin zip to GitHub Release |
| **Update Plugin Manifest** | Release only | Updates `manifest.json` for Jellyfin plugin repository |
| **Build & Publish Develop Plugin** | Push to `develop`, plugin/client changed | Builds plugin, publishes it as an asset on the rolling `develop-latest` pre-release |
| **Update Develop Plugin Manifest** | After the job above | Updates `manifest-dev.json` for the develop plugin channel |

#### Plugin Repository

On release, the workflow automatically updates the [Jellyfin plugin repository](https://tigamingtv.github.io/OpenWatchParty/jellyfin-plugin-repo/manifest.json):

1. Downloads the built plugin zip
2. Calculates MD5 checksum
3. Updates `docs/jellyfin-plugin-repo/manifest.json` with new version
4. Commits and pushes to `main`
5. Triggers GitHub Pages deployment

Users can then install/update the plugin directly from Jellyfin's plugin interface.

On every push to `develop` that touches the plugin or client JS, the same
thing happens against a separate develop channel — see
[Release: Develop Plugin Channel](release#develop-plugin-channel) for how
testers install it.

#### Docker Image

```bash
# Latest stable release
docker pull ghcr.io/tigamingtv/owp-session-server:latest

# Specific version
docker pull ghcr.io/tigamingtv/owp-session-server:v0.1.0

# Latest build from main (pre-release)
docker pull ghcr.io/tigamingtv/owp-session-server:beta

# Latest build from develop
docker pull ghcr.io/tigamingtv/owp-session-server:dev
```

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

The plugin uses NuGet packages from nuget.org:

```xml
<PackageReference Include="Jellyfin.Controller" Version="10.11.5" />
<PackageReference Include="Jellyfin.Model" Version="10.11.5" />
```

The `.csproj` embeds `Web\**\*.js` as resources, so CI copies the client JS
files (including subdirectories: `utils/`, `ui/`, `playback/`, `chat/`,
`ws/`, `app/`) into `OpenWatchParty/Web/` before building, using the same
explicit file list as the release/develop plugin build jobs:

```yaml
- name: Copy JS files to plugin Web directory
  run: |
    mkdir -p OpenWatchParty/Web
    cp ../../clients/jellyfin-web/plugin.js OpenWatchParty/Web/plugin.js
    for f in state.js utils/time.js ... ; do
      mkdir -p "OpenWatchParty/Web/$(dirname "$f")"
      cp "../../clients/jellyfin-web/$f" "OpenWatchParty/Web/$f"
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
docker build -t test ./src/server
```

## Badges

The README includes CI status badges:

```markdown
[![CI](https://img.shields.io/github/actions/workflow/status/TIGamingTV/OpenWatchParty/ci.yml?branch=main)](https://github.com/TIGamingTV/OpenWatchParty/actions/workflows/ci.yml)
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

- [Setup](setup) - Development environment
- [Contributing](contributing) - Contribution guidelines
- [Testing](testing) - Test documentation