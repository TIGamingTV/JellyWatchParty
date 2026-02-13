# ============================================================================
# Configuration
# ============================================================================

PROJECT_NAME    := OpenWatchParty
COMPOSE_FILE      := infra/docker/dev/docker-compose.yml
COMPOSE           := docker compose -f $(COMPOSE_FILE)
COMPOSE_TOOLS     := docker compose --profile tools -f $(COMPOSE_FILE)
COMPOSE_PROD_FILE := infra/docker/prod/docker-compose.yml
COMPOSE_PROD      := docker compose -f $(COMPOSE_PROD_FILE)

# Directories
PLUGIN_DIR      := src/plugins/jellyfin/OpenWatchParty
CLIENT_DIR      := src/clients/jellyfin-web
SERVER_DIR      := src/server
DOCS_DIR        := docs

# Containers
JELLYFIN_CTR    := jellyfin-dev
SESSION_CTR     := owp-session-server

# User mapping for Docker (avoid root-owned files)
export UID      := $(shell id -u)
export GID      := $(shell id -g)

# Client JS files (order matters for plugin loading)
CLIENT_JS_FILES := state.js utils.js ui.js playback.js chat.js ws.js app.js

# Colors (disable with NO_COLOR=1)
ifndef NO_COLOR
  GREEN  := \033[0;32m
  YELLOW := \033[0;33m
  BLUE   := \033[0;34m
  CYAN   := \033[0;36m
  RED    := \033[0;31m
  BOLD   := \033[1m
  RESET  := \033[0m
else
  GREEN  :=
  YELLOW :=
  BLUE   :=
  CYAN   :=
  RED    :=
  BOLD   :=
  RESET  :=
endif
