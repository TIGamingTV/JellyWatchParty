(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const utils = JWP.utils = JWP.utils || {};
  const state = JWP.state;
  const { SUPPRESS_MS } = JWP.constants;

  const shouldSend = () => utils.nowMs() > state.suppressUntil;

  const suppress = (ms = SUPPRESS_MS) => { state.suppressUntil = utils.nowMs() + ms; };

  // --- Democratic mode --------------------------------------------------------
  // Once the room has started (its first, host-only countdown play is done),
  // a democratic room lets any participant play/pause/seek for everyone.
  const canControlPlayback = () => !!(state.isHost || (state.democraticMode && state.roomStarted));

  // Where the room is now according to the last sync baseline.
  const expectedRoomPosition = () => {
    const base = state.lastSyncPosition || 0;
    if (state.lastSyncPlayState !== 'playing' || !state.lastSyncServerTs) return base;
    return base + Math.max(0, utils.getServerNow() - state.lastSyncServerTs) / 1000;
  };

  // Takes a command this client just sent as the room's new state. The
  // server never echoes it back, and a guest's syncLoop would otherwise
  // drift-correct straight back to the state from before it.
  const mirrorRoomState = (position, playState) => {
    state.lastSyncServerTs = utils.getServerNow();
    state.lastSyncPosition = position;
    state.lastSyncPlayState = playState;
  };

  // After a play/pause/seek (sent or applied), the host's next state_update
  // may already have been on its way with the old state; ignore updates that
  // disagree for a moment (see ws/handlers/sync.js handleStateUpdate).
  const armCommandGuard = (position, playState) => {
    const { COMMAND_GUARD_MS } = JWP.constants;
    state.commandGuardUntil = utils.nowMs() + COMMAND_GUARD_MS;
    state.commandGuardPlayState = playState;
    state.commandGuardPosition = typeof position === 'number' ? position : 0;
    state.commandGuardServerTs = utils.getServerNow();
  };

  // True when this state_update contradicts the guarded command and should be
  // ignored. The first one that agrees ends the guard early.
  const commandGuardBlocks = (msg) => {
    if (!state.commandGuardUntil) return false;
    if (utils.nowMs() >= state.commandGuardUntil) {
      state.commandGuardUntil = 0;
      return false;
    }
    const { COMMAND_GUARD_TOLERANCE_SEC } = JWP.constants;
    const p = (msg && msg.payload) || {};
    const ts = typeof msg.server_ts === 'number' ? msg.server_ts : utils.getServerNow();
    const playing = state.commandGuardPlayState === 'playing';
    const expected = state.commandGuardPosition
      + (playing ? Math.max(0, ts - state.commandGuardServerTs) / 1000 : 0);
    const agrees = p.play_state === state.commandGuardPlayState
      && (typeof p.position !== 'number' || Math.abs(p.position - expected) < COMMAND_GUARD_TOLERANCE_SEC);
    if (agrees) {
      state.commandGuardUntil = 0;
      return false;
    }
    return true;
  };

  // Echo filter for democratic mode: a video event that only brings this
  // client in line with the room (we were moved there by a sync, a remote
  // command or a stream reload) is not a user command. A real command moves
  // away from the room's state. Complements the time-based suppression
  // (shouldSend/isSyncing), which can expire before a slow seek completes.
  const isEchoOfRoomState = (action, video) => {
    if (action === 'play') return state.lastSyncPlayState === 'playing';
    if (action === 'pause') return state.lastSyncPlayState === 'paused';
    if (action === 'seek') {
      return !!state.lastSyncServerTs && !!video
        && Math.abs(video.currentTime - expectedRoomPosition()) < JWP.constants.SEEK_THRESHOLD;
    }
    return false;
  };

  const HTML_ENTITIES = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;'
  };

  const escapeHtml = (str) => {
    if (typeof str !== 'string') return '';
    return str.replace(/[&<>"']/g, c => HTML_ENTITIES[c]);
  };

  const getItemImageUrl = (itemId, imageTag) => {
    if (!itemId || !window.ApiClient) return '';
    const serverUrl = window.ApiClient._serverAddress || window.ApiClient.serverAddress?.() || '';
    if (!serverUrl) return '';
    let url = `${serverUrl}/Items/${itemId}/Images/Primary?quality=90`;
    if (imageTag) url += `&tag=${imageTag}`;
    return url;
  };

  const isHomeView = () => {
    if (document.querySelector('.homePage')) return true;
    const hash = window.location.hash || '';
    return hash.includes('home');
  };

  // Builds a Jellyfin `Authorization` header using the MediaBrowser scheme.
  //
  // This is the only token transport Jellyfin accepts unconditionally. The
  // X-Emby-Token header we used to rely on is gated behind the server's
  // EnableLegacyAuthorization setting, which Jellyfin 12 both defaults to
  // false for new installs and force-disables on upgrade - so on a 12 server
  // an X-Emby-Token-only request gets a 401 and the whole feature goes
  // quiet. The MediaBrowser scheme works on 10.11 and 12 alike, and also
  // survives an admin turning legacy auth off on 10.11.
  //
  // Descriptive parts are omitted rather than sent empty when ApiClient does
  // not expose them, since Jellyfin parses the header positionally by name
  // and a Device="undefined" is worse than no Device at all.
  //
  // Shared by ws/auth.js (the /JellyWatchParty/Token handshake) and
  // utils/media.js (apiFetch, used by playback session commands and the
  // native-client bridge UI) - both need it, and this file loads before
  // either.
  const buildAuthHeader = (apiClient, accessToken) => {
    const quote = (value) => `"${String(value).replace(/"/g, '')}"`;
    const part = (name, accessor) => {
      if (!apiClient || typeof apiClient[accessor] !== 'function') return null;
      let value;
      try {
        value = apiClient[accessor]();
      } catch (e) {
        return null;
      }
      if (value === null || value === undefined || value === '') return null;
      return `${name}=${quote(value)}`;
    };

    const parts = [
      part('Client', 'appName'),
      part('Device', 'deviceName'),
      part('DeviceId', 'deviceId'),
      part('Version', 'appVersion'),
      `Token=${quote(accessToken)}`
    ].filter(Boolean);

    return `MediaBrowser ${parts.join(', ')}`;
  };

  Object.assign(utils, {
    shouldSend,
    suppress,
    canControlPlayback,
    expectedRoomPosition,
    mirrorRoomState,
    armCommandGuard,
    commandGuardBlocks,
    isEchoOfRoomState,
    escapeHtml,
    getItemImageUrl,
    isHomeView,
    buildAuthHeader
  });
})();
