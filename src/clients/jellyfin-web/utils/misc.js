(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const utils = JWP.utils = JWP.utils || {};
  const state = JWP.state;
  const { SUPPRESS_MS } = JWP.constants;

  const shouldSend = () => utils.nowMs() > state.suppressUntil;

  const suppress = (ms = SUPPRESS_MS) => { state.suppressUntil = utils.nowMs() + ms; };

  const HTML_ENTITIES = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;'
  };

  const escapeHtml = (str) => {
    if (typeof str !== 'string') return '';
    return str.replace(/[&<>"']/g, c => HTML_ENTITIES[c]);
  };

  const getItemImageUrl = (itemId, imageTag) => {
    if (!itemId || !window.ApiClient) return '';
    const serverUrl = window.ApiClient._serverAddress || window.ApiClient.serverAddress?.() || '';
    if (!serverUrl) return '';
    let url = `${serverUrl}/Items/${itemId}/Images/Primary?quality=90`;
    if (imageTag) url += `&tag=${imageTag}`;
    return url;
  };

  const isHomeView = () => {
    if (document.querySelector('.homePage')) return true;
    const hash = window.location.hash || '';
    return hash.includes('home');
  };

  Object.assign(utils, { shouldSend, suppress, escapeHtml, getItemImageUrl, isHomeView });
})();
