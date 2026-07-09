(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { PANEL_ID, BTN_ID, DEFAULT_WS_URL } = JWP.constants;
  const GLOBAL_BTN_ID = 'jwp-global-btn';

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
          <div class="jwp-section" style="border-top: 1px solid #333; padding-top: 15px;">
            <div class="jwp-label">Host From Another Device (e.g. Fladder)</div>
            <div id="jwp-bridge-active"></div>
            <div id="jwp-bridge-available"></div>
          </div>
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
      <div class="jwp-meta" style="font-size:10px; color:#666; display:flex; justify-content:space-between; flex-shrink:0; padding-top:8px;">
          <span>RTT: <span class="jwp-latency">-</span></span>
          <span>ID: ${state.clientId.split('-')[1] || '...'}</span>
      </div>
    `;
    const leaveBtn = panel.querySelector('#jwp-btn-leave');
    if (leaveBtn) leaveBtn.onclick = () => JWP.actions && JWP.actions.leaveRoom && JWP.actions.leaveRoom();
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

  const injectGlobalButton = () => {
    if (document.getElementById(GLOBAL_BTN_ID)) return;
    const headerRight = document.querySelector('.headerRight') || document.querySelector('.skinHeader .headerRight');
    if (!headerRight) return;

    const btn = document.createElement('button');
    btn.id = GLOBAL_BTN_ID;
    btn.className = 'paper-icon-button-light jwp-global-btn';
    btn.type = 'button';
    btn.title = 'JellyWatchParty';
    btn.setAttribute('aria-label', 'JellyWatchParty');
    btn.innerHTML = '<span class="material-icons groups" aria-hidden="true"></span>';
    btn.onclick = togglePanel;

    headerRight.prepend(btn);
  };

  Object.assign(ui, { render, injectOsdButton, injectGlobalButton });
})();
