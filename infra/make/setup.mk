# ============================================================================
# Setup & Cleanup Targets
# ============================================================================

.PHONY: setup setup-hooks clean clean-plugin clean-server clean-docker reset prune fix-permissions

setup: setup-hooks ## Setup development environment
	@echo "$(GREEN)✓ Development environment ready$(RESET)"

setup-hooks: ## Install pre-commit hooks
	@echo "$(CYAN)▶ Installing pre-commit hooks...$(RESET)"
	@if command -v pre-commit &> /dev/null; then \
		pre-commit install; \
		pre-commit install --hook-type pre-push; \
		echo "$(GREEN)✓ Hooks installed$(RESET)"; \
	else \
		echo "$(YELLOW)⚠ pre-commit not found. Install with: pip install pre-commit$(RESET)"; \
		exit 1; \
	fi

clean: clean-plugin clean-server ## Clean all build artifacts
	@rm -rf dist $(PROJECT_NAME)-release.zip
	@echo "$(GREEN)✓ Cleaned$(RESET)"

clean-plugin: ## Clean plugin build artifacts
	@echo "$(YELLOW)▶ Cleaning plugin artifacts...$(RESET)"
	@rm -rf $(PLUGIN_DIR)/dist $(PLUGIN_DIR)/bin $(PLUGIN_DIR)/obj 2>/dev/null || \
		(echo "$(YELLOW)⚠ Some files owned by root, using sudo...$(RESET)" && \
		 sudo rm -rf $(PLUGIN_DIR)/dist $(PLUGIN_DIR)/bin $(PLUGIN_DIR)/obj)
	@rm -f $(PLUGIN_DIR)/Web/*.js 2>/dev/null || true

clean-server: ## Clean server build artifacts
	@echo "$(YELLOW)▶ Cleaning server artifacts...$(RESET)"
	@rm -rf $(SERVER_DIR)/target

clean-docker: ## Remove Docker images and volumes
	@echo "$(RED)▶ Removing Docker resources...$(RESET)"
	@$(COMPOSE) down -v --rmi local
	@echo "$(GREEN)✓ Docker resources removed$(RESET)"

reset: down clean-docker clean ## Full reset (stop, remove containers/volumes, clean artifacts)
	@echo "$(GREEN)✓ Full reset complete$(RESET)"

prune: ## Remove unused Docker resources (system-wide)
	@echo "$(RED)▶ Pruning Docker system...$(RESET)"
	@docker system prune -f
	@echo "$(GREEN)✓ Docker pruned$(RESET)"

fix-permissions: ## Fix ownership of Docker-created files
	@echo "$(YELLOW)▶ Fixing file permissions...$(RESET)"
	@if [ -d "$(PLUGIN_DIR)/dist" ] && [ "$$(stat -c '%u' $(PLUGIN_DIR)/dist 2>/dev/null)" != "$(UID)" ]; then \
		echo "  Fixing $(PLUGIN_DIR)/dist..."; \
		sudo chown -R $(UID):$(GID) $(PLUGIN_DIR)/dist; \
	fi
	@if [ -d "$(PLUGIN_DIR)/obj" ] && [ "$$(stat -c '%u' $(PLUGIN_DIR)/obj 2>/dev/null)" != "$(UID)" ]; then \
		echo "  Fixing $(PLUGIN_DIR)/obj..."; \
		sudo chown -R $(UID):$(GID) $(PLUGIN_DIR)/obj; \
	fi
	@echo "$(GREEN)✓ Permissions fixed$(RESET)"
