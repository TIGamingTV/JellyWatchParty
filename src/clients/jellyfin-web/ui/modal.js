(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const ui = OWP.ui = OWP.ui || {};
  const utils = OWP.utils;

  // Promise-based text-entry modal — an in-DOM replacement for
  // window.prompt(), which several embedded/CEF-based Jellyfin clients
  // (e.g. Jellyfin Desktop) silently no-op rather than showing a dialog.
  // Resolves with the (trimmed) entered value on submit, or null if the
  // user cancels/dismisses.
  const promptText = ({ title, placeholder = '', submitLabel = 'OK', inputType = 'password' } = {}) => {
    return new Promise((resolve) => {
      const overlay = document.createElement('div');
      overlay.className = 'owp-modal-overlay';
      overlay.innerHTML = `
        <div class="owp-modal">
          <div class="owp-modal-title">${utils.escapeHtml(title || '')}</div>
          <input type="${inputType}" class="owp-input owp-modal-input" placeholder="${utils.escapeHtml(placeholder)}">
          <div class="owp-modal-actions">
            <button class="owp-btn secondary owp-modal-cancel">Cancel</button>
            <button class="owp-btn owp-modal-submit">${utils.escapeHtml(submitLabel)}</button>
          </div>
        </div>
      `;

      const cleanup = (value) => {
        overlay.remove();
        document.removeEventListener('keydown', onKeydown);
        resolve(value);
      };

      const input = overlay.querySelector('.owp-modal-input');
      const submit = () => cleanup(input.value.trim());
      const cancel = () => cleanup(null);

      const onKeydown = (e) => {
        if (e.key === 'Escape') cancel();
        else if (e.key === 'Enter') submit();
      };

      overlay.querySelector('.owp-modal-submit').onclick = submit;
      overlay.querySelector('.owp-modal-cancel').onclick = cancel;
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
