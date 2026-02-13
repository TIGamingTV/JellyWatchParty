# ============================================================================
# Development
# ============================================================================

[group('development')]
[doc('Start the full stack (Jellyfin + session server)')]
up: build-plugin
    @echo -e "{{GREEN}}▶ Starting services...{{RESET}}"
    @{{compose}} up -d session-server jellyfin-dev
    @echo -e "{{GREEN}}✓ Stack started{{RESET}}"
    @echo ""
    @echo -e "  Jellyfin:  {{CYAN}}http://localhost:8096{{RESET}}"
    @echo -e "  WebSocket: {{CYAN}}ws://localhost:3000/ws{{RESET}}"
    @echo ""

[group('development')]
[doc('Stop all services')]
down:
    @echo -e "{{YELLOW}}▶ Stopping services...{{RESET}}"
    @{{compose}} down
    @echo -e "{{GREEN}}✓ Services stopped{{RESET}}"

[group('development')]
[doc('Restart all services')]
restart:
    @echo -e "{{YELLOW}}▶ Restarting services...{{RESET}}"
    @{{compose}} restart session-server jellyfin-dev
    @echo -e "{{GREEN}}✓ Services restarted{{RESET}}"

[group('development')]
[doc('Restart Jellyfin only (after JS changes)')]
restart-jellyfin:
    @echo -e "{{YELLOW}}▶ Restarting Jellyfin...{{RESET}}"
    @{{compose}} restart jellyfin-dev
    @echo -e "{{GREEN}}✓ Jellyfin restarted{{RESET}}"

[group('development')]
[doc('Rebuild and restart session server')]
restart-server: build-server-docker
    @echo -e "{{YELLOW}}▶ Restarting session server...{{RESET}}"
    @{{compose}} up -d --force-recreate session-server
    @echo -e "{{GREEN}}✓ Session server restarted{{RESET}}"

[group('development')]
[doc('Start stack and follow logs')]
dev: up
    @{{compose}} logs -f --tail=100

[group('development')]
[doc('Watch client JS files and auto-restart Jellyfin on change')]
watch:
    #!/usr/bin/env bash
    set -euo pipefail
    echo -e "{{CYAN}}▶ Watching {{client_dir}} for changes...{{RESET}}"
    echo "  Press Ctrl+C to stop"
    while true; do
        inotifywait -q -e modify -e create -e delete {{client_dir}}/*.js 2>/dev/null \
            || fswatch -1 {{client_dir}}/*.js 2>/dev/null \
            || sleep 5
        echo -e "{{YELLOW}}▶ Change detected, restarting Jellyfin...{{RESET}}"
        {{compose}} restart jellyfin-dev
        echo -e "{{GREEN}}✓ Restarted{{RESET}}"
    done

[group('development')]
[doc('Open shell in Jellyfin container')]
shell-jellyfin:
    @docker exec -it {{jellyfin_ctr}} /bin/bash

[group('development')]
[doc('Open shell in session server container')]
shell-server:
    @docker exec -it {{session_ctr}} /bin/sh
