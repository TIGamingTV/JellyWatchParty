# ============================================================================
# Lint module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Run all linters (Rust + JS)')]
default: server discord client

[doc('Lint Rust code with clippy')]
server:
    @echo -e "{{CYAN}}▶ Linting Rust code...{{RESET}}"
    @cd {{server_dir}} && cargo clippy -- -D warnings
    @echo -e "{{GREEN}}✓ Rust lint passed{{RESET}}"

[doc('Lint the Discord bot with clippy')]
discord:
    @echo -e "{{CYAN}}▶ Linting the Discord bot...{{RESET}}"
    @cd {{discord_dir}} && cargo clippy --all-targets -- -D warnings
    @echo -e "{{GREEN}}✓ Discord bot lint passed{{RESET}}"

[doc('Lint JavaScript code (requires eslint)')]
client:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -e "{{CYAN}}▶ Linting JavaScript...{{RESET}}"
    if command -v eslint &> /dev/null; then
        eslint {{client_dir}}/**/*.js {{client_dir}}/*.js || true
    else
        echo -e "{{YELLOW}}⚠ eslint not installed, skipping{{RESET}}"
    fi
