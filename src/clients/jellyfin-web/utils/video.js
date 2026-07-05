(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const utils = OWP.utils = OWP.utils || {};

  // Jellyfin Media Player / jellyfin-desktop (CEF+mpv) never creates a DOM
  // <video> element - its player plugin (mpv-video-player.js) routes
  // playback through a native mpv backend instead, exposing itself as
  // window._mpvVideoPlayerInstance. This adapter wraps that instance behind
  // an HTMLMediaElement-shaped interface (currentTime/paused/playbackRate/
  // readyState props, play()/pause(), add/removeEventListener) via polling,
  // so playback/bind.js and playback/sync.js work unmodified.
  //
  // Known limitation: mpv-player-base.js only confirms 'playing'/'pause'/
  // 'unpause'/'stopped'/'volumechange'/'error' events, with no equivalent to
  // HTML5's 'waiting' (buffering) event - so buffering can't be detected
  // here, and readyState/networkState are coarse approximations, not real
  // buffer-health signals.
  const NATIVE_POLL_MS = 250;
  const NATIVE_SEEK_JUMP_SEC = 1.0;
  const NATIVE_SEEK_SETTLE_MS = 300;

  let nativeAdapter = null;
  let nativeAdapterInstance = null;

  const createNativeAdapter = (instance) => {
    const listeners = { waiting: [], canplay: [], loadeddata: [], playing: [], play: [], pause: [], seeked: [] };
    const data = {
      currentTime: 0,
      paused: true,
      playbackRate: 1,
      readyState: 0,
      seeking: false,
      lastPollTs: Date.now(),
      seekSettleTimer: null,
      loadedFired: false
    };

    const fire = (type) => {
      for (const fn of listeners[type] || []) {
        try { fn(); } catch (err) { console.error('[OpenWatchParty] native adapter listener error:', err); }
      }
    };

    const poll = () => {
      let rawTimeMs, isPaused, rate;
      try {
        rawTimeMs = instance.currentTime();
        isPaused = instance.paused();
        rate = typeof instance.getPlaybackRate === 'function' ? instance.getPlaybackRate() : 1;
      } catch (err) {
        return;
      }
      if (typeof rawTimeMs !== 'number') return;

      const now = Date.now();
      const elapsedSec = Math.max(0, now - data.lastPollTs) / 1000;
      data.lastPollTs = now;
      const newTime = rawTimeMs / 1000;

      if (!data.loadedFired) {
        data.loadedFired = true;
        data.readyState = 4;
        fire('loadeddata');
        fire('canplay');
      }

      if (!data.seeking) {
        const expected = data.paused ? data.currentTime : data.currentTime + elapsedSec * (data.playbackRate || 1);
        if (Math.abs(newTime - expected) > NATIVE_SEEK_JUMP_SEC) {
          data.currentTime = newTime;
          fire('seeked');
        } else {
          data.currentTime = newTime;
        }
      } else {
        data.currentTime = newTime;
      }

      if (isPaused !== data.paused) {
        data.paused = isPaused;
        fire(isPaused ? 'pause' : 'play');
        if (!isPaused) fire('playing');
      }
      data.playbackRate = rate;
    };

    const intervalId = setInterval(poll, NATIVE_POLL_MS);
    poll();

    return {
      __owpNativeAdapter: true,
      get currentTime() { return data.currentTime; },
      set currentTime(val) {
        data.currentTime = val;
        data.seeking = true;
        try { instance.currentTime(val * 1000); } catch (err) {}
        if (data.seekSettleTimer) clearTimeout(data.seekSettleTimer);
        data.seekSettleTimer = setTimeout(() => { data.seeking = false; }, NATIVE_SEEK_SETTLE_MS);
      },
      get paused() { return data.paused; },
      get playbackRate() { return data.playbackRate; },
      set playbackRate(val) {
        data.playbackRate = val;
        try { instance.setPlaybackRate(val); } catch (err) {}
      },
      get readyState() { return data.readyState; },
      get networkState() { return 0; },
      get seeking() { return data.seeking; },
      play() {
        try {
          if (typeof instance.unpause === 'function') instance.unpause();
          else if (typeof instance.resume === 'function') instance.resume();
        } catch (err) {}
        return Promise.resolve();
      },
      pause() {
        try { instance.pause(); } catch (err) {}
      },
      addEventListener(type, fn) {
        if (!listeners[type]) listeners[type] = [];
        listeners[type].push(fn);
      },
      removeEventListener(type, fn) {
        if (!listeners[type]) return;
        listeners[type] = listeners[type].filter((f) => f !== fn);
      },
      __destroy() {
        clearInterval(intervalId);
        if (data.seekSettleTimer) clearTimeout(data.seekSettleTimer);
      }
    };
  };

  const teardownNativeAdapter = () => {
    if (nativeAdapter) nativeAdapter.__destroy();
    nativeAdapter = null;
    nativeAdapterInstance = null;
  };

  const getVideo = () => {
    const real = document.querySelector('video');
    if (real) {
      teardownNativeAdapter();
      return real;
    }

    const instance = window._mpvVideoPlayerInstance;
    const hasApi = instance && typeof instance.currentTime === 'function' && typeof instance.paused === 'function';
    if (!hasApi) {
      teardownNativeAdapter();
      return null;
    }
    if (nativeAdapter && nativeAdapterInstance !== instance) {
      teardownNativeAdapter();
    }
    if (!nativeAdapter) {
      nativeAdapter = createNativeAdapter(instance);
      nativeAdapterInstance = instance;
    }
    return nativeAdapter;
  };

  const isVideoReady = () => {
    const video = getVideo();
    return video && video.readyState >= 3;
  };

  const isBuffering = () => {
    const video = getVideo();
    if (!video) return false;
    return video.readyState < 3 || (video.networkState === 2 && video.readyState < 4);
  };

  const isSeeking = () => {
    const video = getVideo();
    return video && video.seeking;
  };

  const getPlaybackManager = () => window.playbackManager || window.PlaybackManager || window.app?.playbackManager;

  Object.assign(utils, { getVideo, isVideoReady, isBuffering, isSeeking, getPlaybackManager });
})();