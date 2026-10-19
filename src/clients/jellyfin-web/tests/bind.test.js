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
    JWP.state.democraticMode = false;
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

  it('does not toast in democratic mode (the pause is the room\'s now)', () => {
    JWP.state.democraticMode = true;
    video.listeners.pause();
    assert.deepEqual(toasts, []);
  });

  it('does not toast in democratic mode even before the room has started', () => {
    JWP.state.democraticMode = true;
    JWP.state.roomStarted = false;
    video.listeners.pause();
    assert.deepEqual(toasts, []);
    JWP.state.roomStarted = true;
  });
});

describe('playback/bind democratic mode: a guest\'s own play/pause/seek', () => {
  let video;
  let sent;
  let toasts;

  beforeEach(() => {
    video = makeVideo();
    video.currentTime = 50;
    video.paused = false;
    document.querySelector = (sel) => (sel === 'video' ? video : null);
    sent = [];
    JWP.actions = { send: (type, payload) => sent.push({ type, payload }) };
    toasts = [];
    JWP.ui.showToast = (msg) => toasts.push(msg);

    Object.assign(JWP.state, {
      bound: false,
      currentVideoElement: null,
      videoListeners: null,
      inRoom: true,
      isHost: false,
      roomId: 'room-1',
      roomMediaId: '',
      democraticMode: true,
      roomStarted: true,
      startPending: false,
      suppressUntil: 0,
      isSyncing: false,
      isBuffering: false,
      lastSeekSentAt: 0,
      lastGuestPauseToastAt: 0,
      commandGuardUntil: 0,
      intervals: { stateUpdate: null }
    });
    JWP.playback.bindVideo();
    // Bound mid-session with a ready video: past the short "player just
    // appeared" window.
    JWP.state.followerLoadingUntil = 0;
    // In sync with a room playing at 50s.
    JWP.utils.mirrorRoomState(50, 'playing');
  });

  afterEach(() => {
    JWP.playback.cleanupVideoListeners();
    delete JWP.actions;
  });

  it('sends a pause to the room as a player_event only (no state_update)', () => {
    video.paused = true;
    video.listeners.pause();
    assert.deepEqual(sent, [{ type: 'player_event', payload: { action: 'pause', position: 50, play_state: 'paused' } }]);
    assert.deepEqual(toasts, []);
  });

  it('mirrors what it sent as the room state and guards it against stale host updates', () => {
    video.paused = true;
    video.listeners.pause();
    assert.equal(JWP.state.lastSyncPlayState, 'paused');
    assert.equal(JWP.state.lastSyncPosition, 50);
    assert.ok(JWP.state.commandGuardUntil > Date.now());
    assert.equal(JWP.state.commandGuardPlayState, 'paused');
  });

  it('sends a play when the room is paused', () => {
    JWP.utils.mirrorRoomState(50, 'paused');
    video.listeners.play();
    assert.deepEqual(sent, [{ type: 'player_event', payload: { action: 'play', position: 50, play_state: 'playing' } }]);
    assert.equal(JWP.state.lastSyncPlayState, 'playing');
  });

  it('sends a seek, and its own syncLoop baseline moves with it', () => {
    video.currentTime = 120;
    video.listeners.seeked();
    assert.deepEqual(sent, [{ type: 'player_event', payload: { action: 'seek', position: 120, play_state: 'playing' } }]);
    assert.equal(JWP.state.lastSyncPosition, 120);
  });

  it('never sends buffering or the paired play after buffering', () => {
    video.listeners.waiting();
    video.listeners.playing();
    assert.deepEqual(sent, []);
  });

  it('sends nothing when democratic mode is off', () => {
    JWP.state.democraticMode = false;
    video.paused = true;
    video.listeners.pause();
    video.currentTime = 120;
    video.listeners.seeked();
    assert.deepEqual(sent, []);
  });

  it('sends nothing before the room has started: the first (countdown) play is the host\'s', () => {
    JWP.state.roomStarted = false;
    JWP.utils.mirrorRoomState(50, 'paused');
    video.listeners.play();
    assert.deepEqual(sent, []);
    assert.equal(JWP.state.startPending, false, 'no start request from a guest');
  });

  it('sends nothing while a sync is being applied (isSyncing / suppress)', () => {
    JWP.state.isSyncing = true;
    video.paused = true;
    video.listeners.pause();
    JWP.state.isSyncing = false;
    JWP.utils.suppress();
    video.currentTime = 120;
    video.listeners.seeked();
    assert.deepEqual(sent, []);
  });

  it('sends nothing for a video event that only matches the room (an echo)', () => {
    video.listeners.play(); // room is already playing
    video.currentTime = 50.4; // seek landing where the room is
    video.listeners.seeked();
    assert.deepEqual(sent, []);
  });

  it('sends nothing while its stream is loading (autoplay / start position)', () => {
    video.listeners.loadstart();
    video.currentTime = 0;
    video.listeners.seeked();
    JWP.utils.mirrorRoomState(50, 'paused');
    video.listeners.play();
    assert.deepEqual(sent, []);
  });

  it('sends nothing without a sync baseline (just switched media)', () => {
    JWP.state.lastSyncServerTs = 0;
    video.paused = true;
    video.listeners.pause();
    assert.deepEqual(sent, []);
  });

  it('does not send a pause caused by buffering', () => {
    JWP.state.isBuffering = true;
    video.paused = true;
    video.listeners.pause();
    assert.deepEqual(sent, []);
  });
});

describe('playback/bind host in a democratic room', () => {
  let video;
  let sent;

  beforeEach(() => {
    video = makeVideo();
    video.currentTime = 50;
    video.paused = false;
    document.querySelector = (sel) => (sel === 'video' ? video : null);
    sent = [];
    JWP.actions = { send: (type, payload) => sent.push({ type, payload }) };
    Object.assign(JWP.state, {
      bound: false,
      currentVideoElement: null,
      videoListeners: null,
      inRoom: true,
      isHost: true,
      democraticMode: true,
      roomStarted: true,
      startPending: false,
      suppressUntil: 0,
      isSyncing: false,
      isBuffering: false,
      lastSeekSentAt: 0,
      lastSentPosition: 0,
      intervals: { stateUpdate: null }
    });
    JWP.playback.bindVideo();
    JWP.state.mediaSwitchPending = false;
    JWP.utils.mirrorRoomState(50, 'playing');
  });

  afterEach(() => {
    JWP.playback.cleanupVideoListeners();
    delete JWP.actions;
  });

  it('still sends its own commands with the paired state_update', () => {
    video.paused = true;
    video.listeners.pause();
    assert.deepEqual(sent.map((m) => m.type), ['player_event', 'state_update']);
    assert.equal(JWP.state.lastSyncPlayState, 'paused', 'mirrored');
  });

  it('a seek that lands where the room already is (an applied remote seek) is not re-sent', () => {
    // As after applying a guest's seek to 120: the baseline is the target,
    // and the seek completes after the suppression window ran out.
    JWP.utils.mirrorRoomState(120, 'playing');
    video.currentTime = 120.3;
    video.listeners.seeked();
    assert.deepEqual(sent, []);
  });

  it('its own seek elsewhere is sent', () => {
    video.currentTime = 300;
    video.listeners.seeked();
    assert.deepEqual(sent.map((m) => m.type), ['player_event', 'state_update']);
  });
});
