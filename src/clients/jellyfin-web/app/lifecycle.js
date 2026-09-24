(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const state = JWP.state;
  const ui = JWP.ui;
  const utils = JWP.utils;
  const playback = JWP.playback;
  const { UI_CHECK_MS, HOME_REFRESH_MS, SYNC_LOOP_MS } = JWP.constants;

  let panelStopPropagation = null;
  let hadVideoElement = false;

  const clearAllIntervals = () => {
    if (state.intervals.ui) { clearInterval(state.intervals.ui); state.intervals.ui = null; }
    if (state.intervals.ping) { clearInterval(state.intervals.ping); state.intervals.ping = null; }
    if (state.intervals.home) { clearInterval(state.intervals.home); state.intervals.home = null; }
    if (state.intervals.sync) { clearInterval(state.intervals.sync); state.intervals.sync = null; }
    if (state.intervals.stateUpdate) { clearInterval(state.intervals.stateUpdate); state.intervals.stateUpdate = null; }
  };

  const onVideoPlayerExit = () => {
    console.log('[JellyWatchParty] Video player closed, cleaning up...');
    const panel = document.getElementById(JWP.constants.PANEL_ID);
    if (panel) panel.classList.add('hide');
    // A guest with no player open can't follow anything, so they leave.
    // The host stays: closing the player is how a host browses for the next
    // thing to watch (issue #71 case B/C) - they now send set_media once
    // they start playing again (see playback/bind.js), instead of being
    // dropped from the room and re-promoting a guest to host in the
    // meantime. The host still leaves explicitly (Close/Leave button) or
    // via the existing disconnect/reconnect-timeout rules.
    if (state.inRoom && !state.isHost && JWP.actions && JWP.actions.leaveRoom) {
      JWP.actions.leaveRoom();
    }
    if (JWP.playback && JWP.playback.cleanupVideoListeners) {
      JWP.playback.cleanupVideoListeners();
    }
    if (utils.clearServerNowPlaying) utils.clearServerNowPlaying();
    state.playCommandItemId = '';
    state.playCommandUntil = 0;
    state.bound = false;
  };

  const createPanel = () => {
    if (document.getElementById(JWP.constants.PANEL_ID)) return;
    const panel = document.createElement('div');
    panel.id = JWP.constants.PANEL_ID;
    panel.className = 'hide';
    document.body.appendChild(panel);
    panelStopPropagation = (e) => e.stopPropagation();
    panel.addEventListener('click', panelStopPropagation);
    panel.addEventListener('mousedown', panelStopPropagation);
    panel.addEventListener('keydown', panelStopPropagation);
    panel.addEventListener('keyup', panelStopPropagation);
    panel.addEventListener('keypress', panelStopPropagation);
  };

  const startIntervals = () => {
    state.intervals.ui = setInterval(() => {
      if (document.visibilityState !== 'visible') return;
      const video = utils.getVideo();
      if (hadVideoElement && !video) {
        hadVideoElement = false;
        onVideoPlayerExit();
        return;
      }
      if (video) {
        hadVideoElement = true;
        ui.injectOsdButton();
        playback.bindVideo();
        if (playback.patchTrackSwitching) playback.patchTrackSwitching();
        if (state.pendingJoinRoomId) {
          console.log('[JellyWatchParty] Video detected, pendingJoinRoomId:', state.pendingJoinRoomId);
          if (JWP.actions && JWP.actions.joinRoom) {
            const roomId = state.pendingJoinRoomId;
            state.pendingJoinRoomId = '';
            setTimeout(() => {
              console.log('[JellyWatchParty] Auto-joining room:', roomId);
              const room = state.rooms.find(r => r.id === roomId);
              if (room && room.has_password && ui.promptJoinWithPassword) {
                // Ask up front instead of relying on the wrong_password
                // error-retry fallback for a room we already know needs one.
                ui.promptJoinWithPassword(roomId);
              } else {
                JWP.actions.joinRoom(roomId);
              }
            }, 500);
          }
        }
      }

      // Safety net only. Jellyfin is an SPA and replaces header DOM during
      // navigation; ui.observeToolbar() reacts to that immediately, so this
      // poll exists purely to cover the initial mount and any environment
      // without MutationObserver.
      ui.injectGlobalButton();
    }, UI_CHECK_MS);
    state.intervals.home = setInterval(() => {
      if (document.visibilityState === 'visible' && utils.isHomeView()) {
        ui.renderHomeWatchParties();
      }
    }, HOME_REFRESH_MS);
    state.intervals.sync = setInterval(() => {
      if (state.inRoom && !state.isHost) {
        playback.syncLoop();
      }
      if (state.inRoom && playback.reportOwnStatus) playback.reportOwnStatus();
    }, SYNC_LOOP_MS);
  };

  const init = () => {
    if (state.initialized) {
      console.log('[JellyWatchParty] Already initialized, skipping');
      return;
    }
    state.initialized = true;
    console.log('%c JellyWatchParty Plugin Loaded (OSD Mode) ', 'background: #2e7d32; color: #fff; font-size: 12px; padding: 2px; border-radius: 2px;');
    clearAllIntervals();
    ui.injectStyles();
    createPanel();
    if (ui.setupPanelDismiss) ui.setupPanelDismiss();
    // Re-place the toolbar button the moment Jellyfin's router rebuilds the
    // header, rather than up to UI_CHECK_MS later.
    if (ui.observeToolbar) ui.observeToolbar();
    if (JWP.actions && JWP.actions.connect) {
      console.log('[JellyWatchParty] Initiating WebSocket connection...');
      JWP.actions.connect();
    } else {
      console.error('[JellyWatchParty] JWP.actions.connect not available!');
    }
    startIntervals();
  };

  // Expose lifecycle internals for the cleanup module and for tests.
  JWP._lifecycle = {
    get panelStopPropagation() { return panelStopPropagation; },
    set panelStopPropagation(v) { panelStopPropagation = v; },
    get hadVideoElement() { return hadVideoElement; },
    set hadVideoElement(v) { hadVideoElement = v; },
    clearAllIntervals,
    onVideoPlayerExit
  };

  JWP.app = JWP.app || {};
  Object.assign(JWP.app, { init });
})();
