(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const playback = JWP.playback = JWP.playback || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { STATE_UPDATE_MS, SEEK_THRESHOLD } = JWP.constants;

  // True while we're still confirming whether a newly loaded stream is
  // actually a different item (see scheduleNowPlayingRefresh /
  // maybeSendSetMedia below). Self-clears past its safety timeout so a lost
  // timer/rejected promise can't wedge the host silent forever.
  const isMediaSwitchPending = () => {
    if (!state.mediaSwitchPending) return false;
    if (state.mediaSwitchPendingUntil && utils.nowMs() > state.mediaSwitchPendingUntil) {
      state.mediaSwitchPending = false;
      return false;
    }
    return true;
  };

  const sendStateUpdate = (video) => {
    const actions = JWP.actions;
    if (!state.isHost || !actions || !actions.send) return;
    if (state.isSyncing) return;
    if (isMediaSwitchPending()) return;
    // Held paused for the start countdown: a "paused" update would pause the
    // guests who are about to start.
    if (state.startPending) return;
    if (utils.isSeeking()) return;
    if (state.isBuffering || !utils.isVideoReady()) return;
    const now = utils.nowMs();
    if (now - state.lastStateSentAt < STATE_UPDATE_MS) return;
    state.lastStateSentAt = now;
    // The server counts the room as started once the host reports playing;
    // mirror that so a later pause/play doesn't trigger a countdown.
    if (!video.paused) state.roomStarted = true;
    actions.send('state_update', { position: video.currentTime, play_state: video.paused ? 'paused' : 'playing' });
  };

  // The room's first play: keep the host's video where it is, ask the server
  // to start, and let the countdown (ws/handlers/playback.js) start everyone,
  // host included, at the same moment.
  const sendStart = (video) => {
    state.startQueued = false;
    utils.log('HOST', { action: 'start_requested', pos: video.currentTime });
    JWP.actions.send('player_event', { action: 'play', position: video.currentTime, play_state: 'playing' });
  };

  // A start held back while the host's new item was being confirmed goes out
  // right after set_media, so the server already knows the item everyone
  // has to load before it counts down.
  const flushQueuedStart = () => {
    if (!state.startQueued || !state.startPending) return;
    const video = state.currentVideoElement || utils.getVideo();
    if (!video || !JWP.actions || !JWP.actions.send) return;
    sendStart(video);
  };

  const requestStart = (video) => {
    utils.suppress();
    video.pause();
    state.startPending = true;
    if (JWP.ui && JWP.ui.showStartWaiting) JWP.ui.showStartWaiting();
    if (isMediaSwitchPending()) {
      state.startQueued = true;
    } else {
      sendStart(video);
    }
    if (state.startSafetyTimer) clearTimeout(state.startSafetyTimer);
    state.startSafetyTimer = setTimeout(() => {
      state.startSafetyTimer = null;
      if (!state.startPending) return;
      // No countdown came back (e.g. the connection dropped): just play. Not
      // suppressed, so the play is relayed to the guests the usual way.
      state.startPending = false;
      state.startQueued = false;
      state.roomStarted = true;
      if (JWP.ui && JWP.ui.hideCountdown) JWP.ui.hideCountdown();
      video.play().catch(() => {});
    }, JWP.constants.START_SAFETY_MS);
  };

  const onHostEvent = (action, video) => {
    const actions = JWP.actions;
    if (!state.isHost || !actions || !actions.send || !utils.shouldSend()) return;
    if (state.isSyncing) return;
    if (action === 'play' && state.inRoom && (state.startPending || !state.roomStarted)) {
      if (state.startPending) {
        // Pressed play again while waiting: stay paused, the start is on its way.
        utils.suppress();
        video.pause();
        return;
      }
      requestStart(video);
      return;
    }
    if (isMediaSwitchPending()) return;
    if (action === 'seek' && !utils.isVideoReady()) return;
    if (action === 'pause') {
      if (state.isBuffering) return;
      if (utils.isSeeking()) return;
      state.wantsToPlay = false;
    }
    if (action === 'play') {
      if (utils.isSeeking()) return;
      state.wantsToPlay = true;
    }
    if (action === 'seek') {
      const now = utils.nowMs();
      if (now - state.lastSeekSentAt < 250) return;
      if (Math.abs(video.currentTime - state.lastSentPosition) < SEEK_THRESHOLD) return;
      state.lastSeekSentAt = now;
      state.lastSentPosition = video.currentTime;
    }
    utils.log('HOST', { action, pos: video.currentTime, paused: video.paused });
    actions.send('player_event', { action, position: video.currentTime, play_state: video.paused ? 'paused' : 'playing' });
    if (action === 'play' || action === 'pause' || action === 'seek') {
      actions.send('state_update', { position: video.currentTime, play_state: video.paused ? 'paused' : 'playing' });
      state.lastStateSentAt = utils.nowMs();
    }
  };

  // Lets a guest know why their local pause didn't stick: playback is
  // host-controlled, so sync.js resumes them within ~1s (see
  // ws/handlers/sync.js handleStateUpdate). Only fires when the room is
  // actually playing - i.e. the pause was the guest's own doing, not the
  // video catching up to a host who is themselves paused.
  const GUEST_PAUSE_TOAST_COOLDOWN_MS = 10000;
  const onGuestPause = () => {
    if (state.isHost || !state.inRoom) return;
    if (state.lastSyncPlayState !== 'playing') return;
    const now = utils.nowMs();
    if (now - state.lastGuestPauseToastAt < GUEST_PAUSE_TOAST_COOLDOWN_MS) return;
    state.lastGuestPauseToastAt = now;
    if (JWP.ui && JWP.ui.showToast) {
      JWP.ui.showToast('Only the host controls playback');
    }
  };

  const createVideoListeners = (video) => {
    return {
      waiting: () => {
        state.isBuffering = true;
        utils.log('VIDEO', { event: 'buffering', pos: video.currentTime, readyState: video.readyState });
        if (state.isHost && JWP.actions && JWP.actions.send) {
          JWP.actions.send('player_event', { action: 'buffering', position: video.currentTime });
        }
      },
      canplay: () => {
        const wasBuffering = state.isBuffering;
        state.isBuffering = false;
        if (wasBuffering) utils.log('VIDEO', { event: 'ready', pos: video.currentTime, readyState: video.readyState });
      },
      playing: () => {
        const wasBuffering = state.isBuffering;
        state.isBuffering = false;
        if (wasBuffering) {
          utils.log('VIDEO', { event: 'playing', pos: video.currentTime });
          if (state.isHost && JWP.actions && JWP.actions.send) {
            JWP.actions.send('player_event', { action: 'play', position: video.currentTime });
          }
        }
      },
      play: () => onHostEvent('play', video),
      pause: () => {
        onHostEvent('pause', video);
        onGuestPause();
      },
      seeked: () => {
        utils.log('VIDEO', { event: 'seeked', pos: video.currentTime });
        onHostEvent('seek', video);
      },
      // A new stream is loading (next item, or a track switch that reloads
      // the transcode).
      loadstart: () => {
        if (playback.onStreamReload) playback.onStreamReload();
        beginMediaResolution();
      }
    };
  };

  // The server learns the playing item from the player's first progress
  // report, so give it a moment before asking (see utils.getOwnSession).
  const NOW_PLAYING_REFRESH_DELAY_MS = 1500;
  // Safety net: if resolution never completes (lost timer, rejected
  // promise), don't leave the host permanently silent.
  const MEDIA_SWITCH_PENDING_TIMEOUT_MS = 5000;
  let nowPlayingRefreshTimer = null;

  // Resolves this host's current item id, preferring the server-confirmed
  // value over local DOM/global heuristics when there's no global
  // playbackManager (Jellyfin 12.1+): those heuristics can mismatch there
  // (e.g. the OSD's rating button carries the same data-id attribute the
  // selector looks for - see issue #71), while the server lookup, just
  // refreshed by refreshServerNowPlaying, is authoritative.
  const resolveHostItemId = () => {
    if (!utils.getPlaybackManager()) {
      return (state.serverNowPlayingId && utils.normalizeItemId(state.serverNowPlayingId)) || null;
    }
    return utils.normalizeItemId(utils.getCurrentItemId());
  };

  // Compares the host's now-settled item against the room's media_id and,
  // if it changed (a new item after an empty-media room, or the host
  // switched movies while in the room - issue #71), tells the server.
  // Applied optimistically to state.roomMediaId since the host never
  // receives its own media_changed broadcast.
  const maybeSendSetMedia = () => {
    state.mediaSwitchPending = false;
    if (!state.isHost || !state.inRoom) return;
    const currentId = resolveHostItemId();
    if (!currentId) return;
    const roomMediaId = utils.normalizeItemId(state.roomMediaId);
    if (currentId === roomMediaId) return;
    state.roomMediaId = currentId;
    const actions = JWP.actions;
    if (!actions || !actions.send) return;
    const video = utils.getVideo();
    actions.send('set_media', { media_id: currentId, position: video ? video.currentTime : 0 });
  };

  const scheduleNowPlayingRefresh = () => {
    if (!utils.refreshServerNowPlaying) return;
    if (nowPlayingRefreshTimer) clearTimeout(nowPlayingRefreshTimer);
    nowPlayingRefreshTimer = setTimeout(() => {
      nowPlayingRefreshTimer = null;
      Promise.resolve(utils.refreshServerNowPlaying()).finally(() => {
        maybeSendSetMedia();
        flushQueuedStart();
      });
    }, NOW_PLAYING_REFRESH_DELAY_MS);
  };

  // Suppresses host broadcasts (sendStateUpdate/onHostEvent) until the item
  // resolution above finishes, so a mid-load autoplay/progress event is
  // never reported against the room's old media_id.
  const beginMediaResolution = () => {
    if (state.isHost && state.inRoom) {
      state.mediaSwitchPending = true;
      state.mediaSwitchPendingUntil = utils.nowMs() + MEDIA_SWITCH_PENDING_TIMEOUT_MS;
    }
    scheduleNowPlayingRefresh();
  };

  const bindVideo = () => {
    const video = utils.getVideo();
    if (!video) return;
    if (state.bound && state.currentVideoElement !== video) {
      cleanupVideoListeners();
      state.bound = false;
    }
    if (state.bound) return;
    state.bound = true;
    state.currentVideoElement = video;
    const listeners = createVideoListeners(video);
    state.videoListeners = listeners;
    video.addEventListener('waiting', listeners.waiting);
    video.addEventListener('canplay', listeners.canplay);
    video.addEventListener('playing', listeners.playing);
    video.addEventListener('play', listeners.play);
    video.addEventListener('pause', listeners.pause);
    video.addEventListener('seeked', listeners.seeked);
    video.addEventListener('loadstart', listeners.loadstart);
    beginMediaResolution();
    if (state.isHost && state.inRoom && !state.roomStarted && !state.startPending && !video.paused) {
      requestStart(video);
    }
    if (state.intervals.stateUpdate) {
      clearInterval(state.intervals.stateUpdate);
    }
    state.intervals.stateUpdate = setInterval(() => {
      if (state.isHost) sendStateUpdate(video);
    }, STATE_UPDATE_MS);
  };

  const cleanupVideoListeners = () => {
    if (state.currentVideoElement && state.videoListeners) {
      const video = state.currentVideoElement;
      const listeners = state.videoListeners;
      video.removeEventListener('waiting', listeners.waiting);
      video.removeEventListener('canplay', listeners.canplay);
      video.removeEventListener('playing', listeners.playing);
      video.removeEventListener('play', listeners.play);
      video.removeEventListener('pause', listeners.pause);
      video.removeEventListener('seeked', listeners.seeked);
      video.removeEventListener('loadstart', listeners.loadstart);
    }
    if (nowPlayingRefreshTimer) {
      clearTimeout(nowPlayingRefreshTimer);
      nowPlayingRefreshTimer = null;
    }
    if (state.intervals.stateUpdate) {
      clearInterval(state.intervals.stateUpdate);
      state.intervals.stateUpdate = null;
    }
    state.videoListeners = null;
    state.currentVideoElement = null;
  };

  Object.assign(playback, { bindVideo, cleanupVideoListeners });

  // Exposed for tests only - lets set_media detection be exercised directly
  // instead of waiting out NOW_PLAYING_REFRESH_DELAY_MS in real time.
  JWP._bindInternal = { maybeSendSetMedia, resolveHostItemId, isMediaSwitchPending, beginMediaResolution };
})();
