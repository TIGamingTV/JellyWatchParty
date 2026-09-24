const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// app/lifecycle.js needs JWP.ui/playback/utils/actions - all stubbed below
// rather than requiring their real modules, to keep this test focused on
// the exit/leave decision.
require('../app/lifecycle.js');

describe('onVideoPlayerExit: host stays in the room when the player closes (issue #71 case B/C)', () => {
  let leaveCalled;

  beforeEach(() => {
    globalThis.document.getElementById = () => null;
    leaveCalled = false;
    JWP.actions = JWP.actions || {};
    JWP.actions.leaveRoom = () => { leaveCalled = true; };
    JWP.playback = JWP.playback || {};
    JWP.playback.cleanupVideoListeners = () => {};
    JWP.utils.clearServerNowPlaying = () => {};
    JWP.state.inRoom = true;
    JWP.state.isHost = false;
  });

  it('a guest leaves the room when their player closes', () => {
    JWP.state.isHost = false;
    JWP._lifecycle.onVideoPlayerExit();
    assert.equal(leaveCalled, true);
  });

  it('the host does NOT leave - closing the player is how they browse for the next item', () => {
    JWP.state.isHost = true;
    JWP._lifecycle.onVideoPlayerExit();
    assert.equal(leaveCalled, false);
    // Still in the room, waiting to start something new.
    assert.equal(JWP.state.inRoom, true);
  });

  it('does nothing room-related when not in a room at all', () => {
    JWP.state.inRoom = false;
    JWP.state.isHost = false;
    JWP._lifecycle.onVideoPlayerExit();
    assert.equal(leaveCalled, false);
  });

  it('still clears playback bookkeeping regardless of host/guest', () => {
    JWP.state.isHost = true;
    JWP.state.playCommandItemId = 'abc';
    JWP.state.playCommandUntil = 12345;
    JWP.state.bound = true;
    JWP._lifecycle.onVideoPlayerExit();
    assert.equal(JWP.state.playCommandItemId, '');
    assert.equal(JWP.state.playCommandUntil, 0);
    assert.equal(JWP.state.bound, false);
  });
});
