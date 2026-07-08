(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const chat = JWP.chat = JWP.chat || { messages: [], unreadCount: 0 };

  const MAX_MESSAGE_LENGTH = 500;

  const send = (text) => {
    console.log('[JellyWatchParty] Chat.send called with:', text);
    if (!text || !text.trim()) return false;
    const trimmed = text.trim();
    if (trimmed.length > MAX_MESSAGE_LENGTH) {
      JWP.ui.showToast(`Message too long (max ${MAX_MESSAGE_LENGTH} characters)`);
      return false;
    }
    if (!JWP.state.ws || JWP.state.ws.readyState !== 1) {
      console.log('[JellyWatchParty] Chat: Not connected');
      JWP.ui.showToast('Not connected to server');
      return false;
    }
    if (!JWP.state.roomId) {
      console.log('[JellyWatchParty] Chat: Not in a room');
      JWP.ui.showToast('Not in a room');
      return false;
    }
    console.log('[JellyWatchParty] Chat: Sending message to room', JWP.state.roomId);
    JWP.actions.send('chat_message', { text: trimmed });
    return true;
  };

  const isChatVisible = () => {
    const chatSection = document.getElementById('jwp-chat-section');
    const panel = document.getElementById(JWP.constants.PANEL_ID);
    return chatSection && panel && !panel.classList.contains('hide');
  };

  const markRead = () => {
    chat.unreadCount = 0;
    updateBadge();
  };

  const updateBadge = () => {
    const badge = document.getElementById('jwp-chat-badge');
    if (badge) {
      if (chat.unreadCount > 0) {
        badge.textContent = chat.unreadCount > 99 ? '99+' : chat.unreadCount;
        badge.style.display = 'inline-block';
      } else {
        badge.style.display = 'none';
      }
    }
  };

  Object.assign(chat, { send, isChatVisible, markRead, updateBadge });
})();
