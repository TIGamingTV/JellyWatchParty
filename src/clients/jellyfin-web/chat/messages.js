(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const chat = OWP.chat = OWP.chat || { messages: [], unreadCount: 0 };
  const utils = OWP.utils;

  const MAX_MESSAGES = 100;

  const formatTime = (ts) => {
    const date = new Date(ts);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };

  const renderMessage = (message) => {
    const container = document.getElementById('owp-chat-messages');
    if (!container) return;
    const msgEl = document.createElement('div');
    msgEl.className = 'owp-chat-message' + (message.isOwn ? ' owp-chat-own' : '');
    msgEl.innerHTML = `
      <div class="owp-chat-meta">
        <span class="owp-chat-username">${utils.escapeHtml(message.username)}</span>
        <span class="owp-chat-time">${formatTime(message.timestamp)}</span>
      </div>
      <div class="owp-chat-text">${utils.escapeHtml(message.text)}</div>
    `;
    container.appendChild(msgEl);
    container.scrollTop = container.scrollHeight;
  };

  const renderAllMessages = () => {
    const container = document.getElementById('owp-chat-messages');
    if (!container) return;
    container.innerHTML = '';
    chat.messages.forEach(msg => renderMessage(msg));
  };

  const receive = (msg) => {
    console.log('[OpenWatchParty] Chat.receive called with:', msg);
    const message = {
      clientId: msg.client,
      username: msg.payload?.username || 'Anonymous',
      text: msg.payload?.text || '',
      timestamp: msg.server_ts || Date.now(),
      isOwn: msg.client === OWP.state.clientId
    };
    chat.messages.push(message);
    if (chat.messages.length > MAX_MESSAGES) {
      chat.messages.shift();
    }
    if (!chat.isChatVisible()) {
      chat.unreadCount++;
      chat.updateBadge();
      if (!message.isOwn && OWP.ui && OWP.ui.showChatToast) {
        OWP.ui.showChatToast(message.username, message.text);
      }
    }
    renderMessage(message);
  };

  const clear = () => {
    chat.messages = [];
    chat.unreadCount = 0;
    chat.updateBadge();
    const container = document.getElementById('owp-chat-messages');
    if (container) container.innerHTML = '';
  };

  Object.assign(chat, { renderMessage, renderAllMessages, receive, clear });
})();
