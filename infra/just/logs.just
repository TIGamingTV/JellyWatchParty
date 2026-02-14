# ============================================================================
# Logs module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Follow logs from all services')]
default:
    @{{compose}} logs -f --tail=100

[doc('Follow session server logs')]
server:
    @{{compose}} logs -f --tail=100 session-server

[doc('Follow Jellyfin logs')]
jellyfin:
    @{{compose}} logs -f --tail=100 jellyfin-dev
