(() => {
  const OWP = window.OpenWatchParty = window.OpenWatchParty || {};
  const utils = OWP.utils = OWP.utils || {};

  const getVideo = () => document.querySelector('video');

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
