# ============================================================================
# Utilities
# ============================================================================

[group('utilities')]
[doc('Show project information')]
info:
    #!/usr/bin/env bash
    set -euo pipefail
    echo ""
    echo -e "{{BOLD}}{{project_name}}{{RESET}}"
    echo ""
    echo -e "{{CYAN}}Directories:{{RESET}}"
    echo "  Plugin:  {{plugin_dir}}"
    echo "  Client:  {{client_dir}}"
    echo "  Server:  {{server_dir}}"
    echo ""
    echo -e "{{CYAN}}Docker:{{RESET}}"
    echo "  Compose: {{compose_file}}"
    echo ""
    echo -e "{{CYAN}}Git:{{RESET}}"
    echo "  Branch:  $(git branch --show-current)"
    echo "  Status:  $(git status --short | wc -l) changes"
    echo ""

[group('utilities')]
[doc('Show environment variables used')]
env:
    @echo -e "{{BOLD}}Environment Variables:{{RESET}}"
    @echo ""
    @echo "  JELLYFIN_PORT       = ${JELLYFIN_PORT:-8096} (default: 8096)"
    @echo "  SESSION_SERVER_PORT = ${SESSION_SERVER_PORT:-3000} (default: 3000)"
    @echo "  MEDIA_DIR           = ${MEDIA_DIR:-$HOME/Videos/Movies}"
    @echo "  NO_COLOR            = ${NO_COLOR:-} (set to disable colors)"
    @echo ""

[group('utilities')]
[doc('Show service URLs')]
urls:
    @echo ""
    @echo -e "{{BOLD}}Service URLs:{{RESET}}"
    @echo ""
    @echo -e "  {{CYAN}}Jellyfin Web:{{RESET}}     http://localhost:${JELLYFIN_PORT:-8096}"
    @echo -e "  {{CYAN}}Session Server:{{RESET}}   http://localhost:${SESSION_SERVER_PORT:-3000}"
    @echo -e "  {{CYAN}}Health Check:{{RESET}}     http://localhost:${SESSION_SERVER_PORT:-3000}/health"
    @echo -e "  {{CYAN}}WebSocket:{{RESET}}        ws://localhost:${SESSION_SERVER_PORT:-3000}/ws"
    @echo -e "  {{CYAN}}Plugin Repo:{{RESET}}      https://mhbxyz.github.io/OpenWatchParty/jellyfin-plugin-repo/manifest.json"
    @echo ""

[group('utilities')]
[doc('Show project structure')]
tree:
    @echo -e "{{BOLD}}Project Structure:{{RESET}}"
    @tree -L 2 -I 'target|node_modules|dist|obj|bin|refs|*.dll' --dirsfirst 2>/dev/null || find . -maxdepth 2 -type d | grep -v -E '\.(git|cache)' | head -30
