(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const actions = OWP.actions = OWP.actions || {};
  const state = OWP.state;
  const utils = OWP.utils;

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
      console.warn('[OpenWatchParty] Could not get username from Jellyfin:', e);
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

  const scheduleTokenRefresh = (expiresInSec) => {
    if (state.tokenRefreshTimer) {
      clearTimeout(state.tokenRefreshTimer);
      state.tokenRefreshTimer = null;
    }
    const refreshBeforeMs = Math.min(5 * 60 * 1000, expiresInSec * 1000 * 0.2);
    const refreshInMs = Math.max(0, (expiresInSec * 1000) - refreshBeforeMs);
    if (refreshInMs > 0) {
      console.log('[OpenWatchParty] Token refresh scheduled in', Math.round(refreshInMs / 1000), 's');
      state.tokenRefreshTimer = setTimeout(async () => {
        console.log('[OpenWatchParty] Refreshing auth token...');
        state.authToken = null;
        const newToken = await fetchAuthToken();
        if (newToken && state.ws && state.ws.readyState === WebSocket.OPEN) {
          state.ws.send(JSON.stringify({
            type: 'auth',
            payload: { token: newToken, user_name: state.userName, user_id: state.userId },
            ts: utils.nowMs()
          }));
          console.log('[OpenWatchParty] Token refreshed and re-authenticated');
        }
      }, refreshInMs);
    }
  };

  const fetchAuthToken = async () => {
    try {
      const apiAccess = getApiAccessToken();
      if (!apiAccess) {
        console.warn('[OpenWatchParty] ApiClient not available, auth disabled');
        state.userName = getJellyfinUsername();
        return null;
      }
      const { accessToken, serverAddress } = apiAccess;
      const tokenUrl = `${serverAddress}/OpenWatchParty/Token`;
      const response = await fetch(tokenUrl, {
        headers: { 'X-Emby-Token': accessToken }
      });
      if (!response.ok) {
        console.warn('[OpenWatchParty] Failed to fetch auth token:', response.status);
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
      if (data.quality) {
        state.quality.maxBitrate = data.quality.default_max_bitrate || 0;
        state.quality.preferDirectPlay = data.quality.prefer_direct_play !== false;
        state.quality.allowHostControl = data.quality.allow_host_quality_control !== false;
        console.log('[OpenWatchParty] Quality settings:', state.quality);
      }
      if (data.auth_enabled && data.token) {
        state.authToken = data.token;
        const expiresIn = data.expires_in || 3600;
        state.tokenExpiresAt = Date.now() + (expiresIn * 1000);
        scheduleTokenRefresh(expiresIn);
        console.log('[OpenWatchParty] Auth token obtained for user:', state.userName, 'expires in', expiresIn, 's');
        return data.token;
      }
      console.log('[OpenWatchParty] Server auth disabled, connecting without token');
      return null;
    } catch (err) {
      console.warn('[OpenWatchParty] Error fetching auth token:', err);
      state.userName = getJellyfinUsername();
      return null;
    }
  };

  Object.assign(actions, { fetchAuthToken });
})();
