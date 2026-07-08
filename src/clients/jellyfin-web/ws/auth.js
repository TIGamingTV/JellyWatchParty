(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const actions = JWP.actions = JWP.actions || {};
  const state = JWP.state;
  const utils = JWP.utils;

  const getJellyfinUsername = () => {
    try {
      const apiClient = window.ApiClient;
      if (apiClient) {
        if (apiClient._currentUser?.Name) return apiClient._currentUser.Name;
        if (apiClient.currentUser?.()?.Name) return apiClient.currentUser().Name;
      }
      const creds = localStorage.getItem('jellyfin_credentials') || sessionStorage.getItem('jellyfin_credentials');
      if (creds) {
        const parsed = JSON.parse(creds);
        const server = parsed?.Servers?.[0];
        if (server?.Users?.[0]?.Name) return server.Users[0].Name;
      }
      const serverCreds = JSON.parse(localStorage.getItem('_deviceId2') || '{}');
      if (serverCreds?.Servers?.[0]?.Users?.[0]?.Name) return serverCreds.Servers[0].Users[0].Name;
    } catch (e) {
      console.warn('[JellyWatchParty] Could not get username from Jellyfin:', e);
    }
    return '';
  };

  const getApiAccessToken = () => {
    const apiClient = window.ApiClient;
    if (!apiClient || typeof apiClient.accessToken !== 'function') return null;
    const accessToken = apiClient.accessToken();
    if (!accessToken) return null;
    const serverAddress = typeof apiClient.serverAddress === 'function' ? apiClient.serverAddress() : '';
    return { apiClient, accessToken, serverAddress };
  };

  const waitForApiClient = (maxWaitMs = 10000, intervalMs = 250) => {
    return new Promise((resolve) => {
      let elapsed = 0;
      const check = () => {
        const result = getApiAccessToken();
        if (result) return resolve(result);
        elapsed += intervalMs;
        if (elapsed >= maxWaitMs) return resolve(null);
        setTimeout(check, intervalMs);
      };
      check();
    });
  };

  const scheduleTokenRefresh = (expiresInSec) => {
    if (state.tokenRefreshTimer) {
      clearTimeout(state.tokenRefreshTimer);
      state.tokenRefreshTimer = null;
    }
    const refreshBeforeMs = Math.min(5 * 60 * 1000, expiresInSec * 1000 * 0.2);
    const refreshInMs = Math.max(0, (expiresInSec * 1000) - refreshBeforeMs);
    if (refreshInMs > 0) {
      console.log('[JellyWatchParty] Token refresh scheduled in', Math.round(refreshInMs / 1000), 's');
      state.tokenRefreshTimer = setTimeout(async () => {
        console.log('[JellyWatchParty] Refreshing auth token...');
        state.authToken = null;
        const newToken = await fetchAuthToken();
        if (newToken && state.ws && state.ws.readyState === WebSocket.OPEN) {
          state.ws.send(JSON.stringify({
            type: 'auth',
            payload: { token: newToken, user_name: state.userName, user_id: state.userId },
            ts: utils.nowMs()
          }));
          console.log('[JellyWatchParty] Token refreshed and re-authenticated');
        }
      }, refreshInMs);
    }
  };

  const fetchAuthToken = async () => {
    try {
      let apiAccess = getApiAccessToken();
      if (!apiAccess) {
        console.log('[JellyWatchParty] Waiting for ApiClient...');
        apiAccess = await waitForApiClient();
      }
      if (!apiAccess) {
        console.warn('[JellyWatchParty] ApiClient not available after waiting, auth disabled');
        state.userName = getJellyfinUsername();
        return null;
      }
      const { accessToken, serverAddress } = apiAccess;
      const tokenUrl = `${serverAddress}/JellyWatchParty/Token`;
      const response = await fetch(tokenUrl, {
        headers: { 'X-Emby-Token': accessToken }
      });
      if (!response.ok) {
        console.warn('[JellyWatchParty] Failed to fetch auth token:', response.status);
        state.userName = getJellyfinUsername();
        return null;
      }
      const data = await response.json();
      state.authEnabled = data.auth_enabled || false;
      state.userId = data.user_id || '';
      state.userName = data.user_name || getJellyfinUsername() || '';
      if (data.session_server_url) {
        state.wsUrl = data.session_server_url;
      }
      if (data.auth_enabled && data.token) {
        state.authToken = data.token;
        const expiresIn = data.expires_in || 3600;
        state.tokenExpiresAt = Date.now() + (expiresIn * 1000);
        scheduleTokenRefresh(expiresIn);
        console.log('[JellyWatchParty] Auth token obtained for user:', state.userName, 'expires in', expiresIn, 's');
        return data.token;
      }
      console.log('[JellyWatchParty] Server auth disabled, connecting without token');
      return null;
    } catch (err) {
      console.warn('[JellyWatchParty] Error fetching auth token:', err);
      state.userName = getJellyfinUsername();
      return null;
    }
  };

  Object.assign(actions, { fetchAuthToken });
})();
