(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const h = JWP._wsHandlers = JWP._wsHandlers || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const actions = JWP.actions;
  const { TIME_SYNC_MAX_SAMPLES, TIME_SYNC_EMA_ALPHA, PING_STABLE_AFTER } = JWP.constants;

  const updateClockSync = (now, rtt, serverTs) => {
    const sampleOffset = serverTs + (rtt / 2) - now;
    state.timeSyncSamples.push({ rtt, offset: sampleOffset, ts: now });
    if (state.timeSyncSamples.length > TIME_SYNC_MAX_SAMPLES) {
      state.timeSyncSamples.shift();
    }
    const bestSample = state.timeSyncSamples.reduce((best, s) =>
      s.rtt < best.rtt ? s : best
    );
    const prevOffset = state.serverOffsetMs;
    state.serverOffsetMs = state.hasTimeSync
      ? state.serverOffsetMs * (1 - TIME_SYNC_EMA_ALPHA) + bestSample.offset * TIME_SYNC_EMA_ALPHA
      : bestSample.offset;
    state.hasTimeSync = true;
    return { prevOffset, bestSample };
  };

  const checkAdaptivePing = (prevOffset) => {
    const wasInit = state.successfulPings < PING_STABLE_AFTER;
    state.successfulPings++;
    if (wasInit && state.successfulPings >= PING_STABLE_AFTER) {
      actions.schedulePing();
    }
    const delta = Math.abs(state.serverOffsetMs - prevOffset);
    if (state.successfulPings >= PING_STABLE_AFTER && delta > 50) {
      state.successfulPings = 0;
      actions.schedulePing();
    }
  };

  h.handlePong = (msg) => {
    if (!msg.payload || !msg.payload.client_ts) return;
    const now = utils.nowMs();
    const rtt = now - msg.payload.client_ts;
    const latEl = document.querySelector('.jwp-latency');
    if (latEl) latEl.textContent = `${rtt} ms`;
    if (typeof msg.server_ts === 'number' && rtt > 0) {
      const { prevOffset, bestSample } = updateClockSync(now, rtt, msg.server_ts);
      checkAdaptivePing(prevOffset);
      if (Math.random() < 0.1) {
        utils.log('CLOCK', { rtt, best_rtt: bestSample.rtt, server_offset: state.serverOffsetMs, delta: state.serverOffsetMs - prevOffset, samples: state.timeSyncSamples.length });
      }
    }
  };
})();
