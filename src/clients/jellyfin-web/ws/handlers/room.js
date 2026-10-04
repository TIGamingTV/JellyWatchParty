(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const h = JWP._wsHandlers = JWP._wsHandlers || {};
  const state = JWP.state;
  const ui = JWP.ui;

  h.handleRoomList = (msg) => {
    state.rooms = msg.payload || [];
    if (!state.inRoom) ui.updateRoomListUI();
    ui.renderHomeWatchParties();
  };

  h.handleClientHello = (msg) => {
    if (msg.payload && msg.payload.client_id) {
      state.clientId = msg.payload.client_id;
      if (JWP.actions && JWP.actions.rememberSession) {
        JWP.actions.rememberSession(msg.payload.client_id, msg.payload.resume_secret);
      }
      // The server says which room it has us in. If we think we're in a room
      // but the server has none (removed or closed while we were offline,
      // or the reconnect window ran out), leave it locally. A different room
      // arrives as room_state right after this and is handled there.
      if ('room_id' in msg.payload && !msg.payload.room_id && state.inRoom) {
        if (JWP.actions && JWP.actions.resetRoomState) JWP.actions.resetRoomState();
        ui.showToast('You are no longer in the watch party');
      }
      ui.render();
    }
  };

  h.handleParticipants = (msg) => {
    const list = msg.payload && msg.payload.participants;
    if (!Array.isArray(list)) return;
    state.participants = list;
    state.participantCount = list.length;
    if (state.inRoom && ui.renderParticipants) ui.renderParticipants();
  };

  h.handleParticipantsUpdate = (msg) => {
    state.participantCount = msg.payload.participant_count;
    if (state.inRoom && ui.renderParticipants) ui.renderParticipants();
    if (state.lastParticipantCount && state.participantCount > state.lastParticipantCount) {
      ui.showToast('A participant joined the room');
    }
    state.lastParticipantCount = state.participantCount;
  };

  h.handleClientLeft = (msg) => {
    if (msg.payload?.participant_count !== undefined) {
      state.participantCount = msg.payload.participant_count;
      if (state.inRoom) {
        if (ui.renderParticipants) ui.renderParticipants();
        ui.showToast('A participant left the room');
      }
      state.lastParticipantCount = state.participantCount;
    }
  };

  h.handleRoomClosed = (msg) => {
    // Full local reset (countdown, chat, timers, host flag), not just the
    // room fields: a removed host must not stay "host" in the lobby.
    if (JWP.actions && JWP.actions.resetRoomState) JWP.actions.resetRoomState();
    state.inRoom = false;
    state.roomId = '';
    state.roomMediaId = '';
    state.participants = [];
    state.startPending = false;
    state.startQueued = false;
    state.roomStarted = true;
    if (ui.hideCountdown) ui.hideCountdown();
    const reason = msg.payload?.reason || 'The room was closed';
    ui.showToast(reason);
    ui.render();
  };

  h.handleHostChanged = (msg) => {
    if (!msg.payload) return;
    const wasHost = state.isHost;
    state.isHost = (msg.payload.host_id === state.clientId);
    if (msg.payload.participant_count !== undefined) {
      state.participantCount = msg.payload.participant_count;
    }
    if (state.isHost && !wasHost) {
      ui.showToast('You are now the host');
    } else if (!state.isHost) {
      ui.showToast(`${msg.payload.host_name || 'Someone'} is now the host`);
    }
    // Force a full re-render: the fast-render path only checks
    // state.inRoom, not state.isHost, so host-only UI (the Close vs. Leave
    // button) won't otherwise flip.
    ui.render(true);
  };

  h.handleError = (msg) => {
    const message = msg.payload?.message || 'Unknown error';
    console.error('[JellyWatchParty] Server error:', message);
    ui.showToast(message);
    if (msg.payload?.reason === 'room_not_found' && !state.inRoom) {
      // joinRoom set this optimistically; the room is gone.
      state.roomId = '';
    }
    if (msg.payload?.reason === 'wrong_password' && msg.room) {
      ui.promptJoinWithPassword(msg.room);
    }
  };
})();
