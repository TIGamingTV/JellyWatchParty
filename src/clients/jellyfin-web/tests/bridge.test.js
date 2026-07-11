const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// setup.js provides JWP.state / JWP.utils; bridge.js attaches its helpers
// (inRoom, buildFollowPath, buildSessionRow) to JWP.ui.
require('../ui/bridge.js');

// Minimal DOM stub: buildSessionRow only needs createElement returning a node
// whose innerHTML is a plain string and whose querySelector hands back a
// per-selector button stub we can inspect for a wired onclick.
function fakeRow() {
  const buttons = {};
  return {
    className: '',
    style: {},
    innerHTML: '',
    querySelector(sel) {
      buttons[sel] = buttons[sel] || { onclick: null };
      return buttons[sel];
    },
    buttons
  };
}

describe('bridge receiver gating', () => {
  const { inRoom, buildFollowPath, buildSessionRow } = JWP.ui;
  const session = { userName: 'A', deviceName: 'TV', nowPlayingItemName: 'Movie' };

  beforeEach(() => {
    globalThis.document = { createElement: () => fakeRow() };
    JWP.state.inRoom = false;
    JWP.state.roomId = '';
  });

  it('inRoom() is false with no room and true once in a room', () => {
    assert.equal(inRoom(), false);
    JWP.state.inRoom = true;
    JWP.state.roomId = 'ABC';
    assert.equal(inRoom(), true);
  });

  it('buildFollowPath encodes the session id and room id', () => {
    assert.equal(
      buildFollowPath('sess 1', 'ROOM/1'),
      '/JellyWatchParty/Bridge/sess%201/Follow?roomId=ROOM%2F1'
    );
  });

  it('offers only a wired Host action when not in a room', () => {
    const row = buildSessionRow(session);
    assert.match(row.innerHTML, /jwp-bridge-host/);
    assert.doesNotMatch(row.innerHTML, /jwp-bridge-receiver/);
    assert.equal(typeof row.buttons['.jwp-bridge-host'].onclick, 'function');
  });

  it('offers only a wired Receiver action when in a room', () => {
    JWP.state.inRoom = true;
    JWP.state.roomId = 'ABC';
    const row = buildSessionRow(session);
    assert.match(row.innerHTML, /jwp-bridge-receiver/);
    assert.doesNotMatch(row.innerHTML, /jwp-bridge-host/);
    assert.equal(typeof row.buttons['.jwp-bridge-receiver'].onclick, 'function');
  });
});
