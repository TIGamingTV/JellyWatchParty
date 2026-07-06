(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const h = OWP._wsHandlers = OWP._wsHandlers || {};
  const state = OWP.state;
  const ui = OWP.ui;

  h.handleRoomList = (msg) => {
    state.rooms = msg.payload || [];
    if (!state.inRoom) ui.updateRoomListUI();
    ui.renderHomeWatchParties();
  };

  h.handleClientHello = (msg) => {
    if (msg.payload && msg.payload.client_id) {
      state.clientId = msg.payload.client_id;
      ui.render();
    }
  };

  h.handleParticipantsUpdate = (msg) => {
    state.participantCount = msg.payload.participant_count;
    if (state.inRoom) {
      const el = document.getElementById('owp-participants-list');
      if (el) el.textContent = `Online: ${state.participantCount}`;
    }
    if (state.lastParticipantCount && state.participantCount > state.lastParticipantCount) {
      ui.showToast('A participant joined the room');
    }
    state.lastParticipantCount = state.participantCount;
  };

  h.handleClientLeft = (msg) => {
    if (msg.payload?.participant_count !== undefined) {
      state.participantCount = msg.payload.participant_count;
      if (state.inRoom) {
        const el = document.getElementById('owp-participants-list');
        if (el) el.textContent = `Online: ${state.participantCount}`;
        ui.showToast('A participant left the room');
      }
      state.lastParticipantCount = state.participantCount;
    }
  };

  h.handleRoomClosed = (msg) => {
    state.inRoom = false;
    state.roomId = '';
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
    // state.inRoom, not state.isHost, so host-only UI (Close vs. Leave,
    // the democratic-mode toggle) won't otherwise flip.
    ui.render(true);
  };

  h.handleDemocraticModeChanged = (msg) => {
    if (!msg.payload) return;
    state.democraticMode = !!msg.payload.enabled;
    ui.showToast(state.democraticMode ? 'Democratic mode enabled' : 'Democratic mode disabled');
    ui.render(true);
  };

  h.handleError = (msg) => {
    const message = msg.payload?.message || 'Unknown error';
    console.error('[OpenWatchParty] Server error:', message);
    ui.showToast(message);
    if (msg.payload?.reason === 'wrong_password' && msg.room) {
      ui.promptJoinWithPassword(msg.room);
    }
  };
})();
