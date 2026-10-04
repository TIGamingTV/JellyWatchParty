const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// Handlers read JWP.ui at load time.
const toasts = [];
JWP.ui = {
  showToast: (m) => toasts.push(m),
  render: () => {},
  updateSyncIndicator: () => {},
  hideCountdown: () => {}
};

// In-memory storage stand-ins: the id + secret live in sessionStorage (per
// tab); the old shared localStorage copies are removed on load.
const makeStore = (map) => ({
  getItem: (k) => (map.has(k) ? map.get(k) : null),
  setItem: (k, v) => map.set(k, String(v)),
  removeItem: (k) => map.delete(k)
});
const store = new Map();
const shared = new Map([['owp_persistent_client_id', 'shared-id'], ['owp_resume_secret', 'shared']]);
globalThis.sessionStorage = makeStore(store);
globalThis.localStorage = makeStore(shared);
const panel = { classList: { hidden: false, add(c) { if (c === 'hide') this.hidden = true; } } };
document.getElementById = (id) => (id === JWP.constants.PANEL_ID ? panel : null);

require('../utils/video.js');
require('../utils/log.js');
require('../ws/send.js');
require('../ws/connection.js');
require('../ws/handlers/room.js');
require('../ws/handlers/sync.js');

const h = JWP._wsHandlers;

beforeEach(() => {
  toasts.length = 0;
  store.clear();
  panel.classList.hidden = false;
  Object.assign(JWP.state, {
    inRoom: false,
    roomId: '',
    clientId: 'me',
    isHost: false,
    participants: [],
    readyRoomId: '',
    pendingActionTimer: null,
    startSafetyTimer: null
  });
});

const roomState = (room, payload = {}) => ({
  type: 'room_state',
  room,
  client: 'me',
  server_ts: Date.now(),
  payload: { name: 'Movie night', host_id: 'someone', participant_count: 2, state: { position: 0, play_state: 'paused' }, ...payload }
});

describe('admin moves', () => {
  it('drops the old room and announces the new one', () => {
    Object.assign(JWP.state, { inRoom: true, roomId: 'old', readyRoomId: 'old', participants: [{ id: 'x' }] });

    h.handleRoomState(roomState('new', { admin_moved: true }), null);

    assert.equal(JWP.state.inRoom, true);
    assert.equal(JWP.state.roomId, 'new');
    assert.equal(JWP.state.roomName, 'Movie night');
    assert.equal(JWP.state.readyRoomId, '', 'old ready state is gone');
    assert.deepEqual(JWP.state.participants, []);
    assert.deepEqual(toasts, ['An admin added you to "Movie night"']);
    assert.equal(panel.classList.hidden, false, 'an admin move keeps the panel open');
  });

  it('drops a stale room even without the admin flag (moved while offline)', () => {
    Object.assign(JWP.state, { inRoom: true, roomId: 'old', isHost: true, readyRoomId: 'old' });
    h.handleRoomState(roomState('new'), null);
    assert.equal(JWP.state.roomId, 'new');
    assert.equal(JWP.state.readyRoomId, '');
    assert.equal(JWP.state.isHost, false);
  });

  it('leaves the room when client_hello says the server has us in none', () => {
    Object.assign(JWP.state, { inRoom: true, roomId: 'r1', isHost: true });
    h.handleClientHello({ type: 'client_hello', payload: { client_id: 'me', resume_secret: 's', room_id: null } });
    assert.equal(JWP.state.inRoom, false);
    assert.equal(JWP.state.isHost, false);
    assert.deepEqual(toasts, ['You are no longer in the watch party']);
  });

  it('keeps the room on client_hello from an older server (no room_id field)', () => {
    Object.assign(JWP.state, { inRoom: true, roomId: 'r1' });
    h.handleClientHello({ type: 'client_hello', payload: { client_id: 'me' } });
    assert.equal(JWP.state.inRoom, true);
  });

  it('works from the lobby too, and makes us host when the server says so', () => {
    h.handleRoomState(roomState('g1', { admin_moved: true, host_id: 'me' }), null);
    assert.equal(JWP.state.roomId, 'g1');
    assert.equal(JWP.state.isHost, true);
    assert.equal(toasts.length, 1);
  });

  it('stays quiet for a normal join', () => {
    h.handleRoomState(roomState('r1'), null);
    assert.equal(JWP.state.roomId, 'r1');
    assert.deepEqual(toasts, []);
  });

  it('shows the reason when an admin removes us', () => {
    Object.assign(JWP.state, { inRoom: true, roomId: 'r1' });
    JWP.state.isHost = true;
    h.handleRoomClosed({ type: 'room_closed', room: 'r1', payload: { reason: 'An admin removed you from the room', removed: true } });
    assert.equal(JWP.state.inRoom, false);
    assert.equal(JWP.state.isHost, false, 'a removed host is not host in the lobby');
    assert.deepEqual(toasts, ['An admin removed you from the room']);
  });
});

describe('resume secret', () => {
  it('removed the shared copies older versions kept in localStorage', () => {
    assert.equal(shared.size, 0);
  });

  it('stores the id and secret from client_hello', () => {
    h.handleClientHello({ type: 'client_hello', payload: { client_id: 'id-1', resume_secret: 's1' } });
    assert.equal(JWP.state.clientId, 'id-1');
    assert.equal(store.get('owp_persistent_client_id'), 'id-1');
    assert.equal(store.get('owp_resume_secret'), 's1');
  });

  it('adopts a new id when the server refused to reattach', () => {
    store.set('owp_persistent_client_id', 'old-id');
    store.set('owp_resume_secret', 'stale');
    h.handleClientHello({ type: 'client_hello', payload: { client_id: 'new-id', resume_secret: 's2' } });
    assert.equal(store.get('owp_persistent_client_id'), 'new-id');
    assert.equal(store.get('owp_resume_secret'), 's2');
  });

  it('forgets the secret when an older server sends none', () => {
    store.set('owp_resume_secret', 'stale');
    h.handleClientHello({ type: 'client_hello', payload: { client_id: 'id-3' } });
    assert.equal(store.has('owp_resume_secret'), false);
  });

  it('adds the secret to the websocket URL, encoded', () => {
    const url = JWP.actions._withClientId('wss://x/ws', 'abc', 's/=1');
    assert.equal(url, 'wss://x/ws?client_id=abc&resume=s%2F%3D1');
    assert.equal(JWP.actions._withClientId('wss://x/ws?a=1', 'abc'), 'wss://x/ws?a=1&client_id=abc');
  });
});
