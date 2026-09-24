const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// ws/handlers/sync.js unconditionally calls ui.render()/showToast(), so a
// stub must exist before the module loads (unlike ui/playback access inside
// handlers, which is looked up fresh from JWP.* at call time).
JWP.ui = {
  render: () => {},
  showToast: () => {},
  updateSyncIndicator: () => {}
};

require('../utils/video.js');
require('../utils/media.js');
require('../ws/handlers/sync.js');

const ITEM_A = 'abcdef0123456789abcdef0123456789';
const ITEM_B = 'ffffffffffffffffffffffffffffffff';

describe('ws/handlers/sync handleMediaChanged (issue #71: room media follows the host)', () => {
  let ensureCalls;
  let watchReadyCalled;

  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    ensureCalls = [];
    watchReadyCalled = false;
    JWP.playback = {
      ensurePlayback: (id, pos) => ensureCalls.push({ id, pos }),
      watchReady: () => { watchReadyCalled = true; }
    };
    Object.assign(JWP.state, {
      isHost: false,
      roomMediaId: ITEM_A,
      readyRoomId: 'room-1',
      syncStatus: 'synced',
      lastSyncServerTs: 12345,
      lastSyncPosition: 99,
      lastSyncPlayState: 'playing',
      isInitialSync: true,
      initialSyncUntil: 999,
      initialSyncTargetPos: 5,
      syncCooldownUntil: 999,
      isDriftCorrecting: true,
      pendingPlayUntil: 999,
      pendingActionTimer: null,
      serverOffsetMs: 0
    });
  });

  const msg = (mediaId, position) => ({
    type: 'media_changed',
    room: 'room-1',
    payload: { media_id: mediaId, position },
    server_ts: JWP.utils.getServerNow()
  });

  it('updates roomMediaId and resets sync bookkeeping to a fresh, unknown state', () => {
    JWP._wsHandlers.handleMediaChanged(msg(ITEM_B, 0));
    assert.equal(JWP.state.roomMediaId, ITEM_B);
    assert.equal(JWP.state.readyRoomId, '');
    assert.equal(JWP.state.syncStatus, 'unknown');
    assert.equal(JWP.state.lastSyncServerTs, 0);
    assert.equal(JWP.state.lastSyncPosition, 0);
    assert.equal(JWP.state.lastSyncPlayState, 'paused');
    assert.equal(JWP.state.isInitialSync, false);
    assert.equal(JWP.state.isDriftCorrecting, false);
    assert.equal(JWP.state.pendingPlayUntil, 0);
  });

  it('calls ensurePlayback with the new item and adjusted position, then watchReady', () => {
    JWP._wsHandlers.handleMediaChanged(msg(ITEM_B, 42));
    assert.equal(ensureCalls.length, 1);
    assert.equal(ensureCalls[0].id, ITEM_B);
    assert.ok(ensureCalls[0].pos >= 42, 'position should be adjusted forward, never back');
    assert.equal(watchReadyCalled, true);
  });

  it('does nothing for the host - they sent this themselves', () => {
    JWP.state.isHost = true;
    JWP._wsHandlers.handleMediaChanged(msg(ITEM_B, 0));
    assert.equal(JWP.state.roomMediaId, ITEM_A, 'unchanged');
    assert.equal(ensureCalls.length, 0);
  });

  it('does nothing without a media_id in the payload', () => {
    JWP._wsHandlers.handleMediaChanged({ type: 'media_changed', room: 'room-1', payload: {} });
    assert.equal(JWP.state.roomMediaId, ITEM_A, 'unchanged');
    assert.equal(ensureCalls.length, 0);
  });
});
