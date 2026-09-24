(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const h = JWP._wsHandlers = JWP._wsHandlers || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const ui = JWP.ui;
  const { SEEK_THRESHOLD } = JWP.constants;

  const applyRoomState = (msg) => {
    state.inRoom = true;
    state.roomId = msg.room;
    state.roomName = msg.payload.name;
    state.participantCount = msg.payload.participant_count;
    if (!state.clientId && msg.client) {
      state.clientId = msg.client;
    }
    state.isHost = (msg.payload.host_id === state.clientId);
    state.roomMediaId = msg.payload.media_id || '';
    if (JWP.chat && Array.isArray(msg.payload.chat_history)) {
      JWP.chat.hydrate(msg.payload.chat_history);
    }
    if (!state.hasTimeSync && typeof msg.server_ts === 'number') {
      state.serverOffsetMs = msg.server_ts - utils.nowMs();
      state.hasTimeSync = true;
    }
    if (msg.payload && msg.payload.state) {
      state.lastSyncServerTs = msg.server_ts || utils.getServerNow();
      state.lastSyncPosition = typeof msg.payload.state.position === 'number'
        ? msg.payload.state.position
        : 0;
      state.lastSyncPlayState = msg.payload.state.play_state || 'paused';
    }
  };

  const syncToRoom = (msg, video) => {
    if (!video || state.isHost || !msg.payload?.state) return;
    const basePos = msg.payload.state.position || 0;
    const targetPos = utils.adjustedPosition(basePos, msg.server_ts);
    const hostPlaying = msg.payload.state.play_state === 'playing';
    utils.log('CLIENT', {
      type: 'room_state',
      msg_pos: basePos,
      target_pos: targetPos,
      video_pos: video.currentTime,
      gap: targetPos - video.currentTime,
      play_state: msg.payload.state.play_state
    });
    utils.startSyncing();
    if (hostPlaying) {
      const { INITIAL_SYNC_COOLDOWN_MS, INITIAL_SYNC_MAX_MS } = JWP.constants;
      const now = utils.nowMs();
      state.isInitialSync = true;
      state.initialSyncUntil = now + INITIAL_SYNC_MAX_MS;
      state.syncCooldownUntil = now + INITIAL_SYNC_COOLDOWN_MS;
      state.initialSyncTargetPos = targetPos;
      utils.log('CLIENT', { type: 'initial_sync_started', cooldown: INITIAL_SYNC_COOLDOWN_MS, max: INITIAL_SYNC_MAX_MS, targetPos });
    }
    if (Math.abs(video.currentTime - targetPos) > SEEK_THRESHOLD) {
      video.currentTime = targetPos;
    }
    if (hostPlaying) {
      video.play().catch(() => {});
    } else if (msg.payload.state.play_state === 'paused') {
      video.pause();
    }
  };

  h.handleRoomState = (msg, video) => {
    applyRoomState(msg);
    ui.render();
    syncToRoom(msg, video);
    if (!state.isHost && msg.payload?.media_id) {
      if (JWP.playback && JWP.playback.ensurePlayback) {
        const roomPos = msg.payload.state && typeof msg.payload.state.position === 'number'
          ? utils.adjustedPosition(msg.payload.state.position, msg.server_ts)
          : 0;
        JWP.playback.ensurePlayback(msg.payload.media_id, roomPos);
        if (JWP.playback.watchReady) JWP.playback.watchReady();
      }
    }
  };

  // The host started something after creating an empty room, or switched to
  // a different item mid-session (issue #71). Re-runs the same "load and
  // wait to be ready" flow as joining a room; roomMediaId gates syncLoop and
  // the other player_event/state_update handlers until the new item is
  // actually loaded, so stale position corrections never target it.
  h.handleMediaChanged = (msg, video) => {
    if (state.isHost) return; // we sent this; we already know
    const mediaId = msg.payload && msg.payload.media_id;
    if (!mediaId) return;
    state.roomMediaId = mediaId;
    state.readyRoomId = '';
    state.syncStatus = 'unknown';
    state.lastSyncServerTs = 0;
    state.lastSyncPosition = 0;
    state.lastSyncPlayState = 'paused';
    state.isInitialSync = false;
    state.initialSyncUntil = 0;
    state.initialSyncTargetPos = 0;
    state.syncCooldownUntil = 0;
    state.isDriftCorrecting = false;
    state.pendingPlayUntil = 0;
    if (state.pendingActionTimer) {
      clearTimeout(state.pendingActionTimer);
      state.pendingActionTimer = null;
    }
    if (ui.updateSyncIndicator) ui.updateSyncIndicator();
    ui.render();
    if (ui.showToast) ui.showToast("Host switched media");
    if (JWP.playback && JWP.playback.ensurePlayback) {
      const roomPos = msg.payload && typeof msg.payload.position === 'number'
        ? utils.adjustedPosition(msg.payload.position, msg.server_ts)
        : 0;
      JWP.playback.ensurePlayback(mediaId, roomPos);
      if (JWP.playback.watchReady) JWP.playback.watchReady();
    }
  };

  h.handleStateUpdate = (msg, video) => {
    if (state.isHost || !video) return;
    if (!utils.isOnRoomMedia()) return;
    if (msg.payload) {
      state.lastSyncPlayState = msg.payload.play_state || state.lastSyncPlayState;
    }
    if (msg.payload.play_state === 'playing' && video.paused) {
      utils.startSyncing();
      video.play().catch(() => {});
      state.lastSyncServerTs = utils.getServerNow();
      state.lastSyncPosition = video.currentTime;
      state.syncCooldownUntil = utils.nowMs() + 2000;
      return;
    } else if (msg.payload.play_state === 'paused' && !video.paused) {
      utils.startSyncing();
      state.syncCooldownUntil = 0;
      state.isInitialSync = false;
      state.initialSyncUntil = 0;
      state.initialSyncTargetPos = 0;
      video.pause();
    }
    if (state.isBuffering || !utils.isVideoReady()) return;
    if (state.syncCooldownUntil && utils.nowMs() < state.syncCooldownUntil) {
      return;
    }
    if (msg.payload) {
      state.lastSyncServerTs = msg.server_ts || utils.getServerNow();
      state.lastSyncPosition = typeof msg.payload.position === 'number'
        ? msg.payload.position
        : state.lastSyncPosition;
    }
  };
})();
