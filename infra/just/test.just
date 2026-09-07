# ============================================================================
# Test module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Run all tests (Rust + .NET + JS)')]
default: server plugin client

[doc('Run Rust server tests')]
server:
    @echo -e "{{GREEN}}▶ Running server tests...{{RESET}}"
    @cd {{server_dir}} && cargo test
    @echo -e "{{GREEN}}✓ Tests passed{{RESET}}"

[doc('Run .NET plugin tests (net9.0/Jellyfin 10.11 leg only - see below)')]
plugin:
    @echo -e "{{GREEN}}▶ Running plugin tests...{{RESET}}"
    # The test project multi-targets net9.0 and net10.0 (Jellyfin 10.11.x and
    # 12.x - see docs/jellyfin-12-migration.md), but this image only has the
    # .NET 9 SDK/runtime, so run just the net9.0 leg locally. CI
    # (.github/workflows/ci.yml) installs both SDKs and runs the full suite
    # against both frameworks on every push.
    @docker run --rm \
        -v "$(pwd)/src/plugins/jellyfin:/src" \
        -w /src \
        -u "$(id -u):$(id -g)" \
        -e HOME=/tmp \
        -e DOTNET_CLI_HOME=/tmp/.dotnet \
        mcr.microsoft.com/dotnet/sdk:9.0 \
        dotnet test JellyWatchParty.Tests/ -f net9.0
    @echo -e "{{GREEN}}✓ Plugin tests passed{{RESET}}"

[doc('Run JavaScript client tests')]
client:
    @echo -e "{{GREEN}}▶ Running client tests...{{RESET}}"
    @node --test {{client_dir}}/tests/*.test.js
    @echo -e "{{GREEN}}✓ Client tests passed{{RESET}}"
