# ============================================================================
# Shell module
# ============================================================================

set working-directory := "../.."

import "common.just"

[doc('Open shell in session server container')]
server:
    @docker exec -it {{session_ctr}} /bin/sh

[doc('Open shell in Jellyfin container')]
jellyfin:
    @docker exec -it {{jellyfin_ctr}} /bin/bash
