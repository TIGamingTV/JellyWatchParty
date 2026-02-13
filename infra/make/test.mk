# ============================================================================
# Testing & Quality Targets
# ============================================================================

.PHONY: test test-server lint lint-server lint-client fmt fmt-server check pre-commit

test: test-server ## Run all tests

test-server: ## Run Rust server tests
	@echo "$(GREEN)▶ Running server tests...$(RESET)"
	@cd $(SERVER_DIR) && cargo test
	@echo "$(GREEN)✓ Tests passed$(RESET)"

lint: lint-server lint-client ## Run all linters

lint-server: ## Lint Rust code with clippy
	@echo "$(CYAN)▶ Linting Rust code...$(RESET)"
	@cd $(SERVER_DIR) && cargo clippy -- -D warnings
	@echo "$(GREEN)✓ Rust lint passed$(RESET)"

lint-client: ## Lint JavaScript code (requires eslint)
	@echo "$(CYAN)▶ Linting JavaScript...$(RESET)"
	@if command -v eslint &> /dev/null; then \
		eslint $(CLIENT_DIR)/*.js || true; \
	else \
		echo "$(YELLOW)⚠ eslint not installed, skipping$(RESET)"; \
	fi

fmt: fmt-server ## Format all code

fmt-server: ## Format Rust code
	@echo "$(CYAN)▶ Formatting Rust code...$(RESET)"
	@cd $(SERVER_DIR) && cargo fmt
	@echo "$(GREEN)✓ Code formatted$(RESET)"

check: ## Run cargo check (fast compile check)
	@echo "$(CYAN)▶ Running cargo check...$(RESET)"
	@cd $(SERVER_DIR) && cargo check
	@echo "$(GREEN)✓ Check passed$(RESET)"

pre-commit: ## Run all pre-commit hooks manually
	@echo "$(CYAN)▶ Running pre-commit hooks...$(RESET)"
	@pre-commit run --all-files
