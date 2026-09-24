(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const actions = JWP.actions = JWP.actions || {};
  const state = JWP.state;
  const utils = JWP.utils;

  const send = (type, payload = {}, roomOverride = null) => {
    if (!state.ws || state.ws.readyState !== 1) return;
    const message = {
      type,
      room: roomOverride || state.roomId,
      payload,
      ts: utils.nowMs()
    };
    if (state.clientId) message.client = state.clientId;
    state.ws.send(JSON.stringify(message));
  };

  const createRoom = async (password = '') => {
    // Resolve the item id before reading the position: on Jellyfin 12.1+ the
    // id may have to come from the server (no global playbackManager).
    let mediaId = null;
    try {
      mediaId = utils.resolveCurrentItemId
        ? await utils.resolveCurrentItemId()
        : utils.getCurrentItemId();
    } catch (e) {
      mediaId = utils.getCurrentItemId();
    }
    const v = utils.getVideo();
    const userName = state.userName
      || window.ApiClient?._currentUser?.Name
      || 'Anonymous';
    const payload = {
      start_pos: v ? v.currentTime : 0,
      media_id: mediaId,
      user_name: userName
    };
    if (password) payload.password = password;
    send('create_room', payload);
  };

  const joinRoom = (id, password = '') => {
    state.roomId = id;
    const userName = state.userName
      || window.ApiClient?._currentUser?.Name
      || 'Anonymous';
    const payload = { user_name: userName };
    if (password) payload.password = password;
    send('join_room', payload, id);
  };

  const leaveRoom = () => {
    send('leave_room');
    state.inRoom = false;
    state.roomId = '';
    state.roomMediaId = '';
    state.mediaSwitchPending = false;
    state.mediaSwitchPendingUntil = 0;
    state.readyRoomId = '';
    state.isInitialSync = false;
    state.initialSyncUntil = 0;
    state.initialSyncTargetPos = 0;
    state.syncCooldownUntil = 0;
    state.syncStatus = 'synced';
    state.pendingPlayUntil = 0;
    state.currentDrift = 0;
    if (state.pendingActionTimer) {
      clearTimeout(state.pendingActionTimer);
      state.pendingActionTimer = null;
    }
    if (JWP.chat) JWP.chat.clear();
    const panel = document.getElementById(JWP.constants.PANEL_ID);
    if (panel) panel.classList.add('hide');
  };

  Object.assign(actions, { send, createRoom, joinRoom, leaveRoom });
})();
