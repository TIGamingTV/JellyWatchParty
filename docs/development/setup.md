---
title: Setup
parent: Development
nav_order: 1
---

# Development Setup

## Prerequisites

- **Docker** and **Docker Compose**
- **Make**
- **.NET 9.0 SDK** (for plugin development)
- **Rust 1.83+** (for server development)
- **Node.js 20+** (optional, for JS tooling)
- **pre-commit** (for code quality hooks)
- **mold** (recommended, for faster Rust linking)

## Quick Start

```bash
# Clone the repository
git clone https://github.com/mhbxyz/OpenWatchParty.git
cd OpenWatchParty

# Set up development tools (pre-commit hooks)
make setup

# Start development environment
make up
```

This will:
1. Start Jellyfin on `http://localhost:8096`
2. Build and mount the plugin
3. Start the Rust session server on `http://localhost:3000`
4. Auto-inject the client script into Jellyfin's `index.html`

## First-Time Setup

### 1. Jellyfin Configuration

After running `make up`:

1. Open `http://localhost:8096`
2. Complete the Jellyfin setup wizard
3. Add a media library (can use sample media)
4. Create a user account

### 2. Plugin Configuration (Optional)

1. Go to Dashboard > Plugins > OpenWatchParty
2. Configure JWT Secret if testing authentication
3. Save and restart Jellyfin

### 3. Verify Installation

1. Play any video
2. Look for the Watch Party button in the header
3. Click to open the panel

## Project Structure

```
OpenWatchParty/
├── src/
│   ├── clients/
│   │   └── jellyfin-web/          # JavaScript client modules
│   │       ├── plugin.js        # Loader/entry point
│   │       ├── state.js     # State management
│   │       ├── utils.js     # Utilities
│   │       ├── ui.js        # User interface
│   │       ├── playback.js  # Video sync
│   │       ├── ws.js        # WebSocket
│   │       └── app.js       # Initialization
│   │
│   ├── plugins/
│   │   └── jellyfin/
│   │       └── OpenWatchParty/  # C# Jellyfin plugin
│   │           ├── Plugin.cs
│   │           ├── Controllers/
│   │           ├── Configuration/
│   │           └── Web/         # Bundled JS (copied from clients/)
│   │
│   └── server/     # Rust WebSocket server
│       ├── src/
│       │   ├── main.rs
│       │   ├── types.rs
│       │   ├── ws.rs
│       │   ├── room.rs
│       │   ├── messaging.rs
│       │   └── auth.rs
│       └── Cargo.toml
│
├── infra/
│   ├── docker/              # Docker configuration
│   │   ├── server.Dockerfile
│   │   ├── config/          # Jellyfin runtime config (gitignored)
│   │   ├── dev/
│   │   │   └── docker-compose.yml   # Dev environment
│   │   └── prod/
│   │       └── docker-compose.yml   # Prod / release builds
│   ├── make/                # Makefile modules
│   │   ├── config.mk
│   │   ├── dev.mk
│   │   ├── build.mk
│   │   ├── test.mk
│   │   ├── docker.mk
│   │   ├── setup.mk
│   │   ├── utils.mk
│   │   └── help.mk
│   └── scripts/             # Utility scripts
│
├── docs/                    # Documentation
│
├── Makefile                 # Build automation
├── CLAUDE.md               # AI assistant context
└── README.md               # Project overview
```

## Make Commands

Run `make help` for a full list. Key commands:

### Development
| Command | Description |
|---------|-------------|
| `make up` | Start full development environment |
| `make down` | Stop all services |
| `make dev` | Start stack and follow logs |
| `make restart` | Restart all services |
| `make restart-jellyfin` | Restart Jellyfin only (after JS changes) |
| `make restart-server` | Rebuild and restart session server |
| `make watch` | Watch JS files and auto-restart on change |
| `make shell-jellyfin` | Open shell in Jellyfin container |
| `make shell-server` | Open shell in session server container |

### Build
| Command | Description |
|---------|-------------|
| `make build` | Build the Jellyfin plugin |
| `make build-server` | Build the session server locally (Rust) |
| `make build-server-docker` | Rebuild session server Docker image |
| `make build-all` | Build everything (plugin + server image) |
| `make rebuild` | Clean and rebuild everything |
| `make release` | Build release artifacts (zip) |
| `make release-image` | Build release Docker image (prod) |

### Observability
| Command | Description |
|---------|-------------|
| `make logs` | Follow logs from all services |
| `make logs-server` | Follow session server logs only |
| `make logs-jellyfin` | Follow Jellyfin logs only |
| `make status` | Show service status with health info |
| `make health` | Check health of all services |

### Testing & Quality
| Command | Description |
|---------|-------------|
| `make test` | Run all tests |
| `make lint` | Run all linters (Rust + JS) |
| `make fmt` | Format all code |
| `make check` | Run cargo check (fast compile check) |
| `make pre-commit` | Run all pre-commit hooks |
| `make setup` | Install pre-commit hooks |

### Cleanup
| Command | Description |
|---------|-------------|
| `make clean` | Clean all build artifacts |
| `make clean-docker` | Remove Docker images and volumes |
| `make reset` | Full reset (containers + artifacts) |

**Quick aliases:** `u`=up, `d`=down, `r`=restart, `l`=logs, `s`=status, `b`=build

## Pre-commit Hooks

The project uses [pre-commit](https://pre-commit.com/) to ensure code quality before commits. Hooks run automatically on `git commit`.

### Installation

```bash
# Install pre-commit (if not already installed)
pip install pre-commit
# or: brew install pre-commit

# Install the hooks
make setup
```

### What Gets Checked

| Hook | Runs On | Description |
|------|---------|-------------|
| `cargo fmt` | Rust files | Code formatting |
| `cargo clippy` | Rust files | Linting and warnings |
| `cargo test` | Push only | Unit tests |
| `dotnet build` | C# files | Build validation |
| `dotnet test` | Push only | Unit tests |
| `node --check` | JS files | Syntax validation |
| `trailing-whitespace` | All files | Remove trailing spaces |
| `end-of-file-fixer` | All files | Ensure newline at EOF |
| `check-yaml` | YAML files | Syntax validation |
| `check-json` | JSON files | Syntax validation |
| `detect-private-key` | All files | Prevent accidental key commits |
| `hadolint` | Dockerfiles | Dockerfile linting |

### Manual Execution

```bash
# Run all hooks on staged files
pre-commit run

# Run all hooks on all files
pre-commit run --all-files

# Run specific hook
pre-commit run cargo-fmt --all-files

# Skip hooks temporarily (not recommended)
git commit --no-verify
```

### Troubleshooting Hooks

If a hook fails:

1. **Formatting issues**: Run `make fmt` and re-stage files
2. **Clippy warnings**: Fix the warnings or add `#[allow(...)]` if justified
3. **Build failures**: Check error messages and fix compilation issues
4. **Whitespace issues**: Hooks auto-fix these; re-stage the files

## Development Workflow

### JavaScript Client

1. **Edit files** in `src/clients/jellyfin-web/`
2. **Restart Jellyfin** (automatically copies JS files):
   ```bash
   make restart-jellyfin
   ```
3. **Hard refresh browser** (Ctrl+F5)

**Tip:** Use `make watch` to automatically restart Jellyfin when JS files change.

### Rust Session Server

1. **Edit files** in `src/server/src/`
2. **Restart server** (rebuilds automatically):
   ```bash
   make restart-server
   ```

### C# Plugin

1. **Edit files** in `src/plugins/jellyfin/OpenWatchParty/`
2. **Build and restart**:
   ```bash
   make build && make restart-jellyfin
   ```

## Hot Reload

### JavaScript

Use `make watch` for automatic reload on JS file changes. Otherwise:
1. Run `make restart-jellyfin`
2. Hard refresh browser (Ctrl+F5)

### Rust

The session server needs restart after changes:
```bash
make restart-server
```

For faster iteration, run locally:
```bash
cd src/server
cargo watch -x run
```

### C# Plugin

Requires rebuilding and restarting Jellyfin:
```bash
make build && make restart-jellyfin
```

## Debugging

### JavaScript (Browser)

1. Open Developer Tools (F12)
2. Go to Console tab
3. Filter by "OWP" or "OSP"
4. Set breakpoints in Sources tab

**Useful console commands:**
```javascript
// View current state
console.log(OSP.state);

// Check WebSocket connection
console.log(OSP.state.ws?.readyState);

// View rooms
console.log(OSP.state.rooms);
```

### Rust (Server)

Enable debug logging:
```yaml
# docker-compose.yml
environment:
  - LOG_LEVEL=debug
```

Or use RUST_LOG:
```bash
RUST_LOG=debug cargo run
```

### C# (Plugin)

Check Jellyfin logs:
```bash
docker logs jellyfin-dev
```

Or enable debug logging in Jellyfin settings.

## Testing Changes

### Manual Testing

1. Open Jellyfin in two browser windows
2. Play the same video in both
3. Create a room in one window
4. Join from the other window
5. Test sync functionality

### Sync Testing

Things to test:
- [ ] Room creation
- [ ] Room joining
- [ ] Play/pause sync
- [ ] Seek sync
- [ ] Drift correction (watch for 5+ minutes)
- [ ] Disconnect/reconnect
- [ ] Host leaving

## Common Development Issues

### Plugin Not Loading

```bash
# Check plugin is mounted correctly
docker exec jellyfin-dev ls /config/plugins/

# Check plugin logs
docker logs jellyfin-dev | grep OpenWatchParty
```

### Script Not Updating

1. Clear browser cache (Ctrl+Shift+Delete)
2. Hard refresh (Ctrl+F5)
3. Check ETag is changing:
   ```bash
   curl -I http://localhost:8096/OpenWatchParty/ClientScript
   ```

### WebSocket Connection Issues

```bash
# Check session server is running
curl http://localhost:3000/health

# Check WebSocket endpoint
wscat -c ws://localhost:3000/ws
```

### Build Errors

**Rust:**
```bash
cd src/server
cargo clean
cargo build
```

**C#:**
```bash
cd src/plugins/jellyfin/OpenWatchParty
dotnet clean
dotnet build
```

## Build Optimization (Rust)

The Rust server has optimized build configuration for faster development cycles.

### Docker Build Modes

The Dockerfile supports a `BUILD_MODE` argument:

| Mode | Usage | Optimization |
|------|-------|--------------|
| `dev` | Local development (`docker-compose.yml`) | Fast builds, debug symbols |
| `release` | CI/CD and production | Full optimization, smaller binary |

Development builds use `BUILD_MODE=dev` by default. CI releases use `BUILD_MODE=release`.

### Mold Linker (Recommended)

Install the `mold` linker for 5-10x faster linking:

```bash
# Arch Linux / Manjaro
sudo pacman -S mold

# Ubuntu/Debian
sudo apt install mold

# macOS (via Homebrew)
brew install mold
```

The project's `.cargo/config.toml` automatically uses mold when available.

### Cargo Configuration

Located in `src/server/.cargo/config.toml`:

```toml
[build]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]

[profile.dev]
incremental = true
opt-level = 0

[profile.dev.package."*"]
opt-level = 2  # Optimize dependencies (they rarely change)
```

### Tokio Features

The server uses minimal tokio features to reduce compile times:

```toml
# Only what's needed (instead of "full")
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "signal"] }
```

### Build Times

| Build Type | Without Optimization | With Optimization |
|------------|---------------------|-------------------|
| Clean build | ~4-5 min | ~2-3 min |
| Incremental rebuild | ~15-20s | ~2-3s |

## IDE Setup

### VS Code

Recommended extensions:
- **rust-analyzer** - Rust support
- **C# Dev Kit** - C# support
- **ESLint** - JavaScript linting
- **Docker** - Docker support

`.vscode/settings.json`:
```json
{
  "rust-analyzer.cargo.buildScripts.enable": true,
  "editor.formatOnSave": true
}
```

### JetBrains

- **RustRover** for Rust
- **Rider** for C#

## Environment Variables

For local development, create `.env` file:

```bash
# .env
JWT_SECRET=dev-secret-at-least-32-characters-long
ALLOWED_ORIGINS=http://localhost:8096
LOG_LEVEL=debug
```

## Next Steps

- [Contributing](contributing.md) - How to contribute
- [Testing](testing.md) - Running tests
- [Architecture](../technical/architecture.md) - System design
