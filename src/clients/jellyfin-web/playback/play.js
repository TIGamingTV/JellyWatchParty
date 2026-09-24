(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const playback = JWP.playback = JWP.playback || {};
  const utils = JWP.utils;

  const tryPlayMethods = (pm, item, startPositionTicks = 0) => {
    const playOptions = { startPositionTicks };
    const errors = [];
    if (typeof pm.play === 'function') {
      try {
        pm.play({ items: [item], ...playOptions });
        console.log('[JellyWatchParty] Playback started via pm.play({ items })');
        return { success: true, errors };
      } catch (err) {
        errors.push({ method: 'play({ items })', error: err.message });
      }
      try {
        pm.play({ item: item, ...playOptions });
        console.log('[JellyWatchParty] Playback started via pm.play({ item })');
        return { success: true, errors };
      } catch (err) {
        errors.push({ method: 'play({ item })', error: err.message });
      }
      const itemId = item?.Id || item?.id;
      if (itemId) {
        try {
          pm.play({ ids: [itemId], ...playOptions });
          console.log('[JellyWatchParty] Playback started via pm.play({ ids })');
          return { success: true, errors };
        } catch (err) {
          errors.push({ method: 'play({ ids })', error: err.message });
        }
      }
    }
    if (typeof pm.playItems === 'function') {
      try {
        pm.playItems([item], startPositionTicks);
        console.log('[JellyWatchParty] Playback started via pm.playItems()');
        return { success: true, errors };
      } catch (err) {
        errors.push({ method: 'playItems()', error: err.message });
      }
    }
    return { success: false, errors };
  };

  const playItem = (item, startPositionTicks = 0) => {
    const pm = utils.getPlaybackManager();
    if (!pm) {
      console.warn('[JellyWatchParty] Playback failed: PlaybackManager not available');
      return false;
    }
    const result = tryPlayMethods(pm, item, startPositionTicks);
    if (!result.success) {
      console.error('[JellyWatchParty] All playback methods failed:', result.errors);
      if (JWP.ui && JWP.ui.showToast) {
        JWP.ui.showToast('Failed to start playback. Try refreshing the page.');
      }
    }
    return result.success;
  };

  // How long a sent PlayNow command blocks re-sending for the same item. The
  // player normally opens well within this window; the id check in
  // ensurePlayback stops further attempts once it has.
  const PLAY_COMMAND_GUARD_MS = 15000;
  const TICKS_PER_SECOND = 10000000;

  const toStartTicks = (startPos) => {
    const seconds = Number(startPos);
    if (!Number.isFinite(seconds) || seconds <= 0) return 0;
    return Math.floor(seconds * TICKS_PER_SECOND);
  };

  // Jellyfin 12.1+ does not expose playbackManager globally. Instead, ask the
  // server to send a PlayNow command to this browser's own session;
  // jellyfin-web handles that remote-control command itself and opens the
  // player. Resolves to true once the server accepted the command.
  const playViaSessionCommand = async (itemId, startPos = 0) => {
    const state = JWP.state;
    const now = utils.nowMs();
    if (state.playCommandItemId === itemId && now < state.playCommandUntil) return true;
    if (!utils.getOwnSession || !utils.apiFetch) return false;
    let session = null;
    try {
      session = await utils.getOwnSession();
    } catch (e) {
      console.warn('[JellyWatchParty] Own session lookup threw:', e && e.message);
      session = null;
    }
    if (!session || !session.id) {
      // getOwnSession already logged the HTTP status (if any) that caused
      // this - e.g. a 401 means the server rejected our auth headers.
      console.warn('[JellyWatchParty] Playback fallback failed: own session not found');
      return false;
    }
    if (session.nowPlayingItemId === itemId) {
      // Already playing it (e.g. started by hand before joining); don't
      // restart the player.
      state.serverNowPlayingId = itemId;
      return true;
    }
    const params = new URLSearchParams({
      playCommand: 'PlayNow',
      itemIds: itemId,
      startPositionTicks: String(toStartTicks(startPos))
    });
    const path = `/Sessions/${encodeURIComponent(session.id)}/Playing?${params.toString()}`;
    try {
      const res = await utils.apiFetch(path, { method: 'POST' });
      if (!res || !res.ok) {
        console.warn('[JellyWatchParty] PlayNow session command rejected:', res && res.status);
        return false;
      }
    } catch (err) {
      console.warn('[JellyWatchParty] PlayNow session command failed:', err && err.message);
      return false;
    }
    state.playCommandItemId = itemId;
    state.playCommandUntil = utils.nowMs() + PLAY_COMMAND_GUARD_MS;
    console.log('[JellyWatchParty] Playback requested via PlayNow session command');
    return true;
  };

  // Last resort: open the item's details page and press its Play button.
  const playViaDetailsPage = (itemId) => {
    if (JWP.ui && JWP.ui.openDetailsAndPlay) {
      JWP.ui.openDetailsAndPlay(itemId);
      return true;
    }
    return false;
  };

  const retry = (fn, attempt) => {
    if (attempt < 5) setTimeout(() => fn(attempt + 1), 500);
  };

  const ensurePlayback = (itemId, startPos = 0, attempt = 0) => {
    const state = JWP.state;
    if (!itemId || !window.ApiClient) return;
    if (utils.getCurrentItemId() === itemId) return;
    if (state.joiningItemId === itemId) return;
    const again = (next) => ensurePlayback(itemId, startPos, next);

    if (!utils.getPlaybackManager()) {
      state.joiningItemId = itemId;
      playViaSessionCommand(itemId, startPos).then((ok) => {
        if (ok) return;
        if (attempt < 2) {
          retry(again, attempt);
        } else if (playViaDetailsPage(itemId)) {
          console.log('[JellyWatchParty] Falling back to details page playback');
        } else if (JWP.ui && JWP.ui.showToast) {
          JWP.ui.showToast('Failed to start playback. Try refreshing the page.');
        }
      }).finally(() => {
        state.joiningItemId = '';
      });
      return;
    }

    const userId = ApiClient.getCurrentUserId?.() || ApiClient._currentUserId;
    if (!userId) {
      retry(again, attempt);
      return;
    }
    state.joiningItemId = itemId;
    ApiClient.getItem(userId, itemId).then((item) => {
      if (!playItem(item, toStartTicks(startPos))) retry(again, attempt);
    }).catch(() => {
      retry(again, attempt);
    }).finally(() => {
      state.joiningItemId = '';
    });
  };

  Object.assign(playback, { playItem, ensurePlayback, playViaSessionCommand });
})();
