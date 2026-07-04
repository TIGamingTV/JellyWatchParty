(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const ui = OWP.ui = OWP.ui || {};
  const state = OWP.state;

  const updateStatusIndicator = () => {
    const el = document.getElementById('owp-ws-indicator');
    if (!el) return;
    const connected = state.ws && state.ws.readyState === 1;
    el.style.color = connected ? '#69f0ae' : '#ff5252';
    el.textContent = connected ? 'Online' : 'Offline';
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
    return { dotClass: 'unknown', label: 'Not synced yet', showSpinner: false };
  };

  const updateSyncIndicator = () => {
    const el = document.getElementById('owp-sync-indicator');
    if (!el || state.isHost) return;
    const { dotClass, label, showSpinner } = describeSyncStatus(state.syncStatus);
    el.innerHTML = showSpinner
      ? `<div class="owp-sync-spinner"></div><span>${label}</span>`
      : `<div class="owp-sync-dot ${dotClass}"></div><span>${label}</span>`;
  };

  const buildSyncStatusIndicator = () => {
    if (state.isHost) return '';
    const { dotClass, label, showSpinner } = describeSyncStatus(state.syncStatus);
    return `
      <div class="owp-sync-status" id="owp-sync-indicator">
        ${showSpinner ? '<div class="owp-sync-spinner"></div>' : `<div class="owp-sync-dot ${dotClass}"></div>`}
        <span>${label}</span>
      </div>
    `;
  };

  const stopPlayerCapture = (input) => {
    const stopPropagation = (e) => e.stopPropagation();
    input.addEventListener('keydown', stopPropagation);
    input.addEventListener('keyup', stopPropagation);
    input.addEventListener('keypress', stopPropagation);
    input.addEventListener('click', stopPropagation);
    input.addEventListener('mousedown', stopPropagation);
  };

  Object.assign(ui, { updateStatusIndicator, updateSyncIndicator, buildSyncStatusIndicator, stopPlayerCapture });
})();
