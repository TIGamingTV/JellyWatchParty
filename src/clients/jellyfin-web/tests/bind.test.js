const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// playback/bind.js needs utils.getVideo/isVideoReady/isSeeking (utils/video.js),
// utils.log (utils/log.js) and JWP.ui.showToast (ui/toasts.js).
require('../utils/video.js');
require('../utils/log.js');
require('../ui/toasts.js');
require('../playback/bind.js');

const makeVideo = () => ({
  currentTime: 0,
  paused: true,
  playbackRate: 1,
  readyState: 4,
  networkState: 1,
  seeking: false,
  listeners: {},
  addEventListener(evt, fn) { this.listeners[evt] = fn; },
  removeEventListener(evt, fn) { if (this.listeners[evt] === fn) delete this.listeners[evt]; }
});

describe('playback/bind guest pause toast (issue #71: "why did my pause not stick?")', () => {
  let video;
  let toasts;

  beforeEach(() => {
    video = makeVideo();
    document.querySelector = (sel) => (sel === 'video' ? video : null);
    toasts = [];
    JWP.ui.showToast = (msg) => toasts.push(msg);

    JWP.state.bound = false;
    JWP.state.currentVideoElement = null;
    JWP.state.videoListeners = null;
    JWP.state.inRoom = true;
    JWP.state.isHost = false;
    JWP.state.lastSyncPlayState = 'playing';
    JWP.state.lastGuestPauseToastAt = 0;
    JWP.state.intervals = { stateUpdate: null };

    JWP.playback.bindVideo();
  });

  afterEach(() => {
    // bindVideo starts a stateUpdate interval that would otherwise keep the
    // test process alive.
    JWP.playback.cleanupVideoListeners();
  });

  it('tells a guest playback is host-controlled when they pause while the room is playing', () => {
    video.listeners.pause();
    assert.deepEqual(toasts, ['Only the host controls playback']);
  });

  it('does not toast the host (their own pause is the authoritative one)', () => {
    JWP.state.isHost = true;
    video.listeners.pause();
    assert.deepEqual(toasts, []);
  });

  it('does not toast when the room itself is paused (video pausing is expected, not a guest override)', () => {
    JWP.state.lastSyncPlayState = 'paused';
    video.listeners.pause();
    assert.deepEqual(toasts, []);
  });

  it('does not toast outside of a room', () => {
    JWP.state.inRoom = false;
    video.listeners.pause();
    assert.deepEqual(toasts, []);
  });

  it('is rate-limited so repeated pauses do not spam toasts', () => {
    video.listeners.pause();
    video.listeners.pause();
    video.listeners.pause();
    assert.equal(toasts.length, 1);
  });
});
