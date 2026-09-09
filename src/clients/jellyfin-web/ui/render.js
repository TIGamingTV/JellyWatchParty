(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { PANEL_ID, BTN_ID, DEFAULT_WS_URL, SYNC_HIDE_STYLE_ID } = JWP.constants;
  const GLOBAL_BTN_ID = 'jwp-global-btn';

  // Jellyfin's built-in SyncPlay button is `.headerSyncButton` (also carries
  // `.syncButton`) — rendered in the app header and, during playback, in the
  // player OSD header (see jellyfin-web libraryMenu.js / videoosd.scss). When
  // the admin enables "Hide native SyncPlay button", JellyWatchParty's own
  // watch-party controls replace it, so we hide it via an injected stylesheet.
  // CSS (rather than removing the node) survives Jellyfin's SPA re-renders,
  // which repeatedly rebuild the header DOM.
  const applyNativeSyncButtonVisibility = () => {
    const existing = document.getElementById(SYNC_HIDE_STYLE_ID);
    if (state.hideNativeSyncButton) {
      if (existing) return;
      const style = document.createElement('style');
      style.id = SYNC_HIDE_STYLE_ID;
      style.textContent = '.headerSyncButton, .syncButton { display: none !important; }';
      document.head.appendChild(style);
    } else if (existing) {
      existing.remove();
    }
  };

  const togglePanel = (e) => {
    if (e) {
      e.stopPropagation();
      e.preventDefault();
    }
    const panel = document.getElementById(PANEL_ID);
    if (!panel) return;
    panel.classList.toggle('hide');
    if (!panel.classList.contains('hide')) render(true);
  };

  const renderLobby = (panel) => {
    // The native-client host bridge is an opt-in admin feature: only surface
    // the "Host From Another Device" picker when an admin has enabled it.
    const bridgeSection = state.allowThirdPartyHost ? `
          <div class="jwp-section" style="border-top: 1px solid #333; padding-top: 15px;">
            <div class="jwp-label">Host From Another Device (e.g. Fladder)</div>
            <div id="jwp-bridge-active"></div>
            <div id="jwp-bridge-available"></div>
          </div>` : '';
    panel.innerHTML = `
      <div class="jwp-header"><span>JellyWatchParty</span> <span id="jwp-ws-indicator"></span></div>
      <div class="jwp-lobby-container">
          <div class="jwp-section">
            <div class="jwp-label">Available Rooms</div>
            <div id="jwp-room-list"></div>
          </div>
          <div class="jwp-section" style="border-top: 1px solid #333; padding-top: 15px;">
            <button class="jwp-btn" style="width:100%" id="jwp-btn-create">Create Room</button>
          </div>
          ${bridgeSection}
      </div>
      <div class="jwp-footer" id="jwp-server-footer">Server: ${(state.wsUrl || DEFAULT_WS_URL).replace(/^wss?:\/\//, '').replace('/ws', '')}</div>
    `;
    const btn = panel.querySelector('#jwp-btn-create');
    if (btn) btn.onclick = async () => {
      if (!JWP.actions || !JWP.actions.createRoom) return;
      const password = await ui.promptText({
        title: 'Room password (optional, leave blank for none):',
        placeholder: 'Password',
        submitLabel: 'Create Room'
      });
      if (password === null) return; // cancelled — don't create a room
      JWP.actions.createRoom(password);
    };
    ui.updateRoomListUI();
    ui.updateBridgeListUI();
  };

  const renderRoom = (panel) => {
    const syncIndicator = ui.buildSyncStatusIndicator();
    // Attaching a supported client (e.g. Android TV) as a receiver of this
    // room is an opt-in admin feature: only surface the picker when enabled.
    const bridgeSection = state.allowSupportedReceiver ? `
      <div class="jwp-section" style="border-top: 1px solid #333; padding-top: 12px; flex-shrink:0;">
        <div class="jwp-label">Add a Device to This Room</div>
        <div id="jwp-bridge-active"></div>
        <div id="jwp-bridge-available"></div>
      </div>` : '';
    panel.innerHTML = `
      <div class="jwp-header">
        <span style="color:#69f0ae">\u25CF</span>
        <span style="flex-grow:1; margin-left:8px;">${utils.escapeHtml(state.roomName)}</span>
        <button class="jwp-btn danger" id="jwp-btn-leave">${state.isHost ? 'Close' : 'Leave'}</button>
      </div>
      <div class="jwp-section" style="flex-shrink:0;">
        <div class="jwp-label">Participants</div>
        <div id="jwp-participants-list" style="font-size:13px;">Online: ${state.participantCount || 1}</div>
        ${syncIndicator}
      </div>
      <div id="jwp-chat-section">
        <div class="jwp-label">Chat <span id="jwp-chat-badge" class="jwp-chat-badge"></span></div>
        <div id="jwp-chat-messages"></div>
        <div id="jwp-chat-input-container">
          <input type="text" id="jwp-chat-input" placeholder="Type a message..." maxlength="500">
          <button id="jwp-chat-send">Send</button>
        </div>
      </div>
      ${bridgeSection}
      <div class="jwp-meta" style="font-size:10px; color:#666; display:flex; justify-content:space-between; flex-shrink:0; padding-top:8px;">
          <span>RTT: <span class="jwp-latency">-</span></span>
          <span>ID: ${state.clientId.split('-')[1] || '...'}</span>
      </div>
    `;
    const leaveBtn = panel.querySelector('#jwp-btn-leave');
    if (leaveBtn) leaveBtn.onclick = () => JWP.actions && JWP.actions.leaveRoom && JWP.actions.leaveRoom();
    ui.updateBridgeListUI();
  };

  const setupChatInput = (panel) => {
    const chatInput = panel.querySelector('#jwp-chat-input');
    const chatSend = panel.querySelector('#jwp-chat-send');
    if (!chatInput || !chatSend) return;
    ui.stopPlayerCapture(chatInput);
    chatInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        if (JWP.chat && JWP.chat.send(chatInput.value)) {
          chatInput.value = '';
        }
      }
    });
    chatSend.addEventListener('click', () => {
      if (JWP.chat && JWP.chat.send(chatInput.value)) {
        chatInput.value = '';
      }
    });
    if (JWP.chat) {
      JWP.chat.markRead();
      JWP.chat.renderAllMessages();
    }
  };

  const render = (forceFullRender = false) => {
    const panel = document.getElementById(PANEL_ID);
    if (!panel) return;
    if (!forceFullRender && panel.dataset.inRoom === String(state.inRoom) && panel.children.length > 0) {
      ui.updateStatusIndicator();
      ui.updateServerFooter();
      ui.updateSyncIndicator();
      ui.updateRoomListUI();
      ui.updateBridgeListUI();
      ui.renderHomeWatchParties();
      return;
    }
    panel.dataset.inRoom = String(state.inRoom);
    if (!state.inRoom) {
      renderLobby(panel);
    } else {
      renderRoom(panel);
      setupChatInput(panel);
    }
    ui.updateStatusIndicator();
    ui.renderHomeWatchParties();
  };

  const injectOsdButton = () => {
    if (document.getElementById(BTN_ID)) return;
    const videoOsd = document.querySelector('.videoOsdBottom .buttons');
    if (!videoOsd) return;
    const btn = document.createElement('button');
    btn.id = BTN_ID;
    btn.className = 'paper-icon-button-light btnWatchParty autoSize';
    btn.title = 'Watch Party';
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;
    const favBtn = videoOsd.querySelector('[title="Add to favorites"], [title="Remove from favorites"]');
    if (favBtn) {
      favBtn.insertAdjacentElement('beforebegin', btn);
    } else {
      videoOsd.appendChild(btn);
    }
  };

  // Jellyfin 12's default "modern" (React/MUI) layout wraps the entire legacy
  // header DOM in a display:none ancestor — RootAppRouter.tsx renders
  // `<AppHeader isHidden={layoutManager.modern || isNewLayoutPath} />`, and
  // apphost.js's getDefaultLayout() returns the modern layout unconditionally
  // for any normal browser — even though scripts/libraryMenu.js still builds
  // .headerRight into that hidden subtree completely unconditionally. So
  // .headerRight still exists in the DOM on v12, it's just invisible; a plain
  // existence check can't tell the two situations apart. Mirrors jQuery's
  // :visible technique (verified against jellyfin-web's release-12.z source,
  // not assumed).
  const isRendered = (el) => !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);

  // Jellyfin 10.11, and Jellyfin 12 only if the user manually opts back into
  // the legacy layout in Settings: insert into .headerRight as a native icon
  // button, like the rest of Jellyfin's own header buttons. Once inserted, no
  // further per-tick management is needed — .headerRight's own visibility is
  // entirely CSS-driven by Jellyfin's header wrapper, so the button shows and
  // hides itself in step with it automatically.
  const tryInjectLegacyHeader = () => {
    const headerRight = document.querySelector('.headerRight') || document.querySelector('.skinHeader .headerRight');
    if (!headerRight || !isRendered(headerRight)) return false;

    const btn = document.createElement('button');
    btn.id = GLOBAL_BTN_ID;
    btn.className = 'paper-icon-button-light jwp-global-btn';
    btn.type = 'button';
    btn.title = 'JellyWatchParty';
    btn.setAttribute('aria-label', 'JellyWatchParty');
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;

    headerRight.prepend(btn);
    return true;
  };

  // Jellyfin 12's default layout renders a React/MUI toolbar instead
  // (components/toolbar/AppToolbar.tsx), which has no .headerRight
  // equivalent at all. Its user-menu avatar button is the only stable anchor
  // available, identified by aria-controls="app-user-menu"
  // (components/toolbar/UserMenuButton.tsx). That toolbar (and therefore the
  // avatar) is absent on the video OSD and on public paths like login/
  // select-server (both pass isUserMenuAvailable={false}), which is what we
  // want since those already have no header button today either. The
  // dashboard/admin app — including this plugin's own config page — reuses
  // the exact same toolbar and avatar though, so "avatar present" alone can't
  // tell a library page from an admin page; apps/dashboard/AppLayout.tsx
  // additionally tags `document.body` with the `dashboardDocument` class for
  // its own CSS scoping, reused here for the same purpose.
  const isMuiToolbarButtonAllowed = () =>
    !!document.querySelector('[aria-controls="app-user-menu"]') &&
    !document.body.classList.contains('dashboardDocument');

  // React owns the MUI toolbar's DOM and wipes any node inserted into it
  // directly the next time it re-renders, so the button lives on
  // document.body with position:fixed instead and is kept aligned with the
  // avatar on every poll/resize. The avatar sits in its own flex box,
  // immediately preceded by a sibling box holding whichever of SyncPlay/
  // RemotePlay/Search the current page renders, packed against the avatar —
  // anchor to the first visible one of those instead of a fixed offset from
  // the avatar, so the button doesn't render on top of it on pages where one
  // of them is present.
  const positionMuiGlobalButton = (btn) => {
    const avatar = document.querySelector('[aria-controls="app-user-menu"]');
    if (!avatar) return;
    const avatarRect = avatar.getBoundingClientRect();

    let leftAnchorRect = avatarRect;
    const actionsGroup = avatar.parentElement && avatar.parentElement.previousElementSibling;
    if (actionsGroup) {
      for (let i = 0; i < actionsGroup.children.length; i++) {
        const rect = actionsGroup.children[i].getBoundingClientRect();
        if (rect.width || rect.height) {
          leftAnchorRect = rect;
          break;
        }
      }
    }

    btn.style.top = `${Math.round(avatarRect.top + (avatarRect.height - 40) / 2)}px`;
    btn.style.left = `${Math.round(leftAnchorRect.left - 40)}px`;
  };

  const tryInjectMuiToolbar = () => {
    if (!isMuiToolbarButtonAllowed()) return false;

    const btn = document.createElement('button');
    btn.id = GLOBAL_BTN_ID;
    btn.type = 'button';
    btn.title = 'JellyWatchParty';
    btn.setAttribute('aria-label', 'JellyWatchParty');
    btn.className = 'jwp-global-btn jwp-global-btn-floating';
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;

    document.body.appendChild(btn);
    positionMuiGlobalButton(btn);
    return true;
  };

  const injectGlobalButton = () => {
    const existing = document.getElementById(GLOBAL_BTN_ID);
    if (existing) {
      // Only the v12 floating button (parented directly to document.body)
      // needs active upkeep here — the legacy .headerRight button's
      // visibility is entirely CSS-driven once inserted (see above).
      if (existing.parentElement === document.body) {
        if (!isMuiToolbarButtonAllowed()) {
          existing.remove();
        } else {
          positionMuiGlobalButton(existing);
        }
      }
      return;
    }
    // Try the v10.11-style DOM first; fall back to the v12 MUI toolbar.
    if (!tryInjectLegacyHeader()) {
      tryInjectMuiToolbar();
    }
  };

  // Reposition immediately on resize rather than waiting for the next
  // UI_CHECK_MS poll (see app/lifecycle.js). Guarded because window is a
  // plain object (no addEventListener) in the node:test harness; a no-op
  // there is fine since these tests drive injectGlobalButton() directly.
  if (typeof window.addEventListener === 'function') {
    window.addEventListener('resize', () => {
      const existing = document.getElementById(GLOBAL_BTN_ID);
      if (existing && existing.parentElement === document.body) positionMuiGlobalButton(existing);
    });
  }

  Object.assign(ui, { render, injectOsdButton, injectGlobalButton, applyNativeSyncButtonVisibility });
})();
