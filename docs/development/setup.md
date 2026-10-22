---
title: Setup
parent: Development
nav_order: 1
---

# Development Setup

## Prerequisites

- **Docker** with the **Compose** plugin. The dev stack, the plugin build
  (`mcr.microsoft.com/dotnet/sdk:10.0`) and the plugin tests all run in
  containers, so a local .NET SDK is optional.
- **[Just](https://just.systems/)**, the command runner (`cargo install just`
  or [other methods](https://just.systems/man/en/packages.html)).
- **Rust** (stable) for the recipes that run `cargo` on the host:
  `just build server`, `just test server|discord`, `just lint`, `just fmt`,
  `just check`. CI uses the latest stable toolchain; the Docker images build
  with Rust 1.88, so don't rely on anything newer.
- **mold**: `src/server/.cargo/config.toml` always links with mold (see
  [Cargo configuration](#cargo-configuration)).
- **Node.js 20+** for the client tests (`just test client`).
- **curl** and **unzip** (`just build ft`), **zip** (`just release`),
  **inotifywait** or **fswatch** (`just watch`).
- **.NET 10 SDK** only if you want to build or test the plugin outside Docker.

## Quick Start

```bash
git clone https://github.com/TIGamingTV/JellyWatchParty.git
cd JellyWatchParty

# Point git at .githooks/ (once per clone)
just setup

# Optional: local settings (ports, MEDIA_DIR, JWT_SECRET, admin panel, ...)
cp .env.example .env

# Build and start the dev stack
just up
```

`just up`:

1. Downloads the File Transformation plugin (`just build ft`) into
   `infra/docker/dev/config/plugins/FileTransformation`, if that version isn't
   there yet. It injects the client script into Jellyfin's `index.html`.
2. Builds the JellyWatchParty plugin (`just build plugin`): copies the client
   JS into `src/plugins/jellyfin/JellyWatchParty/Web/`, runs `dotnet publish`
   in the `plugin-builder` container, and writes `dist/meta.json`.
3. Starts `session-server` (built from `infra/docker/server.Dockerfile` with
   `BUILD_MODE=dev`) and `jellyfin-dev` (`jellyfin/jellyfin:12.0`, with the
   plugin's `dist/` mounted as `/config/plugins/JellyWatchParty`).

| Service | URL |
|---------|-----|
| Jellyfin | `http://localhost:8096` (`JELLYFIN_PORT`) |
| Session server WebSocket | `ws://localhost:3000/ws` (`SESSION_SERVER_PORT`) |
| Session server health | `http://localhost:3000/health` |
| Admin panel | `http://localhost:3001` (`ADMIN_PANEL_PORT`; only when `ADMIN_PASSWORD` is set) |

The chat bot is not started by `just up`. To run it, set its tokens in `.env`
and start it with its Compose profile:

```bash
docker compose -f infra/docker/dev/docker-compose.yml --profile discord up -d discord-bot
```

## First-Time Setup

### 1. Jellyfin

1. Open `http://localhost:8096` and finish the setup wizard.
2. Add a library. The dev compose file mounts `MEDIA_DIR` (default
   `~/Videos/Movies`) read-only at `/media/Movies`.
3. Create a second user if you want to test with two accounts.

### 2. Plugin configuration (optional)

Dashboard > Plugins > JellyWatchParty. The defaults work for local
development (the client connects to `ws://<page host>:3000/ws`). To test
authentication, set the same JWT secret (32+ characters) in the plugin and as
`JWT_SECRET` in `.env`, then run `just restart`.

### 3. Verify

1. Play any video.
2. Click the Watch Party button (app header or player controls).
3. The panel opens and lists rooms.

## Project Structure

```
JellyWatchParty/
├── src/
│   ├── clients/jellyfin-web/        # Injected JavaScript client
│   │   ├── plugin.js                # Loader (served at /JellyWatchParty/ClientScript)
│   │   ├── state.js                 # JWP.constants / JWP.state
│   │   ├── utils/  ui/  playback/  chat/  ws/  ws/handlers/  app/
│   │   └── tests/                   # node --test suites (*.test.js, setup.js)
│   ├── plugins/jellyfin/
│   │   ├── Directory.Build.props    # net10.0, Jellyfin 12.0.0 packages
│   │   ├── JellyWatchParty/         # C# plugin (JellyWatchPartyPlugin.csproj)
│   │   │   ├── Plugin.cs, ScriptInjectionMiddleware.cs, FileTransformationIntegration.cs
│   │   │   ├── Controllers/  Configuration/  Services/
│   │   │   └── Web/                 # configPage.html + JS copied from clients/ at build time
│   │   └── JellyWatchParty.Tests/   # xUnit tests
│   ├── server/                      # Rust session server (crate session-server)
│   │   ├── .cargo/config.toml       # mold linker, dev profile tuning
│   │   └── src/
│   │       ├── main.rs, routes.rs, auth.rs, messaging.rs, types.rs, tasks.rs, ...
│   │       ├── ws/                  # connection, dispatch, validation, handlers/
│   │       ├── room/                # ops, leave, close, reconnect
│   │       ├── admin/               # admin panel (API + embedded UI)
│   │       ├── jellyfin/            # Jellyfin API device bridge
│   │       └── integration/         # chat bot integration API
│   └── integrations/discord-bot/    # Rust chat bot, Discord + Telegram (crate jwp-discord-bot)
├── infra/
│   ├── docker/
│   │   ├── server.Dockerfile, discord-bot.Dockerfile
│   │   ├── dev/docker-compose.yml   # Dev stack (+ config/, gitignored Jellyfin config)
│   │   ├── dev/scripts/jellyfin-entrypoint.sh
│   │   └── prod/docker-compose.yml  # Published images (JWP_TAG)
│   └── just/                        # Just modules: build, test, lint, logs, clean, shell
├── .githooks/pre-commit             # cargo fmt --check for src/server
├── .github/workflows/               # ci, publish, docker-image, security, docs
├── docs/                            # This site (Jekyll)
└── justfile
```

## Commands

Run `just` for the full list (including modules).

### Development

| Command | Description |
|---------|-------------|
| `just up` (`u`) | Build File Transformation + plugin, start `session-server` and `jellyfin-dev` |
| `just down` (`d`) | Stop all services |
| `just dev` | `just up`, then follow logs |
| `just restart` | `up -d` (recreates containers whose config or `.env` changed), then restart both services |
| `just watch` | Restart Jellyfin whenever a file under `src/clients/jellyfin-web/` changes |
| `just status` (`s`) | `docker compose ps` plus health checks |
| `just setup` | `git config core.hooksPath .githooks` |

### Build (`just build ...`)

| Command | Description |
|---------|-------------|
| `just build` / `just build plugin` | Copy client JS, `dotnet publish` in Docker, write `dist/meta.json` |
| `just build ft` | Download the File Transformation plugin (version pinned in `infra/just/common.just`) |
| `just build server` | `cargo build --release` in `src/server` |
| `just build discord` | `cargo build --release` in `src/integrations/discord-bot` |
| `just build image` | Rebuild the `session-server` dev image |
| `just build all` | `ft` + `plugin` + `image` |
| `just rebuild` | `just clean`, `just build all`, then `up -d --force-recreate` |
| `just release` | Local zip: plugin + release server binary in `JellyWatchParty-release.zip` |

### Tests (`just test ...`)

| Command | Description |
|---------|-------------|
| `just test` | All four suites below |
| `just test server` | `cargo test` in `src/server` |
| `just test discord` | `cargo test` in `src/integrations/discord-bot` |
| `just test plugin` | `dotnet test` in `mcr.microsoft.com/dotnet/sdk:10.0` |
| `just test client` | `node --test src/clients/jellyfin-web/tests/*.test.js` |

### Lint and format

| Command | Description |
|---------|-------------|
| `just lint` | `server` + `discord` + `client` |
| `just lint server` | `cargo clippy -- -D warnings` (server) |
| `just lint discord` | `cargo clippy --all-targets -- -D warnings` (bot) |
| `just lint client` | eslint if installed (never fails the recipe) |
| `just fmt` | `cargo fmt` in `src/server` (the bot: run `cargo fmt` in its directory) |
| `just check` | `cargo check` in `src/server` |

### Logs, shells, cleanup

| Command | Description |
|---------|-------------|
| `just logs` / `logs server` / `logs jellyfin` | Follow logs (all, `session-server`, `jellyfin-dev`) |
| `just shell server` / `shell jellyfin` | Shell in `jwp-session-server` / `jellyfin-dev` |
| `just clean` | Plugin, server and File Transformation artifacts, `dist/`, release zip |
| `just clean plugin` / `server` / `ft` | One of the above |
| `just clean docker` | `compose down -v --rmi local` (removes volumes, including the dev session-server data) |
| `just reset` | `down` + `clean docker` + `clean` |

## Development Workflow

### JavaScript client

The plugin serves the client JS from resources embedded in its DLL, so an
edit only shows up after the plugin is rebuilt:

```bash
just build plugin && just restart
```

Then hard-refresh the browser (Ctrl+F5). `just watch` only restarts Jellyfin;
it does not rebuild the plugin.

### Rust session server

```bash
just build image && just restart
```

For faster iteration, run it on the host (`cargo watch` needs
`cargo install cargo-watch`) and stop the container first so port 3000 is free:

```bash
cd src/server
RUST_LOG=debug cargo watch -x run
```

### C# plugin

```bash
just build plugin && just restart
```

`just rebuild` rebuilds everything from scratch when in doubt.

## Debugging

### JavaScript (browser)

Open the developer tools and filter the console by `JWP` or `JellyWatchParty`.
Client logs (`[JWP:<category>]`) are also sent to the session server as
`client_log` messages and appear in `just logs server`.

```javascript
// The client lives on window.JellyWatchParty
const JWP = window.JellyWatchParty;
JWP.state;               // current state
JWP.state.ws?.readyState // 1 = open
JWP.state.rooms;         // last room_list
JWP.constants;           // timing / sync constants
```

### Rust (server)

Set `RUST_LOG=debug` in `.env` and run `just restart`, or prefix `cargo run`
with it when running on the host.

### C# (plugin)

```bash
just logs jellyfin
```

### Script not updating

1. Rebuild the plugin and restart (see above).
2. Hard-refresh (Ctrl+F5) or clear the browser cache.
3. Check that the ETag changed:
   ```bash
   curl -I http://localhost:8096/JellyWatchParty/ClientScript
   ```

For other problems see [Troubleshooting & FAQ]({{ '/troubleshooting/' | relative_url }}).

## Rust Build Configuration

### Docker build modes

Both Dockerfiles take a `BUILD_MODE` argument:

| Mode | Used by | Result |
|------|---------|--------|
| `dev` | Dev compose file; default of `server.Dockerfile` | Debug build, fast compile |
| `release` | Prod compose file, published images; default of `discord-bot.Dockerfile` | Optimized, stripped |

Both images build on `rust:1.88-alpine` with mold and run on `alpine:3.24`.

### Cargo configuration

`src/server/.cargo/config.toml`:

```toml
[build]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]

[profile.dev]
incremental = true
opt-level = 0

[profile.dev.package."*"]
opt-level = 2
```

mold is not optional here: without it, linking fails. Install it
(`pacman -S mold`, `apt install mold`, `brew install mold`), or override the
flags for one command; a set `RUSTFLAGS` takes precedence over
`build.rustflags`, e.g. `RUSTFLAGS="-C debuginfo=1" cargo test`. CI deletes
the file before building. The file is excluded from the Docker build context
(`.dockerignore`); the Dockerfiles set mold through `RUSTFLAGS` themselves.
The bot crate has no such file.

## Environment Variables

`.env` at the repository root is loaded by `just` and passed on to Compose.
Start from `.env.example`; every variable is optional. A minimal local setup:

```bash
JWT_SECRET=dev-secret-at-least-32-characters-long
ALLOWED_ORIGINS=http://localhost:8096
RUST_LOG=debug
```

See [Configuration]({{ '/configuration/' | relative_url }}) for the full list.

## Next Steps

- [Contributing]({{ '/development/contributing/' | relative_url }})
- [Testing]({{ '/development/testing/' | relative_url }})
- [Core Structure]({{ '/core-structure/' | relative_url }})
