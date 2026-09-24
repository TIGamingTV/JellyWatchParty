const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

require('../ui/render.js');

const { PANEL_ID } = JWP.constants;

// A panel element whose visibility is driven by the 'hide' class, like the
// real one created in app/lifecycle.js.
const makePanel = (open) => {
  const classes = new Set(open ? [] : ['hide']);
  return {
    id: PANEL_ID,
    classList: {
      add: (c) => classes.add(c),
      remove: (c) => classes.delete(c),
      contains: (c) => classes.has(c),
      toggle: (c) => (classes.has(c) ? classes.delete(c) : classes.add(c))
    }
  };
};

// A click target: `insideExempt` decides what closest() finds for the
// dismiss-exempt selector (the panel, its toggle buttons, modal, toasts).
const makeTarget = (insideExempt) => {
  const target = {
    seenSelector: null,
    closest(selector) {
      target.seenSelector = selector;
      return insideExempt ? {} : null;
    }
  };
  return target;
};

describe('panel dismiss (X button, Escape, click outside)', () => {
  let panel;
  let modalOpen;

  beforeEach(() => {
    panel = makePanel(true);
    modalOpen = false;
    globalThis.document = {
      getElementById: (id) => (id === PANEL_ID ? panel : null),
      querySelector: (sel) => (sel === '.jwp-modal-overlay' && modalOpen ? {} : null)
    };
  });

  afterEach(() => {
    JWP.ui.teardownPanelDismiss();
    globalThis.document = { querySelector: () => null };
  });

  it('hidePanel hides an open panel', () => {
    JWP.ui.hidePanel();
    assert.equal(JWP.ui.isPanelOpen(), false);
  });

  it('dismisses on a pointer outside the panel', () => {
    assert.equal(JWP.ui.shouldDismissOnPointer(makeTarget(false)), true);
  });

  it('keeps the panel open for a pointer inside the panel or on an exempt element', () => {
    assert.equal(JWP.ui.shouldDismissOnPointer(makeTarget(true)), false);
  });

  it('exempts the panel, both toggle buttons, the modal and the toasts', () => {
    const target = makeTarget(false);
    JWP.ui.shouldDismissOnPointer(target);
    for (const sel of [`#${PANEL_ID}`, '#jwp-osd-btn', '#jwp-global-btn', '.jwp-modal-overlay', '.jwp-toast-container']) {
      assert.ok(target.seenSelector.includes(sel), `missing ${sel}`);
    }
  });

  it('ignores pointers while the panel is already closed', () => {
    panel = makePanel(false);
    assert.equal(JWP.ui.shouldDismissOnPointer(makeTarget(false)), false);
  });

  it('ignores targets without closest() (e.g. the document itself)', () => {
    assert.equal(JWP.ui.shouldDismissOnPointer({}), false);
    assert.equal(JWP.ui.shouldDismissOnPointer(null), false);
  });

  it('dismisses on Escape while open', () => {
    assert.equal(JWP.ui.shouldDismissOnKey({ key: 'Escape' }), true);
  });

  it('leaves Escape to an open password modal', () => {
    modalOpen = true;
    assert.equal(JWP.ui.shouldDismissOnKey({ key: 'Escape' }), false);
  });

  it('ignores other keys and a closed panel', () => {
    assert.equal(JWP.ui.shouldDismissOnKey({ key: 'Enter' }), false);
    panel = makePanel(false);
    assert.equal(JWP.ui.shouldDismissOnKey({ key: 'Escape' }), false);
  });

  it('registers capture-phase listeners once and removes them on teardown', () => {
    const added = [];
    const removed = [];
    window.addEventListener = (type, fn, capture) => added.push([type, capture]);
    window.removeEventListener = (type, fn, capture) => removed.push([type, capture]);
    try {
      JWP.ui.setupPanelDismiss();
      JWP.ui.setupPanelDismiss();
      assert.deepEqual(added, [['pointerdown', true], ['keydown', true]]);
      JWP.ui.teardownPanelDismiss();
      assert.deepEqual(removed, [['pointerdown', true], ['keydown', true]]);
    } finally {
      delete window.addEventListener;
      delete window.removeEventListener;
    }
  });
});
