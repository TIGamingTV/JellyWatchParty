(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const h = JWP._wsHandlers = JWP._wsHandlers || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const ui = JWP.ui;
  const { SEEK_THRESHOLD } = JWP.constants;

  // play/pause/seek: what democratic mode lets anyone send (buffering and
  // state_update stay with the host).
  const isDiscreteCommand = (action) => action === 'play' || action === 'pause' || action === 'seek';

  // Our own command coming back (e.g. a play the server held until everyone
  // was ready, which it then sends to the whole room).
  const isOwnCommand = (msg) => !!(msg.payload && msg.payload.by && msg.payload.by === state.clientId);

  // Who to credit in a toast: the server names whoever triggered the event
  // (host, a guest, a TV device, a chat-bot user). Older servers don't.
  const actorName = (msg) => {
    const name = msg.payload && typeof msg.payload.by_name === 'string' ? msg.payload.by_name.trim() : '';
    return name || 'Host';
  };

  const handlePlayerPlay = (msg, video, quiet) => {
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
        // The countdown may outlast the suppression started on receipt;
        // this play is the room's, not ours to broadcast (democratic mode).
        utils.startSyncing();
        video.play().catch(() => {});
      });
    } else {
      state.syncStatus = 'syncing';
      if (ui.updateSyncIndicator) ui.updateSyncIndicator();
      video.play().catch(() => {});
    }
    if (!quiet) ui.showToast(`${actorName(msg)} resumed playback`);
  };

  const handlePlayerPause = (msg, video, quiet) => {
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
    if (!quiet) ui.showToast(`${actorName(msg)} paused playback`);
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

  // Whether the host acts on a relayed player_event. Normally it ignores
  // them (it is the one sending); in a democratic room it follows other
  // participants' (and chat bots') play/pause/seek like a guest would,
  // while staying the room's position source (no state_update following,
  // no drift correction).
  const hostApplies = (msg, video) => {
    const action = msg.payload && msg.payload.action;
    if (!state.democraticMode || !isDiscreteCommand(action)) return false;
    if (isOwnCommand(msg)) return false;
    // Mid media switch: the command is for the item we're leaving.
    if (state.mediaSwitchPending) return false;
    // Already playing: nothing to start, and the host's position is the
    // room's (this is also how its own held-back play comes back when the
    // server has no `by` for it).
    if (action === 'play' && video && !video.paused) return false;
    return true;
  };

  h.handlePlayerEvent = (msg, video) => {
    if (state.isHost) {
      if (state.startPending && msg.payload && msg.payload.action === 'play') {
        handleHostStart(msg, video);
        return;
      }
      if (!hostApplies(msg, video)) return;
    }
    if (!video) return;
    // The room switched media and we haven't loaded it yet - don't act on
    // position/play-state commands meant for the new item while we're
    // still (or still showing) the old one (issue #71).
    if (!utils.isOnRoomMedia()) return;
    // Everything below moves the video programmatically; its play/pause/
    // seeked events must not go back out as our own commands.
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
      const quiet = isOwnCommand(msg);
      switch (msg.payload.action) {
        case 'play': handlePlayerPlay(msg, video, quiet); break;
        case 'pause': handlePlayerPause(msg, video, quiet); break;
        case 'seek': handlePlayerSeek(msg, video); break;
        case 'buffering': handlePlayerBuffering(msg, video); break;
      }
      // Democratic mode: commands can come from anyone, and the host's next
      // state_update may predate this one - don't let it undo it.
      if (state.democraticMode && isDiscreteCommand(msg.payload.action)) {
        utils.armCommandGuard(utils.expectedRoomPosition(), state.lastSyncPlayState);
      }
    }
  };
})();
