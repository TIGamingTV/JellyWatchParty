#!/bin/bash
set -e

PUID=${PUID:-1000}
PGID=${PGID:-1000}

# --- Drop privileges ---
chown -R "$PUID:$PGID" /config /cache
exec setpriv --reuid="$PUID" --regid="$PGID" --clear-groups /jellyfin/jellyfin
