(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const playback = JWP.playback = JWP.playback || {};
  const state = JWP.state;
  const utils = JWP.utils;
  const { TRACK_SWITCH_SUPPRESS_MS } = JWP.constants;

  let settleHandler = null;
  let settleVideo = null;

  const clearSettleShortcut = () => {
    if (settleVideo && settleHandler) {
      settleVideo.removeEventListener('playing', settleHandler);
      settleVideo.removeEventListener('canplay', settleHandler);
    }
    settleVideo = null;
    settleHandler = null;
  };

  // Once a host's track-switch-triggered reload visibly settles, collapse the
  // suppression window back down to the normal short one instead of leaving
  // it open for the full safety-net duration.
  const armSettleShortcut = () => {
    const video = utils.getVideo();
    clearSettleShortcut();
    if (!video) return;
    settleVideo = video;
    settleHandler = () => {
      clearSettleShortcut();
      utils.startSyncing();
    };
    video.addEventListener('playing', settleHandler);
    video.addEventListener('canplay', settleHandler);
  };

  const patchMethod = (pm, methodName) => {
    const original = pm[methodName];
    if (typeof original !== 'function' || original.__jwpWrapped) return;
    const wrapped = function (...args) {
      if (state.isHost && state.inRoom) {
        utils.startSyncing(TRACK_SWITCH_SUPPRESS_MS);
        armSettleShortcut();
      }
      return original.apply(this, args);
    };
    wrapped.__jwpWrapped = true;
    pm[methodName] = wrapped;
  };

  const patchTrackSwitching = () => {
    const pm = utils.getPlaybackManager();
    if (!pm || pm.__jwpTracksPatched) return;
    patchMethod(pm, 'setAudioStreamIndex');
    patchMethod(pm, 'setSubtitleStreamIndex');
    pm.__jwpTracksPatched = true;
  };

  Object.assign(playback, { patchTrackSwitching });
})();
