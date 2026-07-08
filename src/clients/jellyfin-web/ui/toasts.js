(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const utils = JWP.utils;

  const showToast = (message) => {
    const toast = document.createElement('div');
    toast.className = 'jwp-toast-system';
    toast.textContent = message;
    toast.onclick = () => {
      toast.classList.add('jwp-toast-out');
      setTimeout(() => toast.remove(), 300);
    };
    document.body.appendChild(toast);
    setTimeout(() => {
      if (toast.parentNode) {
        toast.classList.add('jwp-toast-out');
        setTimeout(() => toast.remove(), 300);
      }
    }, 1500);
  };

  const getToastContainer = () => {
    let container = document.getElementById('jwp-toast-container');
    if (!container) {
      container = document.createElement('div');
      container.id = 'jwp-toast-container';
      container.className = 'jwp-toast-container';
      document.body.appendChild(container);
    }
    return container;
  };

  const showChatToast = (username, text) => {
    const container = getToastContainer();
    const toast = document.createElement('div');
    toast.className = 'jwp-toast';
    toast.innerHTML = `<span class="jwp-toast-username">${utils.escapeHtml(username)}</span><span class="jwp-toast-text">${utils.escapeHtml(text)}</span>`;
    toast.onclick = () => {
      toast.classList.add('jwp-toast-out');
      setTimeout(() => toast.remove(), 300);
    };
    container.appendChild(toast);
    setTimeout(() => {
      if (toast.parentNode) {
        toast.classList.add('jwp-toast-out');
        setTimeout(() => toast.remove(), 300);
      }
    }, 5000);
    const toasts = container.querySelectorAll('.jwp-toast:not(.jwp-toast-out)');
    if (toasts.length > 5) {
      const oldest = toasts[0];
      oldest.classList.add('jwp-toast-out');
      setTimeout(() => oldest.remove(), 300);
    }
  };

  Object.assign(ui, { showToast, showChatToast });
})();
