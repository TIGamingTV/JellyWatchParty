# ============================================================================
# Development Targets
# ============================================================================

.PHONY: up down restart dev watch restart-jellyfin restart-server shell-jellyfin shell-server

up: build-plugin ## Start the full stack (Jellyfin + session server)
	@echo "$(GREEN)▶ Starting services...$(RESET)"
	@$(COMPOSE) up -d session-server jellyfin-dev
	@echo "$(GREEN)✓ Stack started$(RESET)"
	@echo ""
	@echo "  Jellyfin:  $(CYAN)http://localhost:8096$(RESET)"
	@echo "  WebSocket: $(CYAN)ws://localhost:3000/ws$(RESET)"
	@echo ""

down: ## Stop all services
	@echo "$(YELLOW)▶ Stopping services...$(RESET)"
	@$(COMPOSE) down
	@echo "$(GREEN)✓ Services stopped$(RESET)"

restart: ## Restart all services
	@echo "$(YELLOW)▶ Restarting services...$(RESET)"
	@$(COMPOSE) restart session-server jellyfin-dev
	@echo "$(GREEN)✓ Services restarted$(RESET)"

restart-jellyfin: ## Restart Jellyfin only (after JS changes)
	@echo "$(YELLOW)▶ Restarting Jellyfin...$(RESET)"
	@$(COMPOSE) restart jellyfin-dev
	@echo "$(GREEN)✓ Jellyfin restarted$(RESET)"

restart-server: build-server-docker ## Rebuild and restart session server
	@echo "$(YELLOW)▶ Restarting session server...$(RESET)"
	@$(COMPOSE) up -d --force-recreate session-server
	@echo "$(GREEN)✓ Session server restarted$(RESET)"

dev: up logs ## Start stack and follow logs

watch: ## Watch client JS files and auto-restart Jellyfin on change
	@echo "$(CYAN)▶ Watching $(CLIENT_DIR) for changes...$(RESET)"
	@echo "  Press Ctrl+C to stop"
	@while true; do \
		inotifywait -q -e modify -e create -e delete $(CLIENT_DIR)/*.js 2>/dev/null || fswatch -1 $(CLIENT_DIR)/*.js 2>/dev/null || sleep 5; \
		echo "$(YELLOW)▶ Change detected, restarting Jellyfin...$(RESET)"; \
		$(COMPOSE) restart jellyfin-dev; \
		echo "$(GREEN)✓ Restarted$(RESET)"; \
	done

shell-jellyfin: ## Open shell in Jellyfin container
	@docker exec -it $(JELLYFIN_CTR) /bin/bash

shell-server: ## Open shell in session server container
	@docker exec -it $(SESSION_CTR) /bin/sh

# Quick aliases
.PHONY: u d r l s b
u: up
d: down
r: restart
l: logs
s: status
b: build
