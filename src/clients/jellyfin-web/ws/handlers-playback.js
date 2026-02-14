(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const h = OWP._wsHandlers = OWP._wsHandlers || {};
  const state = OWP.state;
  const utils = OWP.utils;
  const ui = OWP.ui;
  const { SEEK_THRESHOLD } = OWP.constants;

  const handlePlayerPlay = (msg, video) => {
    state.lastSyncPlayState = 'playing';
    state.lastSyncServerTs = msg.server_ts;
    state.lastSyncPosition = msg.payload.position;
    state.syncCooldownUntil = utils.nowMs() + 2000;
    const targetTs = msg.payload.target_server_ts || msg.server_ts;
    if (targetTs && targetTs > utils.getServerNow()) {
      state.syncStatus = 'pending_play';
      state.pendingPlayUntil = targetTs;
      if (ui.updateSyncIndicator) ui.updateSyncIndicator();
      utils.scheduleAt(targetTs, () => {
        state.syncStatus = 'syncing';
        state.pendingPlayUntil = 0;
        if (ui.updateSyncIndicator) ui.updateSyncIndicator();
        video.play().catch(() => {});
      });
    } else {
      state.syncStatus = 'syncing';
      if (ui.updateSyncIndicator) ui.updateSyncIndicator();
      video.play().catch(() => {});
    }
    ui.showToast('Host resumed playback');
  };

  const handlePlayerPause = (msg, video) => {
    state.lastSyncPlayState = 'paused';
    state.syncCooldownUntil = 0;
    state.isInitialSync = false;
    state.initialSyncUntil = 0;
    state.initialSyncTargetPos = 0;
    state.syncStatus = 'synced';
    state.pendingPlayUntil = 0;
    if (state.pendingActionTimer) {
      clearTimeout(state.pendingActionTimer);
      state.pendingActionTimer = null;
    }
    if (ui.updateSyncIndicator) ui.updateSyncIndicator();
    video.pause();
    ui.showToast('Host paused playback');
  };

  const handlePlayerSeek = (msg, video) => {
    const hostPlayState = msg.payload.play_state || 'paused';
    state.lastSyncPlayState = hostPlayState;
    if (hostPlayState === 'playing') {
      video.currentTime = msg.payload.position + (OWP.constants.SYNC_LEAD_MS / 1000);
      state.lastSyncServerTs = utils.getServerNow();
      state.lastSyncPosition = msg.payload.position;
      state.syncCooldownUntil = utils.nowMs() + 2000;
      video.play().catch(() => {});
    }
  };

  const handlePlayerBuffering = (msg, video) => {
    state.lastSyncPlayState = 'paused';
    state.pendingPlayUntil = 0;
    if (state.pendingActionTimer) {
      clearTimeout(state.pendingActionTimer);
      state.pendingActionTimer = null;
    }
    if (state.syncStatus === 'pending_play') {
      state.syncStatus = 'syncing';
      if (ui.updateSyncIndicator) ui.updateSyncIndicator();
    }
    video.pause();
  };

  h.handlePlayerEvent = (msg, video) => {
    if (state.isHost || !video) return;
    utils.startSyncing();
    if (msg.payload && typeof msg.payload.position === 'number') {
      const action = msg.payload.action;
      const targetPos = (action === 'seek' || action === 'buffering')
        ? msg.payload.position
        : utils.adjustedPosition(msg.payload.position, msg.server_ts);
      const serverNow = utils.getServerNow();
      const gap = targetPos - video.currentTime;
      utils.log('CLIENT', {
        action,
        msg_pos: msg.payload.position,
        target_pos: targetPos,
        video_pos: video.currentTime,
        gap
      });
      if (Math.abs(gap) > SEEK_THRESHOLD) {
        video.pause();
        video.currentTime = targetPos;
        state.lastSyncServerTs = serverNow;
        state.lastSyncPosition = targetPos;
      } else {
        state.lastSyncServerTs = serverNow;
        state.lastSyncPosition = video.currentTime;
      }
    }
    if (msg.payload) {
      switch (msg.payload.action) {
        case 'play': handlePlayerPlay(msg, video); break;
        case 'pause': handlePlayerPause(msg, video); break;
        case 'seek': handlePlayerSeek(msg, video); break;
        case 'buffering': handlePlayerBuffering(msg, video); break;
      }
    }
  };
})();
