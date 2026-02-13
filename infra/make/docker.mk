# ============================================================================
# Docker & Observability Targets
# ============================================================================

.PHONY: logs logs-server logs-jellyfin status ps health top stats connections

logs: ## Follow logs from all services
	@$(COMPOSE) logs -f --tail=100

logs-server: ## Follow session server logs only
	@$(COMPOSE) logs -f --tail=100 session-server

logs-jellyfin: ## Follow Jellyfin logs only
	@$(COMPOSE) logs -f --tail=100 jellyfin-dev

status: ## Show service status with health info
	@echo "$(BOLD)$(CYAN)Service Status:$(RESET)"
	@echo ""
	@$(COMPOSE) ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}"
	@echo ""
	@$(MAKE) -s health

ps: status ## Alias for status

health: ## Check health of all services
	@echo "$(BOLD)$(CYAN)Health Checks:$(RESET)"
	@echo ""
	@printf "  Session Server: "
	@curl -sf http://localhost:3000/health > /dev/null 2>&1 && echo "$(GREEN)✓ healthy$(RESET)" || echo "$(RED)✗ unhealthy$(RESET)"
	@printf "  Jellyfin:       "
	@curl -sf http://localhost:8096/health > /dev/null 2>&1 && echo "$(GREEN)✓ healthy$(RESET)" || echo "$(RED)✗ unhealthy$(RESET)"
	@printf "  WebSocket:      "
	@timeout 2 bash -c 'echo "" | nc -w1 localhost 3000' > /dev/null 2>&1 && echo "$(GREEN)✓ reachable$(RESET)" || echo "$(YELLOW)? check manually$(RESET)"
	@echo ""

top: ## Show running processes in containers
	@echo "$(BOLD)$(CYAN)Container Processes:$(RESET)"
	@echo ""
	@echo "$(YELLOW)── Session Server ──$(RESET)"
	@docker top $(SESSION_CTR) 2>/dev/null || echo "  (not running)"
	@echo ""
	@echo "$(YELLOW)── Jellyfin ──$(RESET)"
	@docker top $(JELLYFIN_CTR) 2>/dev/null || echo "  (not running)"

stats: ## Show container resource usage (CPU/mem)
	@docker stats --no-stream --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}" $(SESSION_CTR) $(JELLYFIN_CTR) 2>/dev/null || echo "Containers not running"

connections: ## Show active WebSocket connections
	@echo "$(CYAN)Active connections on port 3000:$(RESET)"
	@ss -tn state established '( sport = :3000 )' 2>/dev/null || netstat -tn 2>/dev/null | grep ":3000.*ESTABLISHED" || echo "  No active connections"
