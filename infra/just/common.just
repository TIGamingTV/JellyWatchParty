# ============================================================================
# Shared variables and settings for all just modules
# ============================================================================

set shell := ["bash", "-euo", "pipefail", "-c"]
set quiet

# -- Project -----------------------------------------------------------------

project_name      := "OpenWatchParty"
compose_file      := "infra/docker/dev/docker-compose.yml"
compose           := "docker compose -f " + compose_file
compose_tools     := "docker compose --profile tools -f " + compose_file
compose_prod_file := "infra/docker/prod/docker-compose.yml"
compose_prod      := "docker compose -f " + compose_prod_file

# -- Directories -------------------------------------------------------------

plugin_dir  := "src/plugins/jellyfin/OpenWatchParty"
client_dir  := "src/clients/jellyfin-web"
server_dir  := "src/server"

# -- Containers --------------------------------------------------------------

jellyfin_ctr := "jellyfin-dev"
session_ctr  := "owp-session-server"

# -- Client JS files (order matters for plugin loading) ----------------------

client_js_files := "state.js utils.js ui.js playback.js chat.js ws.js app.js"

# -- Colors ------------------------------------------------------------------

GREEN  := '\033[0;32m'
YELLOW := '\033[0;33m'
BLUE   := '\033[0;34m'
CYAN   := '\033[0;36m'
RED    := '\033[0;31m'
BOLD   := '\033[1m'
RESET  := '\033[0m'
