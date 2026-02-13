# ============================================================================
# Docker & Observability
# ============================================================================

[group('observability')]
[doc('Follow logs from all services')]
logs:
    @{{compose}} logs -f --tail=100

[group('observability')]
[doc('Follow session server logs only')]
logs-server:
    @{{compose}} logs -f --tail=100 session-server

[group('observability')]
[doc('Follow Jellyfin logs only')]
logs-jellyfin:
    @{{compose}} logs -f --tail=100 jellyfin-dev

[group('observability')]
[doc('Show service status with health info')]
status:
    @echo -e "{{BOLD}}{{CYAN}}Service Status:{{RESET}}"
    @echo ""
    @{{compose}} ps --format "table {{{{.Name}}}}\t{{{{.Status}}}}\t{{{{.Ports}}}}"
    @echo ""
    @just health

[group('observability')]
[doc('Alias for status')]
ps: status

[group('observability')]
[doc('Check health of all services')]
health:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -e "{{BOLD}}{{CYAN}}Health Checks:{{RESET}}"
    echo ""
    printf "  Session Server: "
    curl -sf http://localhost:3000/health > /dev/null 2>&1 \
        && echo -e "{{GREEN}}✓ healthy{{RESET}}" \
        || echo -e "{{RED}}✗ unhealthy{{RESET}}"
    printf "  Jellyfin:       "
    curl -sf http://localhost:8096/health > /dev/null 2>&1 \
        && echo -e "{{GREEN}}✓ healthy{{RESET}}" \
        || echo -e "{{RED}}✗ unhealthy{{RESET}}"
    printf "  WebSocket:      "
    timeout 2 bash -c 'echo "" | nc -w1 localhost 3000' > /dev/null 2>&1 \
        && echo -e "{{GREEN}}✓ reachable{{RESET}}" \
        || echo -e "{{YELLOW}}? check manually{{RESET}}"
    echo ""

[group('observability')]
[doc('Show running processes in containers')]
top:
    @echo -e "{{BOLD}}{{CYAN}}Container Processes:{{RESET}}"
    @echo ""
    @echo -e "{{YELLOW}}── Session Server ──{{RESET}}"
    @docker top {{session_ctr}} 2>/dev/null || echo "  (not running)"
    @echo ""
    @echo -e "{{YELLOW}}── Jellyfin ──{{RESET}}"
    @docker top {{jellyfin_ctr}} 2>/dev/null || echo "  (not running)"

[group('observability')]
[doc('Show container resource usage (CPU/mem)')]
stats:
    @docker stats --no-stream --format "table {{{{.Name}}}}\t{{{{.CPUPerc}}}}\t{{{{.MemUsage}}}}\t{{{{.NetIO}}}}" {{session_ctr}} {{jellyfin_ctr}} 2>/dev/null || echo "Containers not running"

[group('observability')]
[doc('Show active WebSocket connections')]
connections:
    @echo -e "{{CYAN}}Active connections on port 3000:{{RESET}}"
    @ss -tn state established '( sport = :3000 )' 2>/dev/null || netstat -tn 2>/dev/null | grep ":3000.*ESTABLISHED" || echo "  No active connections"
