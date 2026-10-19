const { describe, it, beforeEach, afterEach, mock } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// Democratic mode end to end on one client: remote commands are applied
// (host included), toasts credit whoever sent them, and nothing this client
// does to its own video while following the room goes back out as a command.

// Handlers read JWP.ui at load time.
const toasts = [];
const renders = [];
JWP.ui = {
  showToast: (m) => toasts.push(m),
  render: (force) => renders.push(!!force),
  updateSyncIndicator: () => {},
  showCountdown: () => {},
  hideCountdown: () => {},
  showStartWaiting: () => {}
};

require('../utils/video.js');
require('../utils/log.js');
require('../utils/media.js');
require('../playback/bind.js');
require('../playback/sync.js');
require('../ws/send.js');
require('../ws/handlers/room.js');
require('../ws/handlers/sync.js');
require('../ws/handlers/playback.js');
require('../ws/connection.js');

const h = JWP._wsHandlers;

const makeVideo = ({ paused = false, currentTime = 50 } = {}) => {
  const listeners = {};
  return {
    currentTime,
    paused,
    playbackRate: 1,
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

// What the browser does after a programmatic change: fire the event, but
// only once the time-based suppression has already run out (a slow seek, a
// late countdown), which is the case the echo filter has to cover.
const fireLate = (type) => {
  JWP.state.isSyncing = false;
  JWP.state.suppressUntil = 0;
  video.listeners[type]();
};

const playerEvents = () => sent.filter((m) => m.type === 'player_event' || m.type === 'state_update');

const enterRoom = ({ isHost = false, democratic = true } = {}) => {
  Object.assign(JWP.state, {
    inRoom: true,
    isHost,
    roomId: 'room-1',
    clientId: isHost ? 'host-id' : 'guest-id',
    roomMediaId: '',
    democraticMode: democratic,
    deviceControl: false,
    roomStarted: true,
    startPending: false,
    suppressUntil: 0,
    isSyncing: false,
    isBuffering: false,
    bound: false,
    currentVideoElement: null,
    videoListeners: null,
    lastSeekSentAt: 0,
    lastSentPosition: 0,
    pendingActionTimer: null,
    commandGuardUntil: 0,
    syncCooldownUntil: 0,
    isInitialSync: false,
    mediaSwitchPending: false,
    serverOffsetMs: 0,
    intervals: { stateUpdate: null }
  });
  JWP.playback.bindVideo();
  JWP.state.mediaSwitchPending = false;
  JWP.state.followerLoadingUntil = 0;
  JWP.utils.mirrorRoomState(50, 'playing');
};

const playerEvent = (payload, serverTs = JWP.utils.getServerNow()) => ({
  type: 'player_event',
  room: 'room-1',
  payload,
  server_ts: serverTs
});

beforeEach(() => {
  video = makeVideo();
  document.querySelector = (sel) => (sel === 'video' ? video : null);
  window.location.hash = '#/video';
  sent = [];
  toasts.length = 0;
  renders.length = 0;
  JWP.actions.send = (type, payload) => sent.push({ type, payload });
});

afterEach(() => {
  JWP.playback.cleanupVideoListeners();
  if (JWP.state.pendingActionTimer) {
    clearTimeout(JWP.state.pendingActionTimer);
    JWP.state.pendingActionTimer = null;
  }
});

describe('democratic_changed', () => {
  beforeEach(() => enterRoom({ democratic: false }));

  it('turns the mode on, credits who did it and re-renders fully', () => {
    h.handleDemocraticChanged({ type: 'democratic_changed', payload: { enabled: true, device_control: true, by: 'Alice' } });
    assert.equal(JWP.state.democraticMode, true);
    assert.equal(JWP.state.deviceControl, true);
    assert.deepEqual(toasts, ['Alice turned on democratic mode: everyone can control playback']);
    assert.deepEqual(renders, [true]);
  });

  it('turns it off', () => {
    JWP.state.democraticMode = true;
    JWP.state.commandGuardUntil = Date.now() + 5000;
    h.handleDemocraticChanged({ type: 'democratic_changed', payload: { enabled: false, device_control: false, by: 'an admin' } });
    assert.equal(JWP.state.democraticMode, false);
    assert.equal(JWP.state.commandGuardUntil, 0);
    assert.deepEqual(toasts, ['an admin turned off democratic mode: only the host controls playback']);
  });

  it('says who only when the server names someone', () => {
    h.handleDemocraticChanged({ type: 'democratic_changed', payload: { enabled: true, device_control: false, by: '' } });
    assert.deepEqual(toasts, ['Democratic mode on: everyone can control playback']);
  });

  it('re-renders without a toast when only device control changed', () => {
    JWP.state.democraticMode = true;
    h.handleDemocraticChanged({ type: 'democratic_changed', payload: { enabled: true, device_control: true, by: 'Bob' } });
    assert.equal(JWP.state.deviceControl, true);
    assert.deepEqual(toasts, []);
    assert.deepEqual(renders, [true]);
  });
});

describe('room_state', () => {
  const roomState = (extra) => ({
    type: 'room_state',
    room: 'room-1',
    client: 'guest-id',
    server_ts: Date.now(),
    payload: { name: 'R', host_id: 'host-id', participant_count: 2, media_id: null, started: true, state: { position: 0, play_state: 'paused' }, ...extra }
  });

  it('reads democratic and device_control', () => {
    Object.assign(JWP.state, { inRoom: false, roomId: '', clientId: 'guest-id' });
    h.handleRoomState(roomState({ democratic: true, device_control: true }), null);
    assert.equal(JWP.state.democraticMode, true);
    assert.equal(JWP.state.deviceControl, true);
  });

  it('treats missing fields (older server) as off', () => {
    Object.assign(JWP.state, { inRoom: false, roomId: '', clientId: 'guest-id', democraticMode: true });
    h.handleRoomState(roomState({}), null);
    assert.equal(JWP.state.democraticMode, false);
    assert.equal(JWP.state.deviceControl, false);
  });
});

describe('host applies other participants\' commands in a democratic room', () => {
  beforeEach(() => enterRoom({ isHost: true }));

  it('pauses on a guest\'s pause and credits them', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, play_state: 'paused', by: 'guest-id', by_name: 'Alice' }), video);
    assert.equal(video.paused, true);
    assert.deepEqual(toasts, ['Alice paused playback']);
    assert.equal(JWP.state.lastSyncPlayState, 'paused');
  });

  it('does not send the applied pause back out', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, play_state: 'paused', by: 'guest-id', by_name: 'Alice' }), video);
    video.listeners.pause();
    assert.deepEqual(playerEvents(), []);
  });

  it('seeks to a guest\'s seek without re-sending it, even when the seek completes late', () => {
    h.handlePlayerEvent(playerEvent({ action: 'seek', position: 300, play_state: 'playing', by: 'guest-id', by_name: 'Alice' }), video);
    assert.ok(Math.abs(video.currentTime - 300.3) < 0.01, 'at the new position (plus lead)');
    video.listeners.pause();
    video.listeners.play();
    video.listeners.seeked();
    fireLate('seeked');
    fireLate('play');
    assert.deepEqual(playerEvents(), []);
  });

  it('resumes on a guest\'s play when paused', () => {
    video.paused = true;
    JWP.utils.mirrorRoomState(50, 'paused');
    h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, play_state: 'playing', by: 'guest-id', by_name: 'Bob' }), video);
    assert.equal(video.plays, 1);
    assert.deepEqual(toasts, ['Bob resumed playback']);
    video.listeners.play();
    assert.deepEqual(playerEvents(), []);
  });

  it('a scheduled play applied after the suppression window is still not re-sent', () => {
    mock.timers.enable({ apis: ['setTimeout'] });
    try {
      video.paused = true;
      JWP.utils.mirrorRoomState(50, 'paused');
      const target = JWP.utils.getServerNow() + 1000;
      h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, play_state: 'playing', target_server_ts: target, by: 'guest-id', by_name: 'Bob' }, target), video);
      mock.timers.tick(1000);
      assert.equal(video.plays, 1);
      video.listeners.play();
      fireLate('play');
      assert.deepEqual(playerEvents(), []);
    } finally {
      mock.timers.reset();
    }
  });

  it('ignores a play while already playing (e.g. its own play the server held back)', () => {
    video.currentTime = 58;
    h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, play_state: 'playing', by: '' }), video);
    assert.equal(video.currentTime, 58, 'the host stays the position source');
    assert.equal(video.pauses, 0);
    assert.deepEqual(toasts, []);
  });

  it('ignores its own command coming back', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, by: 'host-id', by_name: 'Me' }), video);
    assert.equal(video.paused, false);
  });

  it('ignores buffering', () => {
    h.handlePlayerEvent(playerEvent({ action: 'buffering', position: 50, by: 'guest-id' }), video);
    assert.equal(video.paused, false);
  });

  it('ignores everything when the room is not democratic', () => {
    JWP.state.democraticMode = false;
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, by: 'guest-id', by_name: 'Alice' }), video);
    h.handlePlayerEvent(playerEvent({ action: 'seek', position: 300, play_state: 'playing', by: 'guest-id' }), video);
    assert.equal(video.paused, false);
    assert.equal(video.currentTime, 50);
    assert.deepEqual(toasts, []);
  });

  it('applies a chat-bot command (no client id) too', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, by: '', by_name: 'Carol' }), video);
    assert.equal(video.paused, true);
    assert.deepEqual(toasts, ['Carol paused playback']);
  });

  it('keeps ignoring state_update (still the position source)', () => {
    h.handleStateUpdate({ type: 'state_update', payload: { position: 10, play_state: 'paused' }, server_ts: Date.now() }, video);
    assert.equal(video.paused, false);
    assert.equal(video.currentTime, 50);
  });
});

describe('guest follows the room without echoing it back', () => {
  beforeEach(() => enterRoom());

  it('applying a remote pause does not emit a player_event', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, play_state: 'paused', by: 'other', by_name: 'Alice' }), video);
    assert.equal(video.paused, true);
    video.listeners.pause();
    fireLate('pause');
    assert.deepEqual(playerEvents(), []);
    assert.deepEqual(toasts, ['Alice paused playback']);
  });

  it('applying a remote seek does not emit a player_event, even if the seek completes late', () => {
    h.handlePlayerEvent(playerEvent({ action: 'seek', position: 300, play_state: 'playing', by: 'other' }), video);
    video.listeners.seeked();
    fireLate('seeked');
    fireLate('play');
    assert.deepEqual(playerEvents(), []);
  });

  it('applying a remote play does not emit a player_event', () => {
    video.paused = true;
    JWP.utils.mirrorRoomState(50, 'paused');
    h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, play_state: 'playing', by: 'other' }), video);
    video.listeners.play();
    fireLate('play');
    assert.deepEqual(playerEvents(), []);
  });

  it('a syncLoop hard seek does not emit a player_event', () => {
    video.currentTime = 30; // 20s behind the room
    JWP.playback.syncLoop();
    assert.ok(Math.abs(video.currentTime - 50) < 0.5, 'hard-seeked to the room');
    video.listeners.seeked();
    fireLate('seeked');
    assert.deepEqual(playerEvents(), []);
  });

  it('host state_update resume/pause does not emit a player_event', () => {
    video.paused = true;
    h.handleStateUpdate({ type: 'state_update', payload: { position: 50, play_state: 'playing' }, server_ts: JWP.utils.getServerNow() }, video);
    assert.equal(video.paused, false);
    video.listeners.play();
    fireLate('play');
    h.handleStateUpdate({ type: 'state_update', payload: { position: 50, play_state: 'paused' }, server_ts: JWP.utils.getServerNow() }, video);
    assert.equal(video.paused, true);
    video.listeners.pause();
    fireLate('pause');
    assert.deepEqual(playerEvents(), []);
  });

  it('joining (room_state) does not emit a player_event', () => {
    h.handleRoomState({
      type: 'room_state',
      room: 'room-1',
      client: 'guest-id',
      server_ts: JWP.utils.getServerNow(),
      payload: { name: 'R', host_id: 'host-id', participant_count: 2, media_id: null, started: true, democratic: true, state: { position: 200, play_state: 'paused' } }
    }, video);
    assert.equal(video.paused, true);
    assert.ok(Math.abs(video.currentTime - 200) < 1, 'moved to the room position');
    video.listeners.pause();
    video.listeners.seeked();
    fireLate('pause');
    fireLate('seeked');
    assert.deepEqual(playerEvents(), []);
  });

  it('a countdown play that starts after the suppression window is not re-sent', () => {
    mock.timers.enable({ apis: ['setTimeout'] });
    try {
      video.paused = true;
      JWP.state.roomStarted = false;
      JWP.utils.mirrorRoomState(50, 'paused');
      const target = JWP.utils.getServerNow() + 3000;
      h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, target_server_ts: target, countdown: true }, target), video);
      mock.timers.tick(3000);
      assert.equal(video.plays, 1);
      fireLate('play');
      assert.deepEqual(playerEvents(), []);
    } finally {
      mock.timers.reset();
    }
  });

  it('a media change (new stream loading, autoplay) is not re-sent', () => {
    JWP.playback.ensurePlayback = () => {};
    JWP.playback.watchReady = () => {};
    h.handleMediaChanged({ type: 'media_changed', room: 'room-1', payload: { media_id: 'abcdef0123456789abcdef0123456789', position: 0 }, server_ts: Date.now() });
    video.listeners.loadstart();
    video.currentTime = 0;
    fireLate('seeked');
    fireLate('play');
    video.listeners.canplay();
    fireLate('play');
    assert.deepEqual(playerEvents(), []);
  });

  it('host toast falls back to "Host" without by_name (older server)', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50 }), video);
    assert.deepEqual(toasts, ['Host paused playback']);
  });

  it('no toast for its own command coming back', () => {
    h.handlePlayerEvent(playerEvent({ action: 'play', position: 50, by: 'guest-id', by_name: 'Me' }), video);
    assert.deepEqual(toasts, []);
  });
});

describe('a guest\'s command is not undone by a host update already in flight', () => {
  beforeEach(() => enterRoom());

  const stateUpdate = (position, playState) => ({
    type: 'state_update',
    room: 'room-1',
    payload: { position, play_state: playState },
    server_ts: JWP.utils.getServerNow()
  });

  it('ignores a stale "playing" right after its own pause, until the host agrees', () => {
    video.paused = true;
    video.listeners.pause();
    assert.equal(playerEvents().length, 1, 'the pause went out');
    h.handleStateUpdate(stateUpdate(50.5, 'playing'), video);
    assert.equal(video.paused, true, 'not resumed by the stale update');
    assert.equal(JWP.state.lastSyncPlayState, 'paused');
    h.handleStateUpdate(stateUpdate(50, 'paused'), video);
    assert.equal(JWP.state.commandGuardUntil, 0, 'host agreed: guard over');
    h.handleStateUpdate(stateUpdate(50, 'playing'), video);
    assert.equal(video.paused, false, 'later updates apply again');
  });

  it('ignores a stale position right after its own seek', () => {
    video.currentTime = 300;
    video.listeners.seeked();
    h.handleStateUpdate(stateUpdate(51, 'playing'), video);
    assert.equal(JWP.state.lastSyncPosition, 300, 'baseline not pulled back');
    JWP.playback.syncLoop();
    assert.ok(Math.abs(video.currentTime - 300) < 0.1, 'no hard seek back');
  });

  it('also guards a command applied from another participant', () => {
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, play_state: 'paused', by: 'other', by_name: 'Alice' }), video);
    h.handleStateUpdate(stateUpdate(50.5, 'playing'), video);
    assert.equal(video.paused, true);
  });

  it('is not armed outside democratic mode', () => {
    JWP.state.democraticMode = false;
    h.handlePlayerEvent(playerEvent({ action: 'pause', position: 50, play_state: 'paused' }), video);
    assert.equal(JWP.state.commandGuardUntil, 0);
    h.handleStateUpdate(stateUpdate(50.5, 'playing'), video);
    assert.equal(video.paused, false, 'host-only rooms follow the host as before');
  });

  it('expires', () => {
    JWP.utils.armCommandGuard(50, 'paused');
    JWP.state.commandGuardUntil = Date.now() - 1;
    assert.equal(JWP.utils.commandGuardBlocks(stateUpdate(80, 'playing')), false);
    assert.equal(JWP.state.commandGuardUntil, 0);
  });
});

describe('utils.canControlPlayback', () => {
  it('host always; a guest only in a started democratic room', () => {
    Object.assign(JWP.state, { isHost: true, democraticMode: false, roomStarted: false });
    assert.equal(JWP.utils.canControlPlayback(), true);
    Object.assign(JWP.state, { isHost: false, democraticMode: false, roomStarted: true });
    assert.equal(JWP.utils.canControlPlayback(), false);
    Object.assign(JWP.state, { democraticMode: true, roomStarted: false });
    assert.equal(JWP.utils.canControlPlayback(), false);
    Object.assign(JWP.state, { democraticMode: true, roomStarted: true });
    assert.equal(JWP.utils.canControlPlayback(), true);
  });
});

describe('ws/send', () => {
  it('setDemocratic sends set_democratic for the current room', () => {
    const frames = [];
    JWP.state.ws = { readyState: 1, send: (d) => frames.push(JSON.parse(d)) };
    JWP.state.roomId = 'room-1';
    JWP.actions.setDemocratic(true);
    JWP.actions.setDemocratic(false);
    assert.deepEqual(frames.map((f) => [f.type, f.room, f.payload]), [
      ['set_democratic', 'room-1', { enabled: true }],
      ['set_democratic', 'room-1', { enabled: false }]
    ]);
    JWP.state.ws = null;
  });

  it('leaving forgets the room\'s democratic mode', () => {
    Object.assign(JWP.state, { democraticMode: true, deviceControl: true, commandGuardUntil: Date.now() + 1000 });
    JWP.actions.resetRoomState();
    assert.equal(JWP.state.democraticMode, false);
    assert.equal(JWP.state.deviceControl, false);
    assert.equal(JWP.state.commandGuardUntil, 0);
  });
});

describe('ws/connection', () => {
  const sockets = [];

  beforeEach(() => {
    sockets.length = 0;
    globalThis.WebSocket = class {
      constructor(url) { this.url = url; this.readyState = 1; this.frames = []; sockets.push(this); }
      send(d) { this.frames.push(JSON.parse(d)); }
      close() {}
    };
    globalThis.WebSocket.OPEN = 1;
    Object.assign(JWP.state, { ws: null, isConnecting: false, autoReconnect: false, wsUrl: '', reconnectAttempts: 1 });
  });

  afterEach(() => {
    if (JWP.state.intervals.ping) clearInterval(JWP.state.intervals.ping);
    JWP.state.intervals.ping = null;
    JWP.state.ws = null;
  });

  const open = async () => {
    await JWP.actions.connect();
    sockets[0].onopen();
    return sockets[0].frames.find((f) => f.type === 'auth');
  };

  it('announces the democratic capability in auth', async () => {
    Object.assign(JWP.state, { authToken: 'tok', userName: 'Alice', userId: 'u1' });
    const auth = await open();
    assert.deepEqual(auth.payload, { token: 'tok', user_name: 'Alice', user_id: 'u1', capabilities: ['democratic'] });
  });

  it('still sends no bare auth without any identity (JWT servers would reject it)', async () => {
    Object.assign(JWP.state, { authToken: null, userName: '', userId: '' });
    JWP.actions.fetchAuthToken = async () => null;
    const auth = await open();
    assert.equal(auth, undefined);
    delete JWP.actions.fetchAuthToken;
  });

  it('routes democratic_changed to its handler', async () => {
    Object.assign(JWP.state, { authToken: 'tok', userName: 'Alice', inRoom: true, roomId: 'room-1', democraticMode: false });
    await open();
    sockets[0].onmessage({ data: JSON.stringify({ type: 'democratic_changed', room: 'room-1', payload: { enabled: true, device_control: false, by: 'Host' } }) });
    assert.equal(JWP.state.democraticMode, true);
  });
});
