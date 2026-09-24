const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// The ws handlers read JWP.ui at load time.
let rendered = 0;
JWP.ui = { showToast: () => {}, render: () => {} };

require('../utils/video.js');
require('../utils/log.js');
require('../ui/indicators.js');
require('../playback/sync.js');
require('../ws/handlers/room.js');

const realRender = JWP.ui.renderParticipants;
JWP.ui.renderParticipants = () => { rendered++; };
const h = JWP._wsHandlers;

const makeVideo = (paused = false) => ({ paused, readyState: 4, networkState: 1, seeking: false, currentTime: 0 });

let video;
let sent;

beforeEach(() => {
  video = makeVideo();
  document.querySelector = (sel) => (sel === 'video' ? video : null);
  sent = [];
  rendered = 0;
  JWP.actions = { send: (type, payload) => sent.push({ type, payload }) };
  Object.assign(JWP.state, {
    inRoom: true,
    isHost: false,
    clientId: 'me',
    isBuffering: false,
    syncStatus: 'synced',
    currentVideoElement: null,
    participants: [],
    participantCount: 0,
    lastReportedStatus: '',
    lastStatusReportAt: 0
  });
});

describe('own status', () => {
  it('maps the guest sync state', () => {
    for (const [syncStatus, expected] of [
      ['synced', 'synced'], ['syncing', 'syncing'], ['pending_play', 'loading'],
      ['wrong_media', 'loading'], ['unknown', 'syncing']
    ]) {
      JWP.state.syncStatus = syncStatus;
      assert.equal(JWP.playback.computeOwnStatus(), expected, syncStatus);
    }
  });

  it('reports buffering first, and idle without a video', () => {
    JWP.state.isBuffering = true;
    assert.equal(JWP.playback.computeOwnStatus(), 'buffering');
    video = null;
    assert.equal(JWP.playback.computeOwnStatus(), 'idle');
  });

  it('reports playing / paused for the host', () => {
    JWP.state.isHost = true;
    assert.equal(JWP.playback.computeOwnStatus(), 'playing');
    video.paused = true;
    assert.equal(JWP.playback.computeOwnStatus(), 'paused');
  });

  it('sends client_status only when it changes, at most once per second', () => {
    JWP.playback.reportOwnStatus();
    assert.deepEqual(sent, [{ type: 'client_status', payload: { status: 'synced' } }]);
    JWP.playback.reportOwnStatus();
    assert.equal(sent.length, 1, 'unchanged status is not re-sent');

    JWP.state.syncStatus = 'syncing';
    JWP.playback.reportOwnStatus();
    assert.equal(sent.length, 1, 'held back inside the 1 s window');

    JWP.state.lastStatusReportAt -= 1000;
    JWP.playback.reportOwnStatus();
    assert.deepEqual(sent[1], { type: 'client_status', payload: { status: 'syncing' } });
  });

  it('sends nothing outside a room', () => {
    JWP.state.inRoom = false;
    JWP.playback.reportOwnStatus();
    assert.equal(sent.length, 0);
  });
});

describe('participant list', () => {
  const list = [
    { id: 'host', name: 'Alice', is_host: true, status: 'playing' },
    { id: 'me', name: 'Bob', is_host: false, status: 'syncing' },
    { id: 'x', name: '<b>Eve</b>', is_host: false, status: 'weird' }
  ];

  it('stores the list from a participants message and re-renders', () => {
    h.handleParticipants({ type: 'participants', payload: { participants: list } });
    assert.equal(JWP.state.participants.length, 3);
    assert.equal(JWP.state.participantCount, 3);
    assert.equal(rendered, 1);
  });

  it('ignores a malformed participants message', () => {
    h.handleParticipants({ type: 'participants', payload: {} });
    assert.equal(JWP.state.participants.length, 0);
    assert.equal(rendered, 0);
  });

  it('renders names, host, "(you)" and status labels, escaping names', () => {
    JWP.state.participants = list;
    const html = JWP.ui.buildParticipantsHtml();
    assert.match(html, /Online: 3/);
    assert.match(html, /Alice<\/span><span class="material-icons jwp-participant-host"/);
    assert.match(html, /Playing/);
    assert.match(html, /Bob <span class="jwp-participant-you">\(you\)<\/span>/);
    assert.match(html, /Catching up/);
    assert.ok(!html.includes('<b>Eve</b>'), 'names are escaped');
    assert.match(html, /&lt;b&gt;Eve&lt;\/b&gt;/);
    assert.match(html, /jwp-sync-dot unknown/, 'unknown statuses get the neutral dot');
  });

  it('falls back to a plain count for servers without participant lists', () => {
    JWP.state.participantCount = 2;
    assert.equal(JWP.ui.buildParticipantsHtml(), 'Online: 2');
  });

  it('renders into the panel element', () => {
    const el = { innerHTML: '' };
    document.getElementById = (id) => (id === 'jwp-participants-list' ? el : null);
    JWP.state.participants = list;
    realRender();
    assert.match(el.innerHTML, /Alice/);
  });
});
