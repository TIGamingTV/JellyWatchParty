(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const ui = OWP.ui = OWP.ui || {};
  const utils = OWP.utils;

  const showToast = (message) => {
    const toast = document.createElement('div');
    toast.className = 'owp-toast-system';
    toast.textContent = message;
    toast.onclick = () => {
      toast.classList.add('owp-toast-out');
      setTimeout(() => toast.remove(), 300);
    };
    document.body.appendChild(toast);
    setTimeout(() => {
      if (toast.parentNode) {
        toast.classList.add('owp-toast-out');
        setTimeout(() => toast.remove(), 300);
      }
    }, 1500);
  };

  const getToastContainer = () => {
    let container = document.getElementById('owp-toast-container');
    if (!container) {
      container = document.createElement('div');
      container.id = 'owp-toast-container';
      container.className = 'owp-toast-container';
      document.body.appendChild(container);
    }
    return container;
  };

  const showChatToast = (username, text) => {
    const container = getToastContainer();
    const toast = document.createElement('div');
    toast.className = 'owp-toast';
    toast.innerHTML = `<span class="owp-toast-username">${utils.escapeHtml(username)}</span><span class="owp-toast-text">${utils.escapeHtml(text)}</span>`;
    toast.onclick = () => {
      toast.classList.add('owp-toast-out');
      setTimeout(() => toast.remove(), 300);
    };
    container.appendChild(toast);
    setTimeout(() => {
      if (toast.parentNode) {
        toast.classList.add('owp-toast-out');
        setTimeout(() => toast.remove(), 300);
      }
    }, 5000);
    const toasts = container.querySelectorAll('.owp-toast:not(.owp-toast-out)');
    if (toasts.length > 5) {
      const oldest = toasts[0];
      oldest.classList.add('owp-toast-out');
      setTimeout(() => oldest.remove(), 300);
    }
  };

  Object.assign(ui, { showToast, showChatToast });
})();
