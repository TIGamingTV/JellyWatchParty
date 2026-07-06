(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const state = JWP.state;

  const cleanupPanel = () => {
    const lc = JWP._lifecycle;
    if (lc && lc.panelStopPropagation) {
      const panel = document.getElementById(JWP.constants.PANEL_ID);
      if (panel) {
        panel.removeEventListener('click', lc.panelStopPropagation);
        panel.removeEventListener('mousedown', lc.panelStopPropagation);
        panel.removeEventListener('keydown', lc.panelStopPropagation);
        panel.removeEventListener('keyup', lc.panelStopPropagation);
        panel.removeEventListener('keypress', lc.panelStopPropagation);
      }
      lc.panelStopPropagation = null;
    }
  };

  const cleanupVideo = () => {
    if (state.currentVideoElement && state.videoListeners) {
      const video = state.currentVideoElement;
      const listeners = state.videoListeners;
      if (listeners.waiting) video.removeEventListener('waiting', listeners.waiting);
      if (listeners.canplay) video.removeEventListener('canplay', listeners.canplay);
      if (listeners.playing) video.removeEventListener('playing', listeners.playing);
      if (listeners.play) video.removeEventListener('play', listeners.play);
      if (listeners.pause) video.removeEventListener('pause', listeners.pause);
      if (listeners.seeked) video.removeEventListener('seeked', listeners.seeked);
      state.videoListeners = null;
      state.currentVideoElement = null;
    }
  };

  const cleanup = () => {
    const lc = JWP._lifecycle;
    if (lc) lc.clearAllIntervals();
    if (state.pendingActionTimer) {
      clearTimeout(state.pendingActionTimer);
      state.pendingActionTimer = null;
    }
    if (lc) lc.hadVideoElement = false;
    if (state.ws) {
      state.ws.close();
      state.ws = null;
    }
    cleanupPanel();
    cleanupVideo();
    state.bound = false;
    state.initialized = false;
  };

  JWP.app = JWP.app || {};
  Object.assign(JWP.app, { cleanup });
})();
