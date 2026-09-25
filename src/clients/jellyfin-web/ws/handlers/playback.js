(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const h = JWP._wsHandlers = JWP._wsHandlers || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const ui = JWP.ui;
  const { SEEK_THRESHOLD } = JWP.constants;

  const handlePlayerPlay = (msg, video) => {
    state.roomStarted = true;
    if (msg.payload.countdown && ui.showCountdown) {
      ui.showCountdown(msg.payload.target_server_ts || msg.server_ts);
    }
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
    if (ui.hideCountdown) ui.hideCountdown();
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
      video.currentTime = msg.payload.position + (JWP.constants.SYNC_LEAD_MS / 1000);
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

  // The server answered the host's first play: start at the shared moment,
  // after the countdown when there is one.
  const handleHostStart = (msg, video) => {
    const p = msg.payload || {};
    const target = p.target_server_ts || msg.server_ts;
    if (state.startSafetyTimer) {
      clearTimeout(state.startSafetyTimer);
      state.startSafetyTimer = null;
    }
    if (p.countdown && ui.showCountdown) ui.showCountdown(target);
    utils.scheduleAt(target, () => {
      state.startPending = false;
      state.roomStarted = true;
      if (ui.hideCountdown) ui.hideCountdown();
      if (!video) return;
      utils.suppress();
      if (typeof p.position === 'number' && Math.abs(video.currentTime - p.position) > SEEK_THRESHOLD) {
        video.currentTime = p.position;
      }
      video.play().catch(() => {});
    });
  };

  h.handleStartPending = (msg) => {
    state.roomStarted = false;
    if (ui.showStartWaiting) ui.showStartWaiting();
  };

  h.handlePlayerEvent = (msg, video) => {
    if (state.isHost) {
      if (state.startPending && msg.payload && msg.payload.action === 'play') {
        handleHostStart(msg, video);
      }
      return;
    }
    if (!video) return;
    // The room switched media and we haven't loaded it yet - don't act on
    // position/play-state commands meant for the new item while we're
    // still (or still showing) the old one (issue #71).
    if (!utils.isOnRoomMedia()) return;
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
