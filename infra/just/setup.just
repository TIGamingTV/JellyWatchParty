# ============================================================================
# Setup & Cleanup
# ============================================================================

[group('setup')]
[doc('Setup development environment')]
setup: setup-hooks
    @echo -e "{{GREEN}}✓ Development environment ready{{RESET}}"

[group('setup')]
[doc('Install pre-commit hooks')]
setup-hooks:
    #!/usr/bin/env bash
    set -euo pipefail
    echo -e "{{CYAN}}▶ Installing pre-commit hooks...{{RESET}}"
    if command -v pre-commit &> /dev/null; then
        pre-commit install
        pre-commit install --hook-type pre-push
        echo -e "{{GREEN}}✓ Hooks installed{{RESET}}"
    else
        echo -e "{{YELLOW}}⚠ pre-commit not found. Install with: pip install pre-commit{{RESET}}"
        exit 1
    fi

[group('setup')]
[doc('Clean all build artifacts')]
clean: clean-plugin clean-server
    @rm -rf dist {{project_name}}-release.zip
    @echo -e "{{GREEN}}✓ Cleaned{{RESET}}"

[group('setup')]
[doc('Clean plugin build artifacts')]
clean-plugin:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -e "{{YELLOW}}▶ Cleaning plugin artifacts...{{RESET}}"
    rm -rf {{plugin_dir}}/dist {{plugin_dir}}/bin {{plugin_dir}}/obj 2>/dev/null \
        || (echo -e "{{YELLOW}}⚠ Some files owned by root, using sudo...{{RESET}}" \
            && sudo rm -rf {{plugin_dir}}/dist {{plugin_dir}}/bin {{plugin_dir}}/obj)
    rm -f {{plugin_dir}}/Web/*.js 2>/dev/null || true

[group('setup')]
[doc('Clean server build artifacts')]
clean-server:
    @echo -e "{{YELLOW}}▶ Cleaning server artifacts...{{RESET}}"
    @rm -rf {{server_dir}}/target

[group('setup')]
[doc('Remove Docker images and volumes')]
clean-docker:
    @echo -e "{{RED}}▶ Removing Docker resources...{{RESET}}"
    @{{compose}} down -v --rmi local
    @echo -e "{{GREEN}}✓ Docker resources removed{{RESET}}"

[group('setup')]
[doc('Full reset (stop, remove containers/volumes, clean artifacts)')]
reset: down clean-docker clean
    @echo -e "{{GREEN}}✓ Full reset complete{{RESET}}"

[group('setup')]
[doc('Remove unused Docker resources (system-wide)')]
prune:
    @echo -e "{{RED}}▶ Pruning Docker system...{{RESET}}"
    @docker system prune -f
    @echo -e "{{GREEN}}✓ Docker pruned{{RESET}}"

[group('setup')]
[doc('Fix ownership of Docker-created files')]
fix-permissions:
    #!/usr/bin/env bash
    set -euo pipefail
    echo -e "{{YELLOW}}▶ Fixing file permissions...{{RESET}}"
    if [ -d "{{plugin_dir}}/dist" ] && [ "$(stat -c '%u' {{plugin_dir}}/dist 2>/dev/null)" != "{{UID}}" ]; then
        echo "  Fixing {{plugin_dir}}/dist..."
        sudo chown -R {{UID}}:{{GID}} {{plugin_dir}}/dist
    fi
    if [ -d "{{plugin_dir}}/obj" ] && [ "$(stat -c '%u' {{plugin_dir}}/obj 2>/dev/null)" != "{{UID}}" ]; then
        echo "  Fixing {{plugin_dir}}/obj..."
        sudo chown -R {{UID}}:{{GID}} {{plugin_dir}}/obj
    fi
    echo -e "{{GREEN}}✓ Permissions fixed{{RESET}}"
