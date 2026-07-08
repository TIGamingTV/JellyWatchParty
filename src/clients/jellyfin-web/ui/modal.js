(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const ui = JWP.ui = JWP.ui || {};
  const utils = JWP.utils;

  // Promise-based text-entry modal — an in-DOM replacement for
  // window.prompt(), which several embedded/CEF-based Jellyfin clients
  // (e.g. Jellyfin Desktop) silently no-op rather than showing a dialog.
  // Resolves with the (trimmed) entered value on submit, or null if the
  // user cancels/dismisses.
  const promptText = ({ title, placeholder = '', submitLabel = 'OK', inputType = 'password' } = {}) => {
    return new Promise((resolve) => {
      const overlay = document.createElement('div');
      overlay.className = 'jwp-modal-overlay';
      overlay.innerHTML = `
        <div class="jwp-modal">
          <div class="jwp-modal-title">${utils.escapeHtml(title || '')}</div>
          <input type="${inputType}" class="jwp-input jwp-modal-input" placeholder="${utils.escapeHtml(placeholder)}">
          <div class="jwp-modal-actions">
            <button class="jwp-btn secondary jwp-modal-cancel">Cancel</button>
            <button class="jwp-btn jwp-modal-submit">${utils.escapeHtml(submitLabel)}</button>
          </div>
        </div>
      `;

      const cleanup = (value) => {
        overlay.remove();
        document.removeEventListener('keydown', onKeydown);
        resolve(value);
      };

      const input = overlay.querySelector('.jwp-modal-input');
      const submit = () => cleanup(input.value.trim());
      const cancel = () => cleanup(null);

      const onKeydown = (e) => {
        if (e.key === 'Escape') cancel();
        else if (e.key === 'Enter') submit();
      };

      overlay.querySelector('.jwp-modal-submit').onclick = submit;
      overlay.querySelector('.jwp-modal-cancel').onclick = cancel;
      overlay.onclick = (e) => { if (e.target === overlay) cancel(); };
      // stopPlayerCapture prevents keydown from bubbling to the document
      // (so Jellyfin's player doesn't steal these keystrokes), which means
      // the input needs its own Enter/Escape handling rather than relying
      // on the document-level listener below to see events from it.
      if (ui.stopPlayerCapture) ui.stopPlayerCapture(input);
      input.addEventListener('keydown', onKeydown);
      document.addEventListener('keydown', onKeydown);

      document.body.appendChild(overlay);
      input.focus();
    });
  };

  Object.assign(ui, { promptText });
})();
