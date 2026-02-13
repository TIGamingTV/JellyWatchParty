# ============================================================================
# Build Targets
# ============================================================================

.PHONY: build build-plugin build-server build-server-docker build-all rebuild release release-image

build: build-plugin ## Build the Jellyfin plugin (alias)

build-plugin: ## Build the Jellyfin plugin
	@echo "$(GREEN)▶ Building Jellyfin plugin...$(RESET)"
	@mkdir -p $(PLUGIN_DIR)/Web $(PLUGIN_DIR)/dist $(PLUGIN_DIR)/obj $(PLUGIN_DIR)/bin
	@cp $(CLIENT_DIR)/plugin.js $(PLUGIN_DIR)/Web/plugin.js
	@for f in $(CLIENT_JS_FILES); do cp $(CLIENT_DIR)/$$f $(PLUGIN_DIR)/Web/; done
	@$(COMPOSE_TOOLS) run --rm plugin-builder
	@echo "$(GREEN)✓ Plugin built: $(PLUGIN_DIR)/dist/$(RESET)"

build-server: ## Build the session server locally (Rust)
	@echo "$(GREEN)▶ Building session server...$(RESET)"
	@cd $(SERVER_DIR) && cargo build --release
	@echo "$(GREEN)✓ Server built: $(SERVER_DIR)/target/release/$(RESET)"

build-server-docker: ## Rebuild session server Docker image
	@echo "$(GREEN)▶ Building session server Docker image...$(RESET)"
	@$(COMPOSE) build session-server
	@echo "$(GREEN)✓ Docker image built$(RESET)"

build-all: build-plugin build-server-docker ## Build everything (plugin + server image)
	@echo "$(GREEN)✓ All components built$(RESET)"

rebuild: clean build-all ## Clean and rebuild everything
	@$(COMPOSE) up -d --force-recreate
	@echo "$(GREEN)✓ Stack rebuilt and restarted$(RESET)"

release: clean ## Build release artifacts (zip)
	@echo "$(GREEN)▶ Building release...$(RESET)"
	@mkdir -p dist/plugin dist/server
	@$(MAKE) build-plugin
	@cp -r $(PLUGIN_DIR)/dist/* dist/plugin/
	@cd $(SERVER_DIR) && cargo build --release
	@cp $(SERVER_DIR)/target/release/session-server dist/server/ 2>/dev/null || true
	@cd dist && zip -r ../$(PROJECT_NAME)-release.zip .
	@echo "$(GREEN)✓ Release built: $(PROJECT_NAME)-release.zip$(RESET)"

release-image: ## Build release Docker image via prod compose
	@echo "$(GREEN)▶ Building release Docker image...$(RESET)"
	@$(COMPOSE_PROD) build session-server
	@echo "$(GREEN)✓ Release Docker image built$(RESET)"
