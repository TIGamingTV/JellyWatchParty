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

[doc('Run .NET plugin tests')]
plugin:
    @echo -e "{{GREEN}}▶ Running plugin tests...{{RESET}}"
    @docker run --rm \
        -v "$(pwd)/src/plugins/jellyfin:/src" \
        -w /src \
        -u "$(id -u):$(id -g)" \
        -e HOME=/tmp \
        -e DOTNET_CLI_HOME=/tmp/.dotnet \
        mcr.microsoft.com/dotnet/sdk:9.0 \
        dotnet test JellyWatchParty.Tests/
    @echo -e "{{GREEN}}✓ Plugin tests passed{{RESET}}"

[doc('Run JavaScript client tests')]
client:
    @echo -e "{{GREEN}}▶ Running client tests...{{RESET}}"
    @node --test {{client_dir}}/tests/*.test.js
    @echo -e "{{GREEN}}✓ Client tests passed{{RESET}}"
