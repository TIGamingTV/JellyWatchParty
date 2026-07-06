(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const actions = JWP.actions = JWP.actions || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const ui = JWP.ui;
  const { DEFAULT_WS_URL, RECONNECT_BASE_MS, RECONNECT_MAX_MS, PING_INIT_MS, PING_STABLE_MS, PING_STABLE_AFTER } = JWP.constants;

  const CLIENT_ID_STORAGE_KEY = 'owp_persistent_client_id';

  const generateUuid = () => {
    if (window.crypto && typeof window.crypto.randomUUID === 'function') {
      return window.crypto.randomUUID();
    }
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
      const r = (Math.random() * 16) | 0;
      const v = c === 'x' ? r : (r & 0x3) | 0x8;
      return v.toString(16);
    });
  };

  const getPersistentClientId = () => {
    try {
      let id = window.localStorage.getItem(CLIENT_ID_STORAGE_KEY);
      if (!id) {
        id = generateUuid();
        window.localStorage.setItem(CLIENT_ID_STORAGE_KEY, id);
      }
      return id;
    } catch (err) {
      if (!state.sessionOnlyClientId) state.sessionOnlyClientId = generateUuid();
      return state.sessionOnlyClientId;
    }
  };

  const withClientId = (baseUrl, clientId) => {
    const separator = baseUrl.includes('?') ? '&' : '?';
    return `${baseUrl}${separator}client_id=${encodeURIComponent(clientId)}`;
  };

  const onWsOpen = (token) => {
    console.log('[JellyWatchParty] WebSocket connected');
    state.isConnecting = false;
    state.reconnectAttempts = 0;
    if (utils.flushLogBuffer) utils.flushLogBuffer();
    const authPayload = {};
    if (token) authPayload.token = token;
    if (state.userName) authPayload.user_name = state.userName;
    if (state.userId) authPayload.user_id = state.userId;
    if (Object.keys(authPayload).length > 0) {
      state.ws.send(JSON.stringify({ type: 'auth', payload: authPayload, ts: utils.nowMs() }));
    }
    actions.send('ping', { client_ts: utils.nowMs() });
    schedulePing();
    ui.render();
  };

  const onWsClose = (e) => {
    console.log('[JellyWatchParty] WebSocket closed:', e.code, e.reason);
    state.isConnecting = false;
    state.successfulPings = 0;
    state.timeSyncSamples = [];
    ui.render();
    if (state.autoReconnect && !state.isConnecting) {
      const delay = Math.min(
        RECONNECT_BASE_MS * Math.pow(2, state.reconnectAttempts),
        RECONNECT_MAX_MS
      );
      state.reconnectAttempts++;
      console.log(`[JellyWatchParty] Reconnecting in ${delay}ms (attempt ${state.reconnectAttempts})`);
      setTimeout(connect, delay);
    }
  };

  const handleMessage = (msg) => {
    const video = utils.getVideo();
    console.log('[JellyWatchParty] Received:', msg.type, msg);
    const h = JWP._wsHandlers;
    switch (msg.type) {
      case 'room_list': h.handleRoomList(msg); break;
      case 'client_hello': h.handleClientHello(msg); break;
      case 'room_state': h.handleRoomState(msg, video); break;
      case 'participants_update': h.handleParticipantsUpdate(msg); break;
      case 'client_left': h.handleClientLeft(msg); break;
      case 'room_closed': h.handleRoomClosed(msg); break;
      case 'host_changed': h.handleHostChanged(msg); break;
      case 'player_event': h.handlePlayerEvent(msg, video); break;
      case 'state_update': h.handleStateUpdate(msg, video); break;
      case 'pong': h.handlePong(msg); break;
      case 'chat_message': if (JWP.chat && msg.payload) JWP.chat.receive(msg); break;
      case 'error': h.handleError(msg); break;
    }
  };

  const connect = async () => {
    if (state.isConnecting) {
      console.log('[JellyWatchParty] Connection already in progress, skipping');
      return;
    }
    if (state.ws && state.ws.readyState === WebSocket.OPEN) {
      console.log('[JellyWatchParty] Already connected, skipping');
      return;
    }
    state.isConnecting = true;
    if (state.ws) {
      const wasAutoReconnect = state.autoReconnect;
      state.autoReconnect = false;
      state.ws.close();
      state.ws = null;
      state.autoReconnect = wasAutoReconnect;
    }
    let token = state.authToken;
    if (!token) {
      token = await actions.fetchAuthToken();
    }
    const wsUrl = state.wsUrl || DEFAULT_WS_URL;
    const fullWsUrl = withClientId(wsUrl, getPersistentClientId());
    console.log('[JellyWatchParty] Connecting to WebSocket:', fullWsUrl);
    if (wsUrl.startsWith('ws://') && window.location.protocol === 'https:') {
      console.warn('[JellyWatchParty] WARNING: Using insecure WebSocket (ws://) on HTTPS page. Data may be intercepted.');
    }
    try {
      state.ws = new WebSocket(fullWsUrl);
    } catch (err) {
      console.error('[JellyWatchParty] Failed to create WebSocket:', err);
      state.isConnecting = false;
      return;
    }
    state.ws.onopen = () => onWsOpen(token);
    state.ws.onerror = (err) => {
      console.error('[JellyWatchParty] WebSocket error:', err);
      state.isConnecting = false;
    };
    state.ws.onclose = onWsClose;
    state.ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (!state.inRoom || msg.room === state.roomId || !msg.room || msg.type === 'room_state') {
          handleMessage(msg);
        }
      } catch (err) {
        console.error('[JellyWatchParty] Failed to parse message:', err.message, 'Data:', e.data?.substring?.(0, 100));
      }
    };
  };

  const schedulePing = () => {
    if (state.intervals.ping) clearInterval(state.intervals.ping);
    const interval = state.successfulPings >= PING_STABLE_AFTER
      ? PING_STABLE_MS
      : PING_INIT_MS;
    state.intervals.ping = setInterval(() => {
      if (state.ws && state.ws.readyState === 1) {
        actions.send('ping', { client_ts: utils.nowMs() });
      }
    }, interval);
  };

  Object.assign(actions, { connect, schedulePing });
})();
