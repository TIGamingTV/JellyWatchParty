(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const utils = JWP.utils;
  const state = JWP.state;

  // Bridges a native-client Jellyfin session (e.g. Fladder or the official
  // Android TV app, which can't run this injected script at all) into a
  // JellyWatchParty room. See docs/ARCHITECTURE.md "Native Client Bridge".
  // Two roles are offered per session:
  //   - Host: the session drives a brand-new room; other users join it from
  //     the normal room list below, exactly as any other room.
  //   - Receiver: the session is attached to the room this browser is
  //     currently in, and follows the host via remote-control commands. Only
  //     available while this browser is in a room, and the session must
  //     already be playing the room's item.

  const apiFetch = (path, options) => {
    const apiClient = window.ApiClient;
    if (!apiClient) return Promise.reject(new Error('ApiClient not available'));
    const token = typeof apiClient.accessToken === 'function' ? apiClient.accessToken() : null;
    const serverAddress = typeof apiClient.serverAddress === 'function' ? apiClient.serverAddress() : '';
    const headers = Object.assign({}, options && options.headers, token ? { 'X-Emby-Token': token } : {});
    return fetch(`${serverAddress}${path}`, Object.assign({}, options, { headers }));
  };

  const renderList = (containerId, items, emptyText, buildRow) => {
    const el = document.getElementById(containerId);
    if (!el) return;
    if (!items || !items.length) {
      el.innerHTML = `<div style="font-size:12px; color:#555; padding:8px; text-align:center;">${emptyText}</div>`;
      return;
    }
    el.innerHTML = '';
    items.forEach((item) => el.appendChild(buildRow(item)));
  };

  const buildActionRow = (label, buttonText, buttonClass, onClick) => {
    const row = document.createElement('div');
    row.className = 'jwp-room-item';
    row.style.cursor = 'default';
    row.innerHTML = `<div>${label}</div><button class="jwp-btn ${buttonClass}">${buttonText}</button>`;
    row.querySelector('button').onclick = onClick;
    return row;
  };

  // Whether this browser is currently in a room, i.e. whether a picked
  // session can be attached as a *receiver* of it.
  const inRoom = () => Boolean(state && state.inRoom && state.roomId);

  const buildSessionRow = (session) => {
    const label = `${utils.escapeHtml(session.userName)} — ${utils.escapeHtml(session.deviceName)}` +
      (session.nowPlayingItemName ? `: ${utils.escapeHtml(session.nowPlayingItemName)}` : '');
    const row = document.createElement('div');
    row.className = 'jwp-room-item';
    row.style.cursor = 'default';
    const receiverDisabled = inRoom() ? '' : ' disabled title="Join a room first"';
    row.innerHTML = `<div>${label}</div>` +
      '<div style="display:flex; gap:6px;">' +
      '<button class="jwp-btn secondary jwp-bridge-host">Host</button>' +
      `<button class="jwp-btn secondary jwp-bridge-receiver"${receiverDisabled}>Receiver</button>` +
      '</div>';
    row.querySelector('.jwp-bridge-host').onclick = () => startBridge(session.sessionId);
    const receiverBtn = row.querySelector('.jwp-bridge-receiver');
    if (inRoom()) {
      receiverBtn.onclick = () => followBridge(session.sessionId);
    }
    return row;
  };

  const buildActiveRow = (bridge) => {
    const status = bridge.roomId ? `room ${utils.escapeHtml(bridge.roomId)}` : 'connecting…';
    const role = bridge.role === 'receiver' ? 'receiver' : 'host';
    const label = `${utils.escapeHtml(bridge.userName)} — ${role} · ${status}${bridge.connected ? '' : ' (disconnected)'}`;
    return buildActionRow(label, 'Stop', 'danger', () => stopBridge(bridge.sessionId));
  };

  const updateBridgeListUI = () => {
    Promise.all([
      apiFetch('/JellyWatchParty/Bridge/Sessions').then((r) => (r.ok ? r.json() : [])),
      apiFetch('/JellyWatchParty/Bridge/Status').then((r) => (r.ok ? r.json() : []))
    ]).then(([sessions, bridges]) => {
      renderList('jwp-bridge-active', bridges, 'No active bridges.', buildActiveRow);
      renderList('jwp-bridge-available', sessions, 'No other sessions are playing something.', buildSessionRow);
    }).catch((err) => {
      console.warn('[JellyWatchParty] Failed to load bridge sessions:', err);
    });
  };

  const startBridge = (sessionId) => {
    apiFetch(`/JellyWatchParty/Bridge/${encodeURIComponent(sessionId)}/Start`, { method: 'POST' })
      .then((r) => {
        if (!r.ok) return r.json().then((e) => { throw new Error(e.error || 'Failed to start bridge'); });
        updateBridgeListUI();
      })
      .catch((err) => ui.showToast(err.message || 'Failed to start bridge'));
  };

  const buildFollowPath = (sessionId, roomId) =>
    `/JellyWatchParty/Bridge/${encodeURIComponent(sessionId)}/Follow?roomId=${encodeURIComponent(roomId)}`;

  const followBridge = (sessionId) => {
    if (!inRoom()) {
      ui.showToast('Join a room before attaching a receiver.');
      return;
    }
    apiFetch(buildFollowPath(sessionId, state.roomId), { method: 'POST' })
      .then((r) => {
        if (!r.ok) return r.json().then((e) => { throw new Error(e.error || 'Failed to attach receiver'); });
        updateBridgeListUI();
      })
      .catch((err) => ui.showToast(err.message || 'Failed to attach receiver'));
  };

  const stopBridge = (sessionId) => {
    apiFetch(`/JellyWatchParty/Bridge/${encodeURIComponent(sessionId)}/Stop`, { method: 'POST' })
      .then(() => updateBridgeListUI())
      .catch((err) => ui.showToast(err.message || 'Failed to stop bridge'));
  };

  Object.assign(ui, { updateBridgeListUI, inRoom, buildFollowPath, buildSessionRow });
})();
