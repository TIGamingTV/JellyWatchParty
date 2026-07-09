const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// playback/tracks.js needs utils.getPlaybackManager/getVideo (utils/video.js)
// and utils.startSyncing (already loaded by setup.js via utils/time.js).
require('../utils/video.js');
require('../playback/tracks.js');

const { TRACK_SWITCH_SUPPRESS_MS } = JWP.constants;

const makePlaybackManager = () => ({
  setAudioStreamIndex(index) { this.lastAudioIndex = index; this.audioCalls = (this.audioCalls || 0) + 1; },
  setSubtitleStreamIndex(index) { this.lastSubtitleIndex = index; this.subtitleCalls = (this.subtitleCalls || 0) + 1; }
});

describe('playback/tracks patchTrackSwitching', () => {
  beforeEach(() => {
    document.querySelector = () => null; // no <video> element in these tests
    window.playbackManager = makePlaybackManager();
    JWP.state.isHost = false;
    JWP.state.inRoom = false;
    JWP.state.isSyncing = false;
  });

  it('wraps setAudioStreamIndex/setSubtitleStreamIndex and still calls the original', () => {
    JWP.playback.patchTrackSwitching();
    window.playbackManager.setAudioStreamIndex(2);
    window.playbackManager.setSubtitleStreamIndex(3);
    assert.equal(window.playbackManager.lastAudioIndex, 2);
    assert.equal(window.playbackManager.lastSubtitleIndex, 3);
    assert.equal(window.playbackManager.audioCalls, 1);
    assert.equal(window.playbackManager.subtitleCalls, 1);
  });

  it('suppresses sync broadcasting when the host switches tracks while in a room', () => {
    JWP.playback.patchTrackSwitching();
    JWP.state.isHost = true;
    JWP.state.inRoom = true;
    window.playbackManager.setAudioStreamIndex(1);
    assert.equal(JWP.state.isSyncing, true);
  });

  it('does not touch isSyncing when not host', () => {
    JWP.playback.patchTrackSwitching();
    JWP.state.isHost = false;
    JWP.state.inRoom = true;
    window.playbackManager.setSubtitleStreamIndex(0);
    assert.equal(JWP.state.isSyncing, false);
  });

  it('does not touch isSyncing when host but not in a room', () => {
    JWP.playback.patchTrackSwitching();
    JWP.state.isHost = true;
    JWP.state.inRoom = false;
    window.playbackManager.setAudioStreamIndex(1);
    assert.equal(JWP.state.isSyncing, false);
  });

  it('does not double-wrap when patched twice', () => {
    JWP.playback.patchTrackSwitching();
    JWP.playback.patchTrackSwitching();
    window.playbackManager.setAudioStreamIndex(5);
    assert.equal(window.playbackManager.audioCalls, 1);
  });
});

describe('utils.startSyncing custom duration', () => {
  it('respects a custom ms argument', () => {
    JWP.state.isSyncing = false;
    JWP.utils.startSyncing(TRACK_SWITCH_SUPPRESS_MS);
    assert.equal(JWP.state.isSyncing, true);
  });
});
