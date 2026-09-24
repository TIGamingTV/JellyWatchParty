const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// utils/media.js needs utils.getPlaybackManager (utils/video.js).
require('../utils/video.js');
require('../utils/media.js');

const ITEM = 'abcdef0123456789abcdef0123456789';
const ITEM_DASHED = 'abcdef01-2345-6789-abcd-ef0123456789';
const USER = '11111111111111111111111111111111';

// Simulates Jellyfin 12.1: no global playbackManager, no OSD data-id, player
// route without an item id. Only GET /Sessions knows what is playing.
const installApiClient = () => {
  window.ApiClient = {
    deviceId: () => 'dev-1',
    accessToken: () => 'tok',
    serverAddress: () => 'http://jf',
    getCurrentUserId: () => USER
  };
};

const mockSessions = (responses) => {
  const calls = [];
  let i = 0;
  globalThis.fetch = async (url, opts) => {
    calls.push({ url, opts });
    const body = responses[Math.min(i++, responses.length - 1)];
    return { ok: true, json: async () => body };
  };
  return calls;
};

describe('utils/media server fallback (no global playbackManager)', () => {
  beforeEach(() => {
    delete window.playbackManager;
    delete window.PlaybackManager;
    delete window.NowPlayingItem;
    globalThis.document = { querySelector: () => null };
    window.location.hash = '#/video';
    globalThis.sessionStorage = { getItem: () => null };
    JWP.state.serverNowPlayingId = '';
    installApiClient();
  });

  afterEach(() => {
    delete globalThis.fetch;
    delete window.ApiClient;
  });

  it('getCurrentItemId returns null when nothing is known', () => {
    assert.equal(JWP.utils.getCurrentItemId(), null);
  });

  it('resolveCurrentItemId reads NowPlayingItem.Id of the own session', async () => {
    const calls = mockSessions([[
      { Id: 'other', DeviceId: 'dev-2', UserId: USER, NowPlayingItem: { Id: 'ffffffffffffffffffffffffffffffff' } },
      { Id: 'sess-1', DeviceId: 'dev-1', UserId: USER, NowPlayingItem: { Id: ITEM_DASHED } }
    ]]);
    const id = await JWP.utils.resolveCurrentItemId({ retries: 0 });
    assert.equal(id, ITEM);
    assert.equal(JWP.state.serverNowPlayingId, ITEM);
    assert.equal(JWP.utils.getCurrentItemId(), ITEM);
    assert.equal(calls[0].url, 'http://jf/Sessions?deviceId=dev-1');
    // Jellyfin 12 rejects a request that only carries X-Emby-Token (see
    // issue #71), so Authorization: MediaBrowser ... must be present too.
    assert.ok(calls[0].opts.headers.Authorization.startsWith('MediaBrowser '));
    assert.ok(calls[0].opts.headers.Authorization.includes('Token="tok"'));
    assert.equal(calls[0].opts.headers['X-Emby-Token'], 'tok');
  });

  it('apiFetch sends only Authorization, never X-Emby-Token alone (regression for #71: 401 on Jellyfin 12)', async () => {
    const calls = mockSessions([[]]);
    await JWP.utils.apiFetch('/Sessions?deviceId=dev-1');
    assert.ok(calls[0].opts.headers.Authorization, 'missing Authorization header');
    assert.ok(
      calls[0].opts.headers.Authorization.startsWith('MediaBrowser '),
      'Authorization header must use the MediaBrowser scheme, not a bare token'
    );
  });

  it('retries until the server reports the item', async () => {
    const calls = mockSessions([
      [{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER }],
      [{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER, NowPlayingItem: { Id: ITEM } }]
    ]);
    const id = await JWP.utils.resolveCurrentItemId({ retries: 3, delayMs: 1 });
    assert.equal(id, ITEM);
    assert.equal(calls.length, 2);
  });

  it('returns null after exhausting retries', async () => {
    const calls = mockSessions([[{ Id: 'sess-1', DeviceId: 'dev-1', UserId: USER }]]);
    const id = await JWP.utils.resolveCurrentItemId({ retries: 2, delayMs: 1 });
    assert.equal(id, null);
    assert.equal(calls.length, 3);
  });

  it('prefers a local id without calling the server', async () => {
    const calls = mockSessions([[]]);
    window.location.hash = `#/details?id=${ITEM}`;
    const id = await JWP.utils.resolveCurrentItemId({ retries: 0 });
    assert.equal(id, ITEM);
    assert.equal(calls.length, 0);
  });

  it('getOwnSession picks the session of the current user on a shared device', async () => {
    mockSessions([[
      { Id: 'sess-a', DeviceId: 'dev-1', UserId: '22222222222222222222222222222222' },
      { Id: 'sess-b', DeviceId: 'dev-1', UserId: USER, NowPlayingItem: { Id: ITEM } }
    ]]);
    const session = await JWP.utils.getOwnSession();
    assert.deepEqual(session, { id: 'sess-b', nowPlayingItemId: ITEM });
  });

  it('clearServerNowPlaying drops the cached id', () => {
    JWP.state.serverNowPlayingId = ITEM;
    JWP.utils.clearServerNowPlaying();
    assert.equal(JWP.utils.getCurrentItemId(), null);
  });
});

describe('utils.isOnRoomMedia (issue #71: gate sync/ready against the wrong item)', () => {
  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    window.location.hash = '#/video';
    globalThis.sessionStorage = { getItem: () => null };
    JWP.state.serverNowPlayingId = '';
    JWP.state.roomMediaId = '';
  });

  it('is true when the room has no media set yet - nothing to mismatch', () => {
    JWP.state.roomMediaId = '';
    assert.equal(JWP.utils.isOnRoomMedia(), true);
  });

  it('is true when the current item matches the room media', () => {
    JWP.state.roomMediaId = ITEM;
    JWP.state.serverNowPlayingId = ITEM;
    assert.equal(JWP.utils.isOnRoomMedia(), true);
  });

  it('is false when the current item is a different one', () => {
    JWP.state.roomMediaId = ITEM;
    JWP.state.serverNowPlayingId = 'ffffffffffffffffffffffffffffffff';
    assert.equal(JWP.utils.isOnRoomMedia(), false);
  });

  it('is false when nothing is currently known to be playing but the room has media', () => {
    JWP.state.roomMediaId = ITEM;
    assert.equal(JWP.utils.isOnRoomMedia(), false);
  });

  it('normalizes dashes/case on both sides before comparing', () => {
    JWP.state.roomMediaId = ITEM.toUpperCase();
    JWP.state.serverNowPlayingId = ITEM_DASHED;
    assert.equal(JWP.utils.isOnRoomMedia(), true);
  });
});
