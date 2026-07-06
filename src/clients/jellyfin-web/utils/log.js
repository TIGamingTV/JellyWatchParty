(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const utils = JWP.utils = JWP.utils || {};
  const state = JWP.state;

  const formatLogValue = (k, v) => {
    if (typeof v === 'number') {
      if (k.includes('pos') || k === 'actual' || k === 'expected' || k === 'drift') {
        return `${k}=${v.toFixed(2)}s`;
      }
      if (k === 'rate') {
        return `${k}=${v.toFixed(2)}x`;
      }
      if (k.includes('offset') || k.includes('delay') || k.includes('rtt')) {
        return `${k}=${v >= 0 ? '+' : ''}${Math.round(v)}ms`;
      }
      return `${k}=${v}`;
    }
    return `${k}=${v}`;
  };

  const sendLog = (entry) => {
    if (!state.ws || state.ws.readyState !== 1) return false;
    try {
      state.ws.send(JSON.stringify({
        type: 'client_log',
        payload: { category: entry.category, message: entry.message },
        ts: entry.ts
      }));
      return true;
    } catch (e) {
      console.warn('[JWP] Failed to send log:', e.message);
      return false;
    }
  };

  const flushLogBuffer = () => {
    if (!state.ws || state.ws.readyState !== 1) return;
    while (state.logBuffer.length > 0) {
      const entry = state.logBuffer.shift();
      if (!sendLog(entry)) {
        state.logBuffer.unshift(entry);
        break;
      }
    }
  };

  const log = (category, data) => {
    const parts = Object.entries(data).map(([k, v]) => formatLogValue(k, v));
    const message = parts.join(' ');
    console.log(`[JWP:${category}] ${message}`);

    const logEntry = { category, message, ts: utils.nowMs() };

    if (state.ws && state.ws.readyState === 1) {
      flushLogBuffer();
      sendLog(logEntry);
    } else {
      if (state.logBuffer.length < state.logBufferMax) {
        state.logBuffer.push(logEntry);
      }
    }
  };

  Object.assign(utils, { log, flushLogBuffer });
})();
