(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { PANEL_ID, BTN_ID, DEFAULT_WS_URL, SYNC_HIDE_STYLE_ID } = JWP.constants;
  const GLOBAL_BTN_ID = 'jwp-global-btn';
  // aria-controls value of Jellyfin 12's MUI SyncPlay button, exported as
  // `ID` from jellyfin-web's apps/modern/components/AppToolbar/menus/
  // SyncPlayMenu.tsx — the only stable selector for that button, since MUI
  // assigns it no meaningful class name of its own.
  const SYNC_PLAY_MENU_ID = 'app-sync-play-menu';

  // Jellyfin's built-in SyncPlay button is `.headerSyncButton` (also carries
  // `.syncButton`) — rendered in the app header and, during playback, in the
  // player OSD header (see jellyfin-web libraryMenu.js / videoosd.scss). On
  // Jellyfin 12's default MUI toolbar it's a different component entirely
  // (apps/modern/components/AppToolbar/SyncPlayButton.tsx), identified by
  // `aria-controls="app-sync-play-menu"` — MUI generates no stable class name
  // of its own. When the admin enables "Hide native SyncPlay button",
  // JellyWatchParty's own watch-party controls replace it, so we hide it via
  // an injected stylesheet. CSS (rather than removing the node) survives
  // Jellyfin's SPA re-renders, which repeatedly rebuild the header DOM, and
  // avoids fighting React over nodes it owns.
  //
  // `display: none` is correct for both: our own button is now a real in-flow
  // child of the same flex container (see tryInjectMuiToolbar below), so it
  // simply takes the freed slot. Earlier versions needed `visibility: hidden`
  // here purely to keep the hidden button's layout box measurable for
  // absolute-positioning math; that math is gone.
  const applyNativeSyncButtonVisibility = () => {
    const existing = document.getElementById(SYNC_HIDE_STYLE_ID);
    if (state.hideNativeSyncButton) {
      if (existing) return;
      const style = document.createElement('style');
      style.id = SYNC_HIDE_STYLE_ID;
      style.textContent = '.headerSyncButton, .syncButton { display: none !important; } '
        + `[aria-controls="${SYNC_PLAY_MENU_ID}"] { display: none !important; }`;
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
    // The trailing marker class records which strategy created this button so
    // injectGlobalButton() can tell the two apart later without re-deriving it
    // from the DOM around it.
    btn.className = 'paper-icon-button-light jwp-global-btn jwp-global-btn-legacy';
    btn.type = 'button';
    btn.title = 'JellyWatchParty';
    btn.setAttribute('aria-label', 'JellyWatchParty');
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;

    headerRight.prepend(btn);
    return true;
  };

  // Jellyfin 12's default layout renders a React/MUI toolbar instead
  // (components/toolbar/AppToolbar.tsx), which has no .headerRight equivalent.
  // Its structure, verified against release-12.z, is:
  //
  //   <Toolbar class="MuiToolbar-root ...">
  //     {children}                                        <- ServerButton/UserViewNav
  //     <Box sx={{flexGrow:1, justifyContent:'flex-end'}}> <- the "actions" box:
  //       <SyncPlayButton/><RemotePlayButton/><SearchButton/>
  //     </Box>
  //     <Box sx={{flexGrow:0}}><UserMenuButton/></Box>     <- the avatar
  //   </Toolbar>
  //
  // That actions box is the correct home for a plugin button: it is the same
  // flex container Jellyfin uses for its own toolbar actions, so an in-flow
  // child lands in the right place with no coordinate math whatsoever.
  //
  // Contrary to a long-standing assumption in this plugin, React does *not*
  // remove foreign nodes from a container it manages. React reconciles
  // against its own fiber tree, deleting only nodes it created
  // (removeChild(specificNode)) and inserting relative to its own host
  // siblings; it never enumerates or clears the real child list. The one
  // exception is hydration, and jellyfin-web mounts with createRoot
  // (utils/reactUtils.tsx), never hydrateRoot. Verified empirically against
  // the real jellyfin-web 12.0 production build in Chromium: an appended
  // button survives 200 route navigations, MUI menu open/close churn and
  // viewport/breakpoint changes untouched.
  //
  // Only a genuine unmount of the toolbar removes it — which happens exactly
  // where the button should be gone anyway (the /video OSD, where
  // apps/modern/components/AppToolbar returns null). MutationObserver-driven
  // re-injection restores it on the way back.
  const MUI_TOOLBAR_SELECTOR = '.MuiToolbar-root';
  const AVATAR_SELECTOR = '[aria-controls="app-user-menu"]';

  // Resolve the actions box via the avatar, since the avatar is the only
  // element in the toolbar with a stable, semantic selector of its own
  // (aria-controls="app-user-menu", from components/toolbar/UserMenuButton.tsx).
  // MUI assigns no meaningful class names to the two Boxes, so their identity
  // comes from position: the actions box is the toolbar child immediately
  // preceding the avatar's box.
  //
  // This naturally yields null exactly where no button should exist:
  //   - the video OSD and public paths (login/select-server) render
  //     isUserMenuAvailable={false}, so there is no avatar at all;
  //   - the legacy layout has no MUI toolbar.
  // The admin dashboard does reuse the same toolbar and avatar, so it needs
  // an explicit exclusion: apps/dashboard/AppLayout.tsx tags document.body
  // with `dashboardDocument` for its own CSS scoping, reused here.
  const findMuiActionsBox = () => {
    if (document.body.classList.contains('dashboardDocument')) return null;
    const avatar = document.querySelector(AVATAR_SELECTOR);
    if (!avatar || typeof avatar.closest !== 'function') return null;
    const toolbar = avatar.closest(MUI_TOOLBAR_SELECTOR);
    if (!toolbar) return null;
    const avatarBox = avatar.closest(`${MUI_TOOLBAR_SELECTOR} > *`);
    if (!avatarBox) return null;
    const box = avatarBox.previousElementSibling;
    return (box && toolbar.contains(box)) ? box : null;
  };

  // MUI 6 keeps its real styling in emotion-generated hash classes
  // (e.g. `css-z77o6z-MuiButtonBase-root-MuiIconButton-root`); the stable
  // `Mui*` class names are only selectors for overrides and carry no styles
  // themselves. So rather than hardcoding a hash that changes with every MUI
  // release, or hand-rolling a lookalike, copy the class list verbatim off a
  // real neighbouring IconButton. Measured against the live 12.0 build this
  // yields a *zero* computed-style difference from a native toolbar button.
  const findDonorButton = (box) => {
    if (typeof box.querySelectorAll !== 'function') return null;
    const candidates = box.querySelectorAll('button, a');
    for (let i = 0; i < candidates.length; i++) {
      const el = candidates[i];
      if (el.id === GLOBAL_BTN_ID) continue;
      if (el.classList && el.classList.contains('MuiIconButton-root')) return el;
    }
    return null;
  };

  const buildMuiButton = (donor) => {
    const btn = document.createElement('button');
    btn.id = GLOBAL_BTN_ID;
    btn.type = 'button';
    btn.title = 'JellyWatchParty';
    btn.setAttribute('aria-label', 'JellyWatchParty');
    // Fall back to the bare Mui* names plus our own reset when no donor is
    // available (e.g. SyncPlay/RemotePlay/Search all hidden on this page).
    btn.className = donor && donor.className
      ? `${donor.className} jwp-global-btn`
      : 'MuiButtonBase-root MuiIconButton-root MuiIconButton-colorInherit MuiIconButton-sizeLarge jwp-global-btn jwp-global-btn-standalone';
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;
    return btn;
  };

  const tryInjectMuiToolbar = () => {
    const box = findMuiActionsBox();
    if (!box) return false;
    box.appendChild(buildMuiButton(findDonorButton(box)));
    return true;
  };

  const injectGlobalButton = () => {
    const existing = document.getElementById(GLOBAL_BTN_ID);
    if (existing) {
      // The legacy .headerRight button needs no upkeep once inserted — its
      // visibility is entirely CSS-driven by Jellyfin's own header wrapper,
      // including while the legacy video OSD hides the whole header.
      //
      // The one exception is a live layout switch: Settings → Display → Layout
      // calls layoutManager.setLayout() without reloading the page
      // (apps/modern/features/preferences/hooks/useDisplaySettings.ts), so a
      // legacy button can end up stranded in a now-hidden header while the MUI
      // toolbar takes over. Only treat it as stranded when a MUI actions box
      // has actually appeared, so the legacy OSD case isn't churned.
      if (existing.classList && existing.classList.contains('jwp-global-btn-legacy')) {
        if (isRendered(existing) || !findMuiActionsBox()) return;
        existing.remove();
        tryInjectMuiToolbar();
        return;
      }

      const box = findMuiActionsBox();
      if (!box) {
        // Navigated somewhere the button must not appear: the video OSD or a
        // public path (no avatar at all), or the admin dashboard.
        existing.remove();
        return;
      }
      // React re-adds its own children relative to its own fiber siblings, so
      // after it tears down and rebuilds the action buttons ours can end up
      // ahead of them. Keep it pinned to the trailing slot next to the avatar.
      if (existing.parentElement !== box || box.lastElementChild !== existing) {
        box.appendChild(existing);
      }
      return;
    }
    // Try the v10.11-style DOM first; fall back to the v12 MUI toolbar.
    if (!tryInjectLegacyHeader()) {
      tryInjectMuiToolbar();
    }
  };

  // Jellyfin 12's toolbar is only rebuilt on real route transitions, so
  // observing the DOM reacts immediately and does far less work than polling.
  // Callbacks are coalesced through requestAnimationFrame because a single
  // React commit produces many mutation records, and because injectGlobalButton
  // mutates the DOM itself and would otherwise re-enter its own observer.
  let observer = null;
  let scheduled = false;

  const observeToolbar = () => {
    if (observer || typeof window.MutationObserver !== 'function') return;
    const raf = typeof window.requestAnimationFrame === 'function'
      ? window.requestAnimationFrame.bind(window)
      : (cb) => setTimeout(cb, 16);
    observer = new window.MutationObserver(() => {
      if (scheduled) return;
      scheduled = true;
      raf(() => { scheduled = false; injectGlobalButton(); });
    });
    observer.observe(document.body, { childList: true, subtree: true });
  };

  const disconnectToolbarObserver = () => {
    if (observer) { observer.disconnect(); observer = null; }
    scheduled = false;
  };

  Object.assign(ui, {
    render,
    injectOsdButton,
    injectGlobalButton,
    applyNativeSyncButtonVisibility,
    observeToolbar,
    disconnectToolbarObserver
  });
})();
