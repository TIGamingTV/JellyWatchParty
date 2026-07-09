// Test setup: creates the global JWP namespace expected by client modules.
// Modules are IIFEs that attach to window.JellyWatchParty, so we simulate
// the browser environment minimally with globalThis.

globalThis.window = globalThis;
globalThis.window.location = { protocol: 'https:', hostname: 'localhost', hash: '' };
globalThis.document = { querySelector: () => null };
globalThis.setTimeout = setTimeout;
globalThis.clearTimeout = clearTimeout;

// Load state.js first (defines JWP.constants and JWP.state)
require('../state.js');

// Load time.js (defines JWP.utils.nowMs, getServerNow, adjustedPosition)
require('../utils/time.js');

// Load misc.js (defines JWP.utils.escapeHtml, suppress, shouldSend)
require('../utils/misc.js');

// Load validation.js (defines JWP.utils.validateWsUrl)
require('../utils/validation.js');

module.exports = globalThis.JellyWatchParty;
