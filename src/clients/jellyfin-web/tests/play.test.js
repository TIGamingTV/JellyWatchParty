const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

require('../utils/video.js');
require('../utils/media.js');
require('../playback/play.js');
require('../ws/send.js');

const ITEM = 'abcdef0123456789abcdef0123456789';
const USER = '11111111111111111111111111111111';

const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

const installApiClient = () => {
  window.ApiClient = {
    deviceId: () => 'dev-1',
    accessToken: () => 'tok',
    serverAddress: () => 'http://jf',
    getCurrentUserId: () => USER,
    getItem: async (_userId, id) => ({ Id: id })
  };
};

// Records every request. GET /Sessions returns `sessions`; POSTs succeed.
const mockServer = (sessions) => {
  const calls = [];
  globalThis.fetch = async (url, opts = {}) => {
    calls.push({ url, method: opts.method || 'GET' });
    if (url.includes('/Sessions?')) return { ok: true, json: async () => sessions };
    return { ok: true, status: 204, json: async () => ({}) };
  };
  return calls;
};

const posts = (calls) => calls.filter((c) => c.method === 'POST');

describe('playback/play without global playbackManager (Jellyfin 12.1)', () => {
  beforeEach(() => {
    delete window.playbackManager;
    globalThis.document = { querySelector: () => null };
    window.location.hash = '#/home';
    globalThis.sessionStorage = { getItem: () => null };
    Object.assign(JWP.state, {
      serverNowPlayingId: '',
      playCommandItemId: '',
      playCommandUntil: 0,
      joiningItemId: ''
    });
    installApiClient();
  });

  afterEach(() => {
    delete globalThis.fetch;
    delete window.ApiClient;
  });

  it('sends one PlayNow command to the own session with the start position', async () => {
    const calls = mockServer([{ Id: 'sess 1', DeviceId: 'dev-1', UserId: USER }]);
    JWP.playback.ensurePlayback(ITEM, 12.5);
    await tick();
    const p = posts(calls);
    assert.equal(p.length, 1);
    assert.equal(
      p[0].url,
      `http://jf/Sessions/sess%201/Playing?playCommand=PlayNow&itemIds=${ITEM}&startPositionTicks=125000000`
    );
  });

  it('does not resend the command while the guard window is open', async () => {
    const calls = mockServer([{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER }]);
    JWP.playback.ensurePlayback(ITEM, 0);
    await tick();
    JWP.playback.ensurePlayback(ITEM, 0);
    await tick();
    assert.equal(posts(calls).length, 1);
  });

  it('does not restart playback when the session already plays the item', async () => {
    const calls = mockServer([{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER, NowPlayingItem: { Id: ITEM } }]);
    JWP.playback.ensurePlayback(ITEM, 0);
    await tick();
    assert.equal(posts(calls).length, 0);
    assert.equal(JWP.state.serverNowPlayingId, ITEM);
  });

  it('skips everything when the item is already known to be playing', async () => {
    const calls = mockServer([]);
    JWP.state.serverNowPlayingId = ITEM;
    JWP.playback.ensurePlayback(ITEM, 0);
    await tick();
    assert.equal(calls.length, 0);
  });

  it('uses the playbackManager path when one is exposed', async () => {
    const calls = mockServer([]);
    const played = [];
    window.playbackManager = { play: (opts) => played.push(opts) };
    JWP.playback.ensurePlayback(ITEM, 0);
    await tick();
    assert.equal(played.length, 1);
    assert.equal(played[0].items[0].Id, ITEM);
    assert.equal(calls.length, 0);
  });
});

describe('actions.createRoom media id', () => {
  let sent;

  beforeEach(() => {
    delete window.playbackManager;
    globalThis.document = { querySelector: () => null };
    window.location.hash = '#/video';
    globalThis.sessionStorage = { getItem: () => null };
    JWP.state.serverNowPlayingId = '';
    sent = [];
    JWP.state.ws = { readyState: 1, send: (raw) => sent.push(JSON.parse(raw)) };
    installApiClient();
  });

  afterEach(() => {
    delete globalThis.fetch;
    delete window.ApiClient;
    JWP.state.ws = null;
  });

  it('sends the item id resolved from the server session', async () => {
    mockServer([{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER, NowPlayingItem: { Id: ITEM } }]);
    await JWP.actions.createRoom();
    assert.equal(sent.length, 1);
    assert.equal(sent[0].type, 'create_room');
    assert.equal(sent[0].payload.media_id, ITEM);
  });
});
