(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const state = JWP.state;

  const updateStatusIndicator = () => {
    const el = document.getElementById('jwp-ws-indicator');
    if (!el) return;
    const connected = state.ws && state.ws.readyState === 1;
    el.style.color = connected ? '#69f0ae' : '#ff5252';
    el.textContent = connected ? 'Online' : 'Offline';
  };

  const updateServerFooter = () => {
    const el = document.getElementById('jwp-server-footer');
    if (!el) return;
    const wsUrl = state.wsUrl || JWP.constants.DEFAULT_WS_URL;
    el.textContent = `Server: ${wsUrl.replace(/^wss?:\/\//, '').replace('/ws', '')}`;
  };

  const describeSyncStatus = (status) => {
    if (status === 'pending_play') {
      const remaining = Math.max(0, (state.pendingPlayUntil - (Date.now() + (state.serverOffsetMs || 0))) / 1000);
      return { dotClass: 'pending', label: `Waiting for sync... ${remaining.toFixed(1)}s`, showSpinner: true };
    }
    if (status === 'syncing') {
      return { dotClass: 'syncing', label: 'Out of sync', showSpinner: false };
    }
    if (status === 'synced') {
      return { dotClass: 'synced', label: 'In sync', showSpinner: false };
    }
    if (status === 'wrong_media') {
      return { dotClass: 'syncing', label: 'Loading host\u2019s media\u2026', showSpinner: false };
    }
    return { dotClass: 'unknown', label: 'Not synced yet', showSpinner: false };
  };

  const updateSyncIndicator = () => {
    const el = document.getElementById('jwp-sync-indicator');
    if (!el || state.isHost) return;
    const { dotClass, label, showSpinner } = describeSyncStatus(state.syncStatus);
    el.innerHTML = showSpinner
      ? `<div class="jwp-sync-spinner"></div><span>${label}</span>`
      : `<div class="jwp-sync-dot ${dotClass}"></div><span>${label}</span>`;
  };

  const buildSyncStatusIndicator = () => {
    if (state.isHost) return '';
    const { dotClass, label, showSpinner } = describeSyncStatus(state.syncStatus);
    return `
      <div class="jwp-sync-status" id="jwp-sync-indicator">
        ${showSpinner ? '<div class="jwp-sync-spinner"></div>' : `<div class="jwp-sync-dot ${dotClass}"></div>`}
        <span>${label}</span>
      </div>
    `;
  };

  // Participant list: who is in the room and how their playback is doing,
  // from the statuses each client reports (see playback/sync.js).
  const PARTICIPANT_STATUS = {
    synced: { dotClass: 'synced', label: 'In sync' },
    playing: { dotClass: 'synced', label: 'Playing' },
    syncing: { dotClass: 'syncing', label: 'Catching up' },
    buffering: { dotClass: 'pending', label: 'Buffering' },
    loading: { dotClass: 'pending', label: 'Loading' },
    paused: { dotClass: 'unknown', label: 'Paused' },
    idle: { dotClass: 'unknown', label: 'Not watching' }
  };

  const describeParticipantStatus = (status) => PARTICIPANT_STATUS[status] || { dotClass: 'unknown', label: '...' };

  const buildParticipantsHtml = () => {
    const list = Array.isArray(state.participants) ? state.participants : [];
    // Older session servers only send a count.
    if (list.length === 0) return `Online: ${state.participantCount || 1}`;
    const escape = JWP.utils.escapeHtml;
    const rows = list.map((p) => {
      const { dotClass, label } = describeParticipantStatus(p.status);
      const you = p.id === state.clientId ? ' <span class="jwp-participant-you">(you)</span>' : '';
      const host = p.is_host ? '<span class="material-icons jwp-participant-host" title="Host" aria-label="Host">star</span>' : '';
      return `<div class="jwp-participant">`
        + `<div class="jwp-sync-dot ${dotClass}"></div>`
        + `<span class="jwp-participant-name">${escape(p.name || 'Someone')}${you}</span>${host}`
        + `<span class="jwp-participant-status">${label}</span>`
        + `</div>`;
    }).join('');
    return `<div class="jwp-participant-count">Online: ${list.length}</div>${rows}`;
  };

  const renderParticipants = () => {
    const el = document.getElementById('jwp-participants-list');
    if (el) el.innerHTML = buildParticipantsHtml();
  };

  // Start countdown overlay: "waiting for everyone", then 3, 2, 1 counted
  // against server time, so every client flips at the same moment.
  const COUNTDOWN_ID = 'jwp-countdown';
  let countdownTimer = null;

  const countdownEl = () => {
    let el = document.getElementById(COUNTDOWN_ID);
    if (!el && document.body) {
      el = document.createElement('div');
      el.id = COUNTDOWN_ID;
      el.setAttribute('role', 'status');
      el.setAttribute('aria-live', 'polite');
      document.body.appendChild(el);
    }
    return el;
  };

  const hideCountdown = () => {
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    const el = document.getElementById(COUNTDOWN_ID);
    if (el) el.remove();
  };

  const showStartWaiting = () => {
    hideCountdown();
    const el = countdownEl();
    if (el) el.innerHTML = '<div class="jwp-countdown-label">Waiting for everyone to be ready...</div>';
  };

  const countdownSecondsLeft = (targetServerTs) => {
    const now = JWP.utils.getServerNow();
    return Math.ceil((targetServerTs - now) / 1000);
  };

  const showCountdown = (targetServerTs) => {
    hideCountdown();
    const el = countdownEl();
    if (!el) return;
    const tick = () => {
      const left = countdownSecondsLeft(targetServerTs);
      if (left <= 0) {
        hideCountdown();
        return;
      }
      el.innerHTML = `<div class="jwp-countdown-number">${left}</div><div class="jwp-countdown-label">Starting together</div>`;
    };
    tick();
    countdownTimer = setInterval(tick, 100);
  };

  const stopPlayerCapture = (input) => {
    const stopPropagation = (e) => e.stopPropagation();
    input.addEventListener('keydown', stopPropagation);
    input.addEventListener('keyup', stopPropagation);
    input.addEventListener('keypress', stopPropagation);
    input.addEventListener('click', stopPropagation);
    input.addEventListener('mousedown', stopPropagation);
  };

  Object.assign(ui, {
    updateStatusIndicator, updateServerFooter, updateSyncIndicator, buildSyncStatusIndicator,
    describeParticipantStatus, buildParticipantsHtml, renderParticipants,
    showStartWaiting, showCountdown, hideCountdown, countdownSecondsLeft, stopPlayerCapture
  });
})();
