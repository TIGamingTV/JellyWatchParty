const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// Democratic mode in the panel: the host's switch, everyone's info line and
// the lobby marker. Drives the real render() like
// render-bridge-reachability.test.js.
require('../ui/bridge.js');
require('../ui/cards.js');
require('../ui/render.js');

const { PANEL_ID } = JWP.constants;
const TOGGLE = 'id="jwp-democratic-toggle"';

let panel;
let toggle;
let roomList;

function makePanel() {
  toggle = { checked: false, onchange: null };
  return {
    innerHTML: '',
    dataset: {},
    classList: { contains: () => false, toggle() {}, add() {}, remove() {} },
    children: [],
    // Hand out the checkbox only when the markup has it, like the DOM would.
    querySelector: (sel) => (sel === '#jwp-democratic-toggle' && panel.innerHTML.includes(TOGGLE) ? toggle : null)
  };
}

function installDom() {
  roomList = { innerHTML: '', items: [], appendChild(el) { this.items.push(el); } };
  globalThis.document = {
    getElementById: (id) => {
      if (id === PANEL_ID) return panel;
      if (id === 'jwp-room-list') return roomList;
      return null;
    },
    createElement: () => ({
      style: {}, classList: { add() {}, remove() {} }, innerHTML: '',
      querySelector: () => null, appendChild() {}, insertAdjacentElement() {},
      setAttribute() {}, prepend() {}
    }),
    querySelector: () => null
  };
}

function stubUi() {
  Object.assign(JWP.ui, {
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

const renderRoom = (overrides) => {
  Object.assign(JWP.state, overrides);
  JWP.ui.render(true);
  return panel.innerHTML;
};

describe('democratic mode controls in the room panel', () => {
  beforeEach(() => {
    panel = makePanel();
    installDom();
    stubUi();
    Object.assign(JWP.state, {
      clientId: 'jwp-me',
      roomName: 'Room',
      participantCount: 2,
      roomId: 'ROOM1',
      inRoom: true,
      isHost: false,
      allowDemocraticMode: false,
      democraticMode: false,
      deviceControl: false,
      allowSupportedReceiver: false
    });
  });

  it('host sees the switch when an admin allows the mode', () => {
    const html = renderRoom({ isHost: true, allowDemocraticMode: true });
    assert.match(html, /jwp-checkbox-row/);
    assert.ok(html.includes(TOGGLE));
    assert.match(html, /Everyone can control playback/);
    assert.ok(!html.includes(`${TOGGLE} checked`), 'off while the mode is off');
  });

  it('host does not see the switch when the admin has not allowed it', () => {
    const html = renderRoom({ isHost: true });
    assert.ok(!html.includes(TOGGLE));
    assert.doesNotMatch(html, /Everyone can control playback/);
  });

  it('host still sees the (checked) switch for a mode an admin turned on, to turn it off', () => {
    const html = renderRoom({ isHost: true, democraticMode: true });
    assert.ok(html.includes(`${TOGGLE} checked`));
  });

  it('guests never see the switch', () => {
    const html = renderRoom({ allowDemocraticMode: true, democraticMode: true });
    assert.ok(!html.includes(TOGGLE));
  });

  it('everyone sees the info line while the mode is on', () => {
    const html = renderRoom({ democraticMode: true });
    assert.match(html, /jwp-democratic-info/);
    assert.match(html, /Everyone can control playback</);
  });

  it('the info line mentions TV devices when they may control too', () => {
    const html = renderRoom({ democraticMode: true, deviceControl: true });
    assert.match(html, /Everyone can control playback \(TV devices too\)/);
  });

  it('no info line while the mode is off', () => {
    const html = renderRoom({ allowDemocraticMode: true });
    assert.doesNotMatch(html, /jwp-democratic-info/);
  });

  it('toggling the switch asks the server (host only, confirmed by democratic_changed)', () => {
    const calls = [];
    JWP.actions = { setDemocratic: (on) => calls.push(on) };
    renderRoom({ isHost: true, allowDemocraticMode: true });
    assert.equal(typeof toggle.onchange, 'function');
    toggle.checked = true;
    toggle.onchange();
    toggle.checked = false;
    toggle.onchange();
    assert.deepEqual(calls, [true, false]);
    delete JWP.actions;
  });
});

describe('lobby room list marks democratic rooms', () => {
  beforeEach(() => {
    panel = makePanel();
    installDom();
  });

  it('shows the marker only on democratic rooms', () => {
    JWP.state.rooms = [
      { id: 'a', name: 'Open <b>', count: 2, democratic: true },
      { id: 'b', name: 'Classic', count: 1 }
    ];
    JWP.ui.updateRoomListUI();
    assert.equal(roomList.items.length, 2);
    assert.match(roomList.items[0].innerHTML, /how_to_vote/);
    assert.match(roomList.items[0].innerHTML, /Open &lt;b&gt;/, 'room name still escaped');
    assert.doesNotMatch(roomList.items[1].innerHTML, /how_to_vote/);
  });
});
