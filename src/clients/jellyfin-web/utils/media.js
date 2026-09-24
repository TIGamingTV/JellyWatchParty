(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const utils = JWP.utils = JWP.utils || {};

  const getCurrentItem = () => {
    const pm = utils.getPlaybackManager();
    if (!pm) return null;
    if (typeof pm.getCurrentItem === 'function') return pm.getCurrentItem();
    if (typeof pm.currentItem === 'function') return pm.currentItem();
    return pm.currentItem || pm._currentItem || null;
  };

  const getItemIdFromGlobals = () => {
    try {
      if (window.NowPlayingItem?.Id) return window.NowPlayingItem.Id;
      if (window.Emby?.Page?.currentItem?.Id) return window.Emby.Page.currentItem.Id;
      if (window.appRouter?.currentRouteInfo?.options?.item?.Id) {
        return window.appRouter.currentRouteInfo.options.item.Id;
      }
      const playbackInfo = sessionStorage.getItem('playbackInfo');
      if (playbackInfo) {
        const info = JSON.parse(playbackInfo);
        if (info?.ItemId && /^[a-f0-9]{32}$/i.test(info.ItemId)) return info.ItemId;
      }
    } catch (e) { /* ignore */ }
    const pm = utils.getPlaybackManager();
    if (pm) {
      const item = getCurrentItem();
      if (item?.Id) return item.Id;
    }
    return null;
  };

  const getItemIdFromDom = () => {
    const titleEl = document.querySelector('.osdTitle[data-id], .videoOsdTitle[data-id], [class*="osd"] [data-id]');
    if (titleEl?.dataset?.id && /^[a-f0-9]{32}$/i.test(titleEl.dataset.id)) {
      return titleEl.dataset.id;
    }
    const itemIdEl = document.querySelector('.videoOsd [data-itemid], .videoOsdBottom [data-itemid]');
    if (itemIdEl?.dataset?.itemid && /^[a-f0-9]{32}$/i.test(itemIdEl.dataset.itemid)) {
      return itemIdEl.dataset.itemid;
    }
    return null;
  };

  const getItemIdFromUrl = () => {
    const hash = window.location.hash || '';
    const patterns = [
      /[?&]id=([a-f0-9]{32})/i,
      /\/items\/([a-f0-9]{32})/i,
      /\/videos\/([a-f0-9]{32})/i,
      /id=([a-f0-9]{32})/i
    ];
    for (const pattern of patterns) {
      const match = hash.match(pattern);
      if (match) return match[1];
    }
    return null;
  };

  const ITEM_ID_RE = /^[a-f0-9]{32}$/i;

  // Normalizes a Jellyfin GUID (dashed or not) to the 32-char hex form used
  // everywhere else in the client and validated by the session server.
  const normalizeItemId = (id) => {
    if (typeof id !== 'string') return null;
    const compact = id.replace(/-/g, '');
    return ITEM_ID_RE.test(compact) ? compact.toLowerCase() : null;
  };

  // Authenticated fetch against the Jellyfin server via the page's ApiClient.
  //
  // Sends the token as `Authorization: MediaBrowser ...`, since Jellyfin 12
  // rejects the legacy `X-Emby-Token` header by default (see
  // utils.buildAuthHeader in utils/misc.js for details). X-Emby-Token is
  // kept alongside it for older servers that only understand that header.
  const apiFetch = (path, options) => {
    const apiClient = window.ApiClient;
    if (!apiClient) return Promise.reject(new Error('ApiClient not available'));
    const token = typeof apiClient.accessToken === 'function' ? apiClient.accessToken() : null;
    const serverAddress = typeof apiClient.serverAddress === 'function' ? apiClient.serverAddress() : '';
    const authHeaders = token
      ? { Authorization: utils.buildAuthHeader(apiClient, token), 'X-Emby-Token': token }
      : {};
    const headers = Object.assign({}, options && options.headers, authHeaders);
    return fetch(`${serverAddress}${path}`, Object.assign({}, options, { headers }));
  };

  const getDeviceId = () => {
    const apiClient = window.ApiClient;
    if (!apiClient) return '';
    if (typeof apiClient.deviceId === 'function') return apiClient.deviceId() || '';
    return apiClient._deviceId || '';
  };

  const getUserId = () => {
    const apiClient = window.ApiClient;
    if (!apiClient) return '';
    return (typeof apiClient.getCurrentUserId === 'function' && apiClient.getCurrentUserId())
      || apiClient._currentUserId || '';
  };

  // The server's view of this browser's own session. Independent of
  // jellyfin-web internals, so it keeps working when playbackManager is not
  // exposed globally (Jellyfin 12.1+). Resolves to null when unavailable.
  const getOwnSession = async () => {
    const deviceId = getDeviceId();
    if (!deviceId) return null;
    const res = await apiFetch(`/Sessions?deviceId=${encodeURIComponent(deviceId)}`);
    if (!res) return null;
    if (!res.ok) {
      console.warn('[JellyWatchParty] GET /Sessions failed with status', res.status);
      return null;
    }
    const sessions = await res.json();
    if (!Array.isArray(sessions)) return null;
    const userId = normalizeItemId(getUserId());
    const own = sessions.filter((s) => s && s.DeviceId === deviceId);
    const match = own.find((s) => userId && normalizeItemId(s.UserId) === userId) || own[0];
    if (!match) return null;
    return {
      id: match.Id,
      nowPlayingItemId: normalizeItemId(match.NowPlayingItem && match.NowPlayingItem.Id)
    };
  };

  const getCurrentItemId = () => {
    return getItemIdFromGlobals() || getItemIdFromDom() || getItemIdFromUrl()
      || (JWP.state && JWP.state.serverNowPlayingId) || null;
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  // Updates the cached server-side now-playing id. Returns the id or null.
  const refreshServerNowPlaying = async () => {
    try {
      const session = await getOwnSession();
      const id = session && session.nowPlayingItemId;
      if (id && JWP.state) JWP.state.serverNowPlayingId = id;
      return id || null;
    } catch (e) {
      return null;
    }
  };

  const clearServerNowPlaying = () => {
    if (JWP.state) JWP.state.serverNowPlayingId = '';
  };

  // Async variant of getCurrentItemId: when local detection fails, asks the
  // server which item this session is playing. The server only learns that
  // after the player's first progress report, so retry briefly.
  const resolveCurrentItemId = async ({ retries = 4, delayMs = 500 } = {}) => {
    const local = getItemIdFromGlobals() || getItemIdFromDom() || getItemIdFromUrl();
    if (local) return local;
    for (let attempt = 0; attempt <= retries; attempt++) {
      const id = await refreshServerNowPlaying();
      if (id) return id;
      if (attempt < retries) await sleep(delayMs);
    }
    return (JWP.state && JWP.state.serverNowPlayingId) || null;
  };

  Object.assign(utils, {
    getCurrentItem,
    getCurrentItemId,
    resolveCurrentItemId,
    refreshServerNowPlaying,
    clearServerNowPlaying,
    getOwnSession,
    apiFetch,
    normalizeItemId
  });
})();
