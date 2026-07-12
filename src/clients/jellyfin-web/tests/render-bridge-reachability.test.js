const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// Regression guard for the class of bug where the Receiver action existed but
// was rendered only in a context where it could never fire. Unit-testing
// buildSessionRow in isolation missed it; this test drives the real render()
// and asserts the bridge picker container is present in BOTH panel views
// (lobby and in-room), so an action wired for one context is actually
// reachable there. The per-context button wiring itself is covered by
// bridge.test.js.
require('../ui/bridge.js');
require('../ui/render.js');

const { PANEL_ID } = JWP.constants;

function makePanel() {
  return {
    innerHTML: '',
    dataset: {},
    classList: { contains: () => false, toggle() {}, add() {}, remove() {} },
    children: [],
    querySelector: () => null
  };
}

function installDom(panel) {
  globalThis.document = {
    getElementById: (id) => (id === PANEL_ID ? panel : null),
    createElement: () => ({
      style: {}, classList: { add() {}, remove() {} },
      querySelector: () => null, appendChild() {}, insertAdjacentElement() {},
      setAttribute() {}, prepend() {}
    }),
    querySelector: () => null
  };
}

// Stub the ui.* helpers render() calls so it runs headless. updateBridgeListUI
// is a no-op here on purpose — this test checks the container exists, not that
// it gets populated (that path hits fetch and is covered elsewhere).
function stubUi() {
  Object.assign(JWP.ui, {
    updateRoomListUI() {},
    updateBridgeListUI() {},
    buildSyncStatusIndicator: () => '',
    updateStatusIndicator() {},
    updateServerFooter() {},
    updateSyncIndicator() {},
    renderHomeWatchParties() {},
    promptText: async () => null,
    stopPlayerCapture() {}
  });
}

describe('bridge picker reachability across panel views', () => {
  let panel;

  beforeEach(() => {
    panel = makePanel();
    installDom(panel);
    stubUi();
    JWP.state.clientId = 'jwp-testclient';
    JWP.state.roomName = 'Test Room';
    JWP.state.isHost = false;
    JWP.state.participantCount = 1;
    JWP.state.roomId = '';
    JWP.state.inRoom = false;
    // Both bridge roles are opt-in admin features; enable them so the picker
    // renders. Gating-off behaviour is covered separately below.
    JWP.state.allowThirdPartyHost = true;
    JWP.state.allowSupportedReceiver = true;
  });

  it('lobby view contains the bridge picker container', () => {
    JWP.state.inRoom = false;
    JWP.ui.render(true);
    assert.match(panel.innerHTML, /id="jwp-bridge-available"/);
    assert.match(panel.innerHTML, /Host From Another Device/);
  });

  it('in-room view also contains the bridge picker container', () => {
    JWP.state.inRoom = true;
    JWP.state.roomId = 'ROOM1';
    JWP.ui.render(true);
    assert.match(panel.innerHTML, /id="jwp-bridge-available"/);
    assert.match(panel.innerHTML, /Add a Device to This Room/);
  });
});

describe('bridge picker opt-in gating', () => {
  let panel;

  beforeEach(() => {
    panel = makePanel();
    installDom(panel);
    stubUi();
    JWP.state.clientId = 'jwp-testclient';
    JWP.state.roomName = 'Test Room';
    JWP.state.isHost = false;
    JWP.state.participantCount = 1;
    JWP.state.roomId = '';
    JWP.state.inRoom = false;
    JWP.state.allowThirdPartyHost = false;
    JWP.state.allowSupportedReceiver = false;
  });

  it('lobby hides the host picker when third-party hosting is disabled', () => {
    JWP.state.inRoom = false;
    JWP.ui.render(true);
    assert.doesNotMatch(panel.innerHTML, /Host From Another Device/);
    assert.doesNotMatch(panel.innerHTML, /id="jwp-bridge-available"/);
  });

  it('lobby shows the host picker only when third-party hosting is enabled', () => {
    JWP.state.allowThirdPartyHost = true;
    JWP.state.inRoom = false;
    JWP.ui.render(true);
    assert.match(panel.innerHTML, /Host From Another Device/);
  });

  it('in-room hides the receiver picker when supported receivers are disabled', () => {
    JWP.state.inRoom = true;
    JWP.state.roomId = 'ROOM1';
    JWP.ui.render(true);
    assert.doesNotMatch(panel.innerHTML, /Add a Device to This Room/);
    assert.doesNotMatch(panel.innerHTML, /id="jwp-bridge-available"/);
  });

  it('in-room shows the receiver picker only when supported receivers are enabled', () => {
    JWP.state.allowSupportedReceiver = true;
    JWP.state.inRoom = true;
    JWP.state.roomId = 'ROOM1';
    JWP.ui.render(true);
    assert.match(panel.innerHTML, /Add a Device to This Room/);
  });
});
