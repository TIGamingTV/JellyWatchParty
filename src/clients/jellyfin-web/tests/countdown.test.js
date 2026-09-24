const { describe, it, beforeEach, afterEach, mock } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// Handlers and bind.js read JWP.ui at load time; record the overlay calls.
const overlay = [];
JWP.ui = {
  showToast: () => {},
  render: () => {},
  updateSyncIndicator: () => {},
  showStartWaiting: () => overlay.push('waiting'),
  showCountdown: (target) => overlay.push(['countdown', target]),
  hideCountdown: () => overlay.push('hide')
};

require('../utils/video.js');
require('../utils/log.js');
require('../utils/media.js');
require('../playback/bind.js');
require('../ws/handlers/sync.js');
require('../ws/handlers/playback.js');

const h = JWP._wsHandlers;

// A video element whose listeners bind.js registers, so tests can fire them.
const makeVideo = ({ paused = false, currentTime = 0 } = {}) => {
  const listeners = {};
  return {
    currentTime,
    paused,
    readyState: 4,
    networkState: 1,
    seeking: false,
    plays: 0,
    pauses: 0,
    listeners,
    play() { this.paused = false; this.plays++; return Promise.resolve(); },
    pause() { this.paused = true; this.pauses++; },
    addEventListener(type, fn) { listeners[type] = fn; },
    removeEventListener(type) { delete listeners[type]; }
  };
};

let video;
let sent;

const resetState = () => {
  Object.assign(JWP.state, {
    inRoom: true,
    isHost: true,
    roomId: 'room-1',
    clientId: 'host',
    roomStarted: false,
    startPending: false,
    startSafetyTimer: null,
    suppressUntil: 0,
    isSyncing: false,
    isBuffering: false,
    bound: false,
    currentVideoElement: null,
    lastSeekSentAt: 0,
    lastSentPosition: 0,
    pendingActionTimer: null,
    roomMediaId: '',
    mediaSwitchPending: false,
    mediaSwitchPendingUntil: 0,
    startQueued: false
  });
};

beforeEach(() => {
  video = makeVideo({ currentTime: 30 });
  document.querySelector = (sel) => (sel === 'video' ? video : null);
  sent = [];
  overlay.length = 0;
  JWP.actions = { send: (type, payload) => sent.push({ type, payload }) };
  resetState();
});

afterEach(() => {
  JWP.playback.cleanupVideoListeners();
  if (JWP.state.startSafetyTimer) clearTimeout(JWP.state.startSafetyTimer);
});

describe('host: the room\'s first play waits for the countdown', () => {
  beforeEach(() => {
    video.paused = true; // bound before playback; the play event starts it
    JWP.playback.bindVideo();
    JWP.state.mediaSwitchPending = false;
  });

  it('holds the video and asks the server to start', () => {
    video.paused = false; // the browser already started it
    video.listeners.play();
    assert.equal(video.pauses, 1, 'kept where it is');
    assert.equal(JWP.state.startPending, true);
    assert.deepEqual(overlay, ['waiting']);
    assert.deepEqual(sent, [{ type: 'player_event', payload: { action: 'play', position: 30, play_state: 'playing' } }]);
  });

  it('pressing play again while waiting sends nothing more', () => {
    video.listeners.play();
    JWP.state.suppressUntil = 0;
    video.listeners.play();
    assert.equal(sent.length, 1);
    assert.equal(video.pauses, 2);
  });

  it('plays normally once the room has started', () => {
    JWP.state.roomStarted = true;
    video.listeners.play();
    assert.equal(video.pauses, 0);
    assert.equal(sent[0].type, 'player_event');
    assert.equal(sent[0].payload.action, 'play');
    assert.equal(JWP.state.startPending, false);
  });

  it('starts at the shared moment when the countdown play arrives', () => {
    video.listeners.play();
    const target = JWP.utils.getServerNow(); // due now: runs synchronously
    video.currentTime = 35; // well past SEEK_THRESHOLD
    h.handlePlayerEvent({
      type: 'player_event',
      payload: { action: 'play', position: 30, target_server_ts: target, countdown: true },
      server_ts: target
    }, video);
    assert.deepEqual(overlay.slice(1), [['countdown', target], 'hide']);
    assert.equal(JWP.state.startPending, false);
    assert.equal(JWP.state.roomStarted, true);
    assert.equal(video.plays, 1);
    assert.equal(video.currentTime, 30, 'back to the agreed position');
    assert.equal(sent.length, 1, 'its own start is not echoed back');
  });

  it('gives up and plays if no countdown comes back', () => {
    mock.timers.enable({ apis: ['setTimeout'] });
    try {
      JWP.playback.cleanupVideoListeners();
      JWP.state.bound = false;
      JWP.playback.bindVideo();
      video.listeners.play();
      mock.timers.tick(JWP.constants.START_SAFETY_MS);
      assert.equal(JWP.state.startPending, false);
      assert.equal(JWP.state.roomStarted, true);
      assert.equal(video.plays, 1);
    } finally {
      mock.timers.reset();
    }
  });
});

describe('host: first play while the new item is still being confirmed', () => {
  it('holds the video and queues the start until set_media has gone out', () => {
    mock.timers.enable({ apis: ['setTimeout'] });
    try {
      window.ApiClient = undefined; // no server lookup: resolution just finishes
      JWP.playback.bindVideo(); // begins media resolution (mediaSwitchPending)
      assert.equal(JWP.state.mediaSwitchPending, true);
      video.listeners.play();
      assert.equal(video.pauses, 1, 'held right away');
      assert.equal(JWP.state.startQueued, true);
      assert.equal(sent.length, 0, 'nothing sent before the item is confirmed');

      mock.timers.tick(1500); // NOW_PLAYING_REFRESH_DELAY_MS
      return Promise.resolve().then(() => Promise.resolve()).then(() => {
        assert.equal(JWP.state.startQueued, false);
        assert.deepEqual(sent.map((m) => m.type), ['player_event']);
        assert.equal(sent[0].payload.action, 'play');
      });
    } finally {
      mock.timers.reset();
    }
  });
});

describe('host: autoplay before the video was bound', () => {
  it('holds a video that is already playing when bound, as the first play', () => {
    video.paused = false; // Jellyfin autoplayed before the UI poll bound it
    JWP.playback.bindVideo();
    assert.equal(video.pauses, 1);
    assert.equal(JWP.state.startPending, true);
    assert.equal(JWP.state.startQueued, true, 'sent once the new item is confirmed');
  });

  it('leaves a paused video alone (its play event will be caught)', () => {
    video.paused = true;
    JWP.playback.bindVideo();
    assert.equal(video.pauses, 0);
    assert.equal(JWP.state.startPending, false);
  });

  it('does not hold anything once the room has started', () => {
    JWP.state.roomStarted = true;
    video.paused = false;
    JWP.playback.bindVideo();
    assert.equal(video.pauses, 0);
  });
});

describe('host ignores player events it did not ask for', () => {
  it('does nothing without a pending start', () => {
    JWP.state.roomStarted = true;
    h.handlePlayerEvent({ type: 'player_event', payload: { action: 'play', position: 0 } }, video);
    assert.equal(video.plays, 0);
    assert.equal(overlay.length, 0);
  });
});

describe('guest', () => {
  beforeEach(() => {
    JWP.state.isHost = false;
    JWP.state.clientId = 'guest';
  });

  it('shows the countdown and marks the room started', () => {
    const target = JWP.utils.getServerNow() + 3000;
    h.handlePlayerEvent({
      type: 'player_event',
      payload: { action: 'play', position: 30, target_server_ts: target, countdown: true },
      server_ts: target
    }, video);
    assert.deepEqual(overlay[0], ['countdown', target]);
    assert.equal(JWP.state.roomStarted, true);
    if (JWP.state.pendingActionTimer) clearTimeout(JWP.state.pendingActionTimer);
  });

  it('shows "waiting for everyone" on start_pending', () => {
    JWP.state.roomStarted = true;
    h.handleStartPending({ type: 'start_pending', payload: { timeout_ms: 10000 } });
    assert.deepEqual(overlay, ['waiting']);
    assert.equal(JWP.state.roomStarted, false);
  });

  it('a pause clears the overlay', () => {
    h.handlePlayerEvent({ type: 'player_event', payload: { action: 'pause', position: 30 } }, video);
    assert.ok(overlay.includes('hide'));
  });
});

describe('room_state', () => {
  const roomState = (started) => ({
    type: 'room_state',
    room: 'room-1',
    payload: { name: 'Room', host_id: 'host', participant_count: 1, media_id: null, started, state: { position: 0, play_state: 'paused' } },
    server_ts: JWP.utils.getServerNow()
  });

  it('takes the started flag from the server', () => {
    video.paused = true;
    h.handleRoomState(roomState(false), null);
    assert.equal(JWP.state.roomStarted, false);
  });

  it('treats a missing flag (older server) as started', () => {
    h.handleRoomState(roomState(undefined), null);
    assert.equal(JWP.state.roomStarted, true);
  });

  it('a host already playing counts as started', () => {
    video.paused = false;
    h.handleRoomState(roomState(false), null);
    assert.equal(JWP.state.roomStarted, true);
  });
});
