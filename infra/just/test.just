# ============================================================================
# Testing & Quality
# ============================================================================

[group('testing')]
[doc('Run all tests')]
test: test-server

[group('testing')]
[doc('Run Rust server tests')]
test-server:
    @echo -e "{{GREEN}}▶ Running server tests...{{RESET}}"
    @cd {{server_dir}} && cargo test
    @echo -e "{{GREEN}}✓ Tests passed{{RESET}}"

[group('testing')]
[doc('Run all linters (Rust + JS)')]
lint: lint-server lint-client

[group('testing')]
[doc('Lint Rust code with clippy')]
lint-server:
    @echo -e "{{CYAN}}▶ Linting Rust code...{{RESET}}"
    @cd {{server_dir}} && cargo clippy -- -D warnings
    @echo -e "{{GREEN}}✓ Rust lint passed{{RESET}}"

[group('testing')]
[doc('Lint JavaScript code (requires eslint)')]
lint-client:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -e "{{CYAN}}▶ Linting JavaScript...{{RESET}}"
    if command -v eslint &> /dev/null; then
        eslint {{client_dir}}/*.js || true
    else
        echo -e "{{YELLOW}}⚠ eslint not installed, skipping{{RESET}}"
    fi

[group('testing')]
[doc('Format all code')]
fmt: fmt-server

[group('testing')]
[doc('Format Rust code')]
fmt-server:
    @echo -e "{{CYAN}}▶ Formatting Rust code...{{RESET}}"
    @cd {{server_dir}} && cargo fmt
    @echo -e "{{GREEN}}✓ Code formatted{{RESET}}"

[group('testing')]
[doc('Run cargo check (fast compile check)')]
check:
    @echo -e "{{CYAN}}▶ Running cargo check...{{RESET}}"
    @cd {{server_dir}} && cargo check
    @echo -e "{{GREEN}}✓ Check passed{{RESET}}"

[group('testing')]
[doc('Run all pre-commit hooks manually')]
pre-commit:
    @echo -e "{{CYAN}}▶ Running pre-commit hooks...{{RESET}}"
    @pre-commit run --all-files
