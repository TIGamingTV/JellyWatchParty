(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const ui = OWP.ui = OWP.ui || {};
  const utils = OWP.utils;

  // Bridges a native-client Jellyfin session (e.g. Fladder, which can't run
  // this injected script at all) in as an OpenWatchParty room host. See
  // docs/ARCHITECTURE.md "Native Client Bridge" — this only sets up the
  // *host* side; other users still join the resulting room themselves from
  // the normal room list below, exactly as any other room.

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
    row.className = 'owp-room-item';
    row.style.cursor = 'default';
    row.innerHTML = `<div>${label}</div><button class="owp-btn ${buttonClass}">${buttonText}</button>`;
    row.querySelector('button').onclick = onClick;
    return row;
  };

  const buildSessionRow = (session) => {
    const label = `${utils.escapeHtml(session.userName)} — ${utils.escapeHtml(session.deviceName)}` +
      (session.nowPlayingItemName ? `: ${utils.escapeHtml(session.nowPlayingItemName)}` : '');
    return buildActionRow(label, 'Start', 'secondary', () => startBridge(session.sessionId));
  };

  const buildActiveRow = (bridge) => {
    const status = bridge.roomId ? `room ${utils.escapeHtml(bridge.roomId)}` : 'connecting…';
    const label = `${utils.escapeHtml(bridge.userName)} — ${status}${bridge.connected ? '' : ' (disconnected)'}`;
    return buildActionRow(label, 'Stop', 'danger', () => stopBridge(bridge.sessionId));
  };

  const updateBridgeListUI = () => {
    Promise.all([
      apiFetch('/OpenWatchParty/Bridge/Sessions').then((r) => (r.ok ? r.json() : [])),
      apiFetch('/OpenWatchParty/Bridge/Status').then((r) => (r.ok ? r.json() : []))
    ]).then(([sessions, bridges]) => {
      renderList('owp-bridge-active', bridges, 'No active bridges.', buildActiveRow);
      renderList('owp-bridge-available', sessions, 'No other sessions are playing something.', buildSessionRow);
    }).catch((err) => {
      console.warn('[OpenWatchParty] Failed to load bridge sessions:', err);
    });
  };

  const startBridge = (sessionId) => {
    apiFetch(`/OpenWatchParty/Bridge/${encodeURIComponent(sessionId)}/Start`, { method: 'POST' })
      .then((r) => {
        if (!r.ok) return r.json().then((e) => { throw new Error(e.error || 'Failed to start bridge'); });
        updateBridgeListUI();
      })
      .catch((err) => ui.showToast(err.message || 'Failed to start bridge'));
  };

  const stopBridge = (sessionId) => {
    apiFetch(`/OpenWatchParty/Bridge/${encodeURIComponent(sessionId)}/Stop`, { method: 'POST' })
      .then(() => updateBridgeListUI())
      .catch((err) => ui.showToast(err.message || 'Failed to stop bridge'));
  };

  Object.assign(ui, { updateBridgeListUI });
})();
