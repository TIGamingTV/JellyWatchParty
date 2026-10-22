---
title: CI/CD
parent: Development
nav_order: 5
---

# Continuous Integration

GitHub Actions runs five workflows from `.github/workflows/`.

| Workflow | File | Triggers |
|----------|------|----------|
| CI | `ci.yml` | Push and PR to `main` / `develop` |
| Security | `security.yml` | Push and PR to `main` / `develop`, weekly (Monday 00:00 UTC) |
| Publish | `publish.yml` | Push to `main` / `develop`, release published |
| Docker Image | `docker-image.yml` | Reusable, called by Publish once per image |
| Deploy Documentation | `docs.yml` | Push to `main` touching `docs/**` or the workflow, manual dispatch, dispatched by Publish after a manifest update |

## Branching model

Releases are cut from `main`. Fixes and self-contained features are merged
into `main`; larger work goes through `develop`, which publishes `dev` test
builds. See [Release]({{ '/development/release/' | relative_url }}#branching-model).

## CI (`ci.yml`)

```
Rust Tests ─────────▶ Build Server
Discord Bot Tests ──▶ Build Discord Bot Image
.NET Tests
JavaScript Lint & Tests
```

| Job | What it runs |
|-----|--------------|
| **Rust Tests** | `src/server`: deletes `.cargo/config.toml` (mold), then `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --verbose` on stable Rust |
| **Discord Bot Tests** | `src/integrations/discord-bot`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` on stable Rust |
| **.NET Tests** | .NET SDK 10.0.x: copies the client JS into `JellyWatchParty/Web/`, then restore, build and test `JellyWatchParty.Tests` |
| **JavaScript Lint & Tests** | Node 20: `node --check` on every client file outside `tests/`, then `node --test src/clients/jellyfin-web/tests/*.test.js` |
| **Build Server** | Builds `server.Dockerfile` (default `BUILD_MODE=dev`), not pushed |
| **Build Discord Bot Image** | Builds `discord-bot.Dockerfile` with `BUILD_MODE=release`, not pushed |

CI uses the latest stable Rust while the Dockerfiles pin `rust:1.88-alpine`;
new clippy lints on stable can fail CI in code that hasn't changed.

## Security (`security.yml`)

| Job | Tool | Notes |
|-----|------|-------|
| **Cargo Audit** | `cargo audit` | Server and bot crates. Results are only in the job log, not the Security tab |
| **Trivy Container Scan** (`session-server`, `discord-bot`) | `aquasecurity/trivy-action`, pinned by commit (v0.36.0) | `docker build --pull`, then reports fixable `CRITICAL`/`HIGH` CVEs as SARIF (categories `trivy-session-server`, `trivy-discord-bot`) |
| **CodeQL Analysis** | `github/codeql-action` | JavaScript |

Trivy and CodeQL results appear under **Security > Code scanning**.

## Publish (`publish.yml`)

On pushes, a **Detect Changes** job (`dorny/paths-filter`) decides what to
rebuild:

| Output | Paths |
|--------|-------|
| `server` | `src/server/**`, `infra/docker/server.Dockerfile`, `.github/workflows/docker-image.yml` |
| `bot` | `src/integrations/discord-bot/**`, `infra/docker/discord-bot.Dockerfile`, `.github/workflows/docker-image.yml` |
| `plugin` | `src/plugins/jellyfin/**`, `src/clients/jellyfin-web/**` |

A release rebuilds everything regardless.

| Job | Runs on | Does |
|-----|---------|------|
| **Session Server Image** | `server` changed, or release | `ghcr.io/tigamingtv/jwp-session-server` via `docker-image.yml` |
| **Discord Bot Image** | `bot` changed, or release | `ghcr.io/tigamingtv/jwp-discord-bot` (the Discord + Telegram bot) via `docker-image.yml` |
| **Build Jellyfin Plugin** | Release | `dotnet publish -f net10.0` with `Version`/`AssemblyVersion`/`FileVersion` set to the tag minus `v`; zips `JellyWatchParty-<tag>.zip` |
| **Build Windows Session Server** | Release | `cargo build --release` on `windows-latest`; zips `session-server.exe` + `README.txt` as `jwp-session-server-windows-<tag>.zip` |
| **Upload Release Assets** | Release, after the server image, plugin and Windows jobs succeed | Attaches both zips to the release (the bot image is not required) |
| **Update Plugin Repository Manifest** | Release, after the assets | Prepends a version (`targetAbi 12.0.0.0`, MD5 checksum, release body as changelog) to `docs/jellyfin-plugin-repo/manifest.json` on `main`, then dispatches `docs.yml` |
| **Build Develop Plugin** | Push to `develop` with `plugin` changed | Same build with version `0.0.<run_number>`, zipped as `JellyWatchParty-dev.zip` |
| **Publish Develop Plugin & Manifest** | After the job above | Uploads the zip to the rolling `develop-latest` prerelease, replaces the single entry in `manifest-dev.json` on `main`, dispatches `docs.yml` |

The manifest step only accepts numeric tags with 2-4 parts (`v4.1.4.0`) and
expects the `v` prefix in the asset name.

### Docker images (`docker-image.yml`)

Each image is built natively per platform (`linux/amd64` on
`ubuntu-latest`, `linux/arm64` on `ubuntu-24.04-arm`) with
`BUILD_MODE=release`, pushed by digest, and joined into one multi-arch
manifest by the **Merge & Push Manifest** job. The build uses `pull: true`
and `no-cache-filters: runtime`, so the `runtime` stage (which runs
`apk upgrade`) is rebuilt every time while the Rust builder layers come from
the GHA cache. Fixing a Trivy alert in an Alpine package is usually a
rebuild; when the Alpine branch nears end of life, bump `FROM alpine:` in both
Dockerfiles.

| Event | Tags |
|-------|------|
| Push to `main` | `beta` |
| Push to `develop` | `dev` |
| Release | `latest` |

The workflow also asks `docker/metadata-action` for `type=semver` tags
(full version and major.minor) on releases, but release tags are four
parts (`v4.1.4.0`), which is not valid semver, so none are produced. GHCR
currently has only `latest`, `beta` and `dev`; there are no per-version image
tags to pin.

```bash
docker pull ghcr.io/tigamingtv/jwp-session-server:latest   # last release
docker pull ghcr.io/tigamingtv/jwp-session-server:beta     # last main build
docker pull ghcr.io/tigamingtv/jwp-discord-bot:dev         # last develop build
```

A new GHCR package starts out private; see
[Release: Package Visibility]({{ '/development/release/' | relative_url }}#package-visibility).

### Plugin repository

Jellyfin installs from `https://tigamingtv.github.io/JellyWatchParty/jellyfin-plugin-repo/manifest.json`
(stable) or `manifest-dev.json` (develop, see
[Release: Develop Plugin Channel]({{ '/development/release/' | relative_url }}#develop-plugin-channel)).
Both files live in `docs/jellyfin-plugin-repo/` on `main`, and the site is
redeployed after each update.

## Build Notes

### .NET

The plugin targets `net10.0` and Jellyfin 12.x
(`src/plugins/jellyfin/Directory.Build.props`: `JellyfinPackageVersion`
12.0.0, `JellyfinTargetAbi` 12.0.0.0). `Jellyfin.Controller` and
`Jellyfin.Model` come from nuget.org with `ExcludeAssets="runtime"`.

The csproj embeds `Web\**\*.js`, so every job that builds the plugin first
copies the client files into `JellyWatchParty/Web/` from an explicit list
(not `cp -r`, so `tests/` never gets embedded). That list exists in
`ci.yml`, in both plugin jobs of `publish.yml`, and as `client_js_files` in
`infra/just/common.just`; keep them identical.

## Troubleshooting

**Rust formatting:**
```bash
(cd src/server && cargo fmt)
(cd src/integrations/discord-bot && cargo fmt)
```

**Clippy:**
```bash
(cd src/server && cargo clippy -- -D warnings)
(cd src/integrations/discord-bot && cargo clippy --all-targets -- -D warnings)
```

**`cargo test --locked` fails in the bot job:** `Cargo.lock` is out of date;
run `cargo update -p <crate>` or `cargo build` locally and commit the lock
file.

**Docker build:**
```bash
docker build -f infra/docker/server.Dockerfile -t test ./src/server
docker build -f infra/docker/discord-bot.Dockerfile -t test-bot ./src/integrations/discord-bot
```

## Badge

The README shows the CI status of `main`:

```markdown
[![CI](https://img.shields.io/github/actions/workflow/status/TIGamingTV/JellyWatchParty/ci.yml?branch=main)](https://github.com/TIGamingTV/JellyWatchParty/actions/workflows/ci.yml)
```

## Next Steps

- [Setup]({{ '/development/setup/' | relative_url }})
- [Contributing]({{ '/development/contributing/' | relative_url }})
- [Testing]({{ '/development/testing/' | relative_url }})
