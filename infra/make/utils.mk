# ============================================================================
# Utility Targets
# ============================================================================

.PHONY: info env urls tree

info: ## Show project information
	@echo ""
	@echo "$(BOLD)$(PROJECT_NAME)$(RESET)"
	@echo ""
	@echo "$(CYAN)Directories:$(RESET)"
	@echo "  Plugin:  $(PLUGIN_DIR)"
	@echo "  Client:  $(CLIENT_DIR)"
	@echo "  Server:  $(SERVER_DIR)"
	@echo ""
	@echo "$(CYAN)Docker:$(RESET)"
	@echo "  Compose: $(COMPOSE_FILE)"
	@echo ""
	@echo "$(CYAN)Git:$(RESET)"
	@echo "  Branch:  $$(git branch --show-current)"
	@echo "  Status:  $$(git status --short | wc -l) changes"
	@echo ""

env: ## Show environment variables used
	@echo "$(BOLD)Environment Variables:$(RESET)"
	@echo ""
	@echo "  JELLYFIN_PORT       = $${JELLYFIN_PORT:-8096} (default: 8096)"
	@echo "  SESSION_SERVER_PORT = $${SESSION_SERVER_PORT:-3000} (default: 3000)"
	@echo "  MEDIA_DIR           = $${MEDIA_DIR:-$$HOME/Videos/Movies}"
	@echo "  NO_COLOR            = $${NO_COLOR:-} (set to disable colors)"
	@echo ""

urls: ## Show service URLs
	@echo ""
	@echo "$(BOLD)Service URLs:$(RESET)"
	@echo ""
	@echo "  $(CYAN)Jellyfin Web:$(RESET)     http://localhost:$${JELLYFIN_PORT:-8096}"
	@echo "  $(CYAN)Session Server:$(RESET)   http://localhost:$${SESSION_SERVER_PORT:-3000}"
	@echo "  $(CYAN)Health Check:$(RESET)     http://localhost:$${SESSION_SERVER_PORT:-3000}/health"
	@echo "  $(CYAN)WebSocket:$(RESET)        ws://localhost:$${SESSION_SERVER_PORT:-3000}/ws"
	@echo "  $(CYAN)Plugin Repo:$(RESET)      https://mhbxyz.github.io/OpenWatchParty/jellyfin-plugin-repo/manifest.json"
	@echo ""

tree: ## Show project structure
	@echo "$(BOLD)Project Structure:$(RESET)"
	@tree -L 2 -I 'target|node_modules|dist|obj|bin|refs|*.dll' --dirsfirst 2>/dev/null || find . -maxdepth 2 -type d | grep -v -E '\.(git|cache)' | head -30
