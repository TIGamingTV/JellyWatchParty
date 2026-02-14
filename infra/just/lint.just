# ============================================================================
# Lint module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Run all linters (Rust + JS)')]
default: server client

[doc('Lint Rust code with clippy')]
server:
    @echo -e "{{CYAN}}▶ Linting Rust code...{{RESET}}"
    @cd {{server_dir}} && cargo clippy -- -D warnings
    @echo -e "{{GREEN}}✓ Rust lint passed{{RESET}}"

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
