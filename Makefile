# ============================================================================
# OpenWatchParty - Makefile
# ============================================================================
# Usage: make [target]
# Run 'make help' for available targets
# ============================================================================

.DEFAULT_GOAL := help
SHELL := /bin/bash

# Include modular makefiles
include infra/make/config.mk
include infra/make/dev.mk
include infra/make/build.mk
include infra/make/test.mk
include infra/make/docker.mk
include infra/make/setup.mk
include infra/make/utils.mk
include infra/make/help.mk
