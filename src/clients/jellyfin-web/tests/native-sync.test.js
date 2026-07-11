const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// setup.js provides JWP.state / JWP.constants; render.js attaches
// applyNativeSyncButtonVisibility to JWP.ui.
require('../ui/render.js');

// Minimal DOM stub good enough for the style-injection code path in
// ui/render.js's applyNativeSyncButtonVisibility (createElement + head +
// getElementById + remove).
function installFakeDom() {
  const byId = new Map();
  const head = {
    appendChild(node) { byId.set(node.id, node); }
  };
  globalThis.document = {
    head,
    createElement() {
      const node = {
        id: '',
        textContent: '',
        remove() { byId.delete(node.id); }
      };
      return node;
    },
    getElementById(id) { return byId.get(id) || null; }
  };
  return byId;
}

describe('applyNativeSyncButtonVisibility', () => {
  const { SYNC_HIDE_STYLE_ID } = JWP.constants;
  const apply = JWP.ui.applyNativeSyncButtonVisibility;
  let byId;

  beforeEach(() => {
    byId = installFakeDom();
    JWP.state.hideNativeSyncButton = false;
  });

  it('injects a hiding stylesheet when the flag is on', () => {
    JWP.state.hideNativeSyncButton = true;
    apply();
    const style = byId.get(SYNC_HIDE_STYLE_ID);
    assert.ok(style, 'expected a style element to be injected');
    assert.match(style.textContent, /\.headerSyncButton/);
    assert.match(style.textContent, /display:\s*none/);
  });

  it('does not inject anything when the flag is off', () => {
    apply();
    assert.equal(byId.get(SYNC_HIDE_STYLE_ID), undefined);
  });

  it('removes a previously injected stylesheet when the flag is turned off', () => {
    JWP.state.hideNativeSyncButton = true;
    apply();
    assert.ok(byId.get(SYNC_HIDE_STYLE_ID));

    JWP.state.hideNativeSyncButton = false;
    apply();
    assert.equal(byId.get(SYNC_HIDE_STYLE_ID), undefined);
  });

  it('is idempotent while the flag stays on (no duplicate injection)', () => {
    JWP.state.hideNativeSyncButton = true;
    apply();
    const first = byId.get(SYNC_HIDE_STYLE_ID);
    apply();
    assert.strictEqual(byId.get(SYNC_HIDE_STYLE_ID), first);
  });
});
