const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// setup.js provides JWP.state / JWP.constants; render.js attaches
// injectGlobalButton to JWP.ui.
require('../ui/render.js');

const GLOBAL_BTN_ID = 'jwp-global-btn';

// Regression coverage for the "button not appearing on Jellyfin 12" class of
// bug: Jellyfin 12's default "modern" (React/MUI) layout wraps the entire
// legacy header — including .headerRight, which scripts/libraryMenu.js still
// builds unconditionally — in a display:none ancestor
// (`<AppHeader isHidden={layoutManager.modern || isNewLayoutPath} />` in
// jellyfin-web's RootAppRouter.tsx; apphost.js's getDefaultLayout() returns
// the modern layout unconditionally for any normal browser). So .headerRight
// still *exists* on v12, it's just invisible. A naive existence-only check
// falsely "succeeds" by inserting into the hidden node and never falls
// through to a v12-aware strategy — verified against jellyfin-web's
// release-12.z source, not assumed.

function makeClassList() {
  const set = new Set();
  return {
    add: (c) => set.add(c),
    remove: (c) => set.delete(c),
    contains: (c) => set.has(c)
  };
}

// Minimal fake DOM covering exactly what ui/render.js's global button
// injection touches: id-based registration (getElementById),
// prepend/appendChild/remove that maintain parentElement, classList,
// getBoundingClientRect/getClientRects/offsetWidth/offsetHeight (the
// jQuery-:visible-style check), and a previousElementSibling/children chain
// for the MUI actions-group anchor lookup.
function makeFakeDom() {
  const byId = new Map();
  const register = (el) => { if (el && el.id) byId.set(el.id, el); };
  const unregister = (el) => { if (el && el.id && byId.get(el.id) === el) byId.delete(el.id); };

  function makeElement() {
    const el = {
      id: '', className: '', type: '', title: '', innerHTML: '', onclick: null,
      style: {},
      parentElement: null,
      previousElementSibling: null,
      children: [],
      offsetWidth: 0,
      offsetHeight: 0,
      rect: { top: 0, left: 0, width: 0, height: 0 },
      setAttribute() {},
      getClientRects() { return []; },
      getBoundingClientRect() { return el.rect; },
      prepend(child) { el.children.unshift(child); child.parentElement = el; register(child); },
      appendChild(child) { el.children.push(child); child.parentElement = el; register(child); },
      remove() {
        if (el.parentElement) {
          const idx = el.parentElement.children.indexOf(el);
          if (idx >= 0) el.parentElement.children.splice(idx, 1);
        }
        el.parentElement = null;
        unregister(el);
      },
      querySelector() { return null; },
      classList: makeClassList()
    };
    return el;
  }

  const body = makeElement();
  const routes = { headerRight: null, avatar: null, syncPlayBtn: null };

  globalThis.document = {
    body,
    querySelector(sel) {
      if (sel === '.headerRight' || sel === '.skinHeader .headerRight') return routes.headerRight;
      if (sel === '[aria-controls="app-user-menu"]') return routes.avatar;
      if (sel === '[aria-controls="app-sync-play-menu"]') return routes.syncPlayBtn;
      return null;
    },
    getElementById: (id) => byId.get(id) || null,
    createElement: () => makeElement()
  };

  return { makeElement, routes, byId, body };
}

// A visible .headerRight (Jellyfin 10.11, or v12 with the user manually back
// on the legacy layout): non-zero offsetWidth, so isRendered() reports true.
function makeVisibleHeaderRight(makeElement) {
  const el = makeElement();
  el.offsetWidth = 100;
  el.offsetHeight = 40;
  return el;
}

// .headerRight the way Jellyfin 12's default layout actually renders it:
// present in the DOM but wrapped in a display:none ancestor, so it reports
// zero size everywhere isRendered() checks.
function makeHiddenHeaderRight(makeElement) {
  const el = makeElement();
  el.offsetWidth = 0;
  el.offsetHeight = 0;
  return el;
}

// The MUI toolbar's user-menu avatar button (aria-controls="app-user-menu"),
// wrapped in the same [actions Box (flexGrow:1), avatar Box (flexGrow:0)]
// sibling structure as components/toolbar/AppToolbar.tsx, optionally with a
// visible action button (e.g. Search) packed against it the way SyncPlay/
// RemotePlay/Search are in practice.
function makeAvatar(makeElement, { actionRect } = {}) {
  const avatar = makeElement();
  avatar.rect = { top: 8, left: 900, width: 40, height: 40 };
  const avatarBox = makeElement();
  avatarBox.children = [avatar];
  avatar.parentElement = avatarBox;

  const actionsGroup = makeElement();
  if (actionRect) {
    const visibleAction = makeElement();
    visibleAction.rect = actionRect;
    actionsGroup.children = [visibleAction];
  }
  avatarBox.previousElementSibling = actionsGroup;

  return avatar;
}

describe('injectGlobalButton — Jellyfin 10.11 / legacy layout header', () => {
  let dom;
  beforeEach(() => { dom = makeFakeDom(); });

  it('inserts into a visible .headerRight', () => {
    const headerRight = makeVisibleHeaderRight(dom.makeElement);
    dom.routes.headerRight = headerRight;

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected the button to be created');
    assert.equal(btn.parentElement, headerRight);
    assert.equal(typeof btn.onclick, 'function');
  });

  it('is a no-op on the next poll once inserted (no repositioning needed)', () => {
    const headerRight = makeVisibleHeaderRight(dom.makeElement);
    dom.routes.headerRight = headerRight;
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);

    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), btn, 'expected the same button instance, not recreated');
    assert.equal(btn.parentElement, headerRight, 'expected it to stay put in .headerRight');
  });
});

describe('injectGlobalButton — Jellyfin 12 default "modern" layout', () => {
  let dom;
  beforeEach(() => { dom = makeFakeDom(); });

  it('falls through to the MUI toolbar strategy when .headerRight exists but is hidden', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected the floating v12 button to be created');
    assert.equal(btn.parentElement, dom.body, 'expected it appended to document.body, not the hidden .headerRight');
    assert.match(btn.className, /jwp-global-btn-floating/);
  });

  it('does not create a button when neither the legacy header nor the MUI avatar is present', () => {
    // e.g. the video OSD or a public (login/select-server) path, where
    // Jellyfin 12's AppToolbar renders isUserMenuAvailable={false}.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);
  });

  it('positions the floating button relative to the avatar when no sibling action button is visible', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, `${900 - 40}px`);
    assert.equal(btn.style.top, `${Math.round(8 + (40 - 40) / 2)}px`);
  });

  it('anchors to the leftmost visible sibling action button instead of the avatar, to avoid overlapping it', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement, { actionRect: { top: 10, left: 820, width: 36, height: 36 } });
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, `${820 - 40}px`, 'expected to anchor off the visible action button, not the avatar');
  });

  it('anchors to the left of another plugin\'s floating button instead of overlapping it', () => {
    // Regression coverage for user-reported overlap with JellyPrivateLibraries,
    // which floats its own document.body-appended button using the exact
    // same "leftmost visible sibling minus an offset" anchoring trick — with
    // no visible sibling action button on this page, both would otherwise
    // compute the identical position (immediately left of the avatar).
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);

    const foreignBtn = dom.makeElement();
    foreignBtn.style.position = 'fixed';
    foreignBtn.rect = { top: 8, left: 860, width: 40, height: 40 };
    dom.body.children.push(foreignBtn);
    foreignBtn.parentElement = dom.body;

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, `${860 - 40}px`, 'expected to anchor off the foreign floating button, not the avatar');
  });

  it('ignores foreign fixed elements that are not roughly level with the avatar', () => {
    // e.g. some unrelated fixed banner/toast elsewhere on the page — should
    // not be mistaken for a competing toolbar button.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);

    const unrelatedFixed = dom.makeElement();
    unrelatedFixed.style.position = 'fixed';
    unrelatedFixed.rect = { top: 500, left: 20, width: 40, height: 40 };
    dom.body.children.push(unrelatedFixed);
    unrelatedFixed.parentElement = dom.body;

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, `${900 - 40}px`, 'expected to anchor off the avatar, ignoring the unrelated fixed element');
  });

  it('replaces the native MUI SyncPlay button in place when hideNativeSyncButton is enabled', () => {
    JWP.state.hideNativeSyncButton = true;
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement, { actionRect: { top: 10, left: 820, width: 36, height: 36 } });
    dom.routes.syncPlayBtn = dom.makeElement();
    dom.routes.syncPlayBtn.rect = { top: 10, left: 760, width: 48, height: 48 };

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, '760px', 'expected to sit exactly at the SyncPlay button\'s own position, not beside it');
    assert.equal(btn.style.top, `${Math.round(10 + (48 - 40) / 2)}px`);
    JWP.state.hideNativeSyncButton = false;
  });

  it('falls back to the normal anchor when hideNativeSyncButton is enabled but SyncPlay is absent', () => {
    JWP.state.hideNativeSyncButton = true;
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement, { actionRect: { top: 10, left: 820, width: 36, height: 36 } });
    // dom.routes.syncPlayBtn intentionally left null — SyncPlay isn't loaded/available.

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.left, `${820 - 40}px`);
    JWP.state.hideNativeSyncButton = false;
  });

  it('does not create the floating button on the admin dashboard (dashboardDocument)', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);
    dom.body.classList.add('dashboardDocument');

    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);
  });

  it('removes an existing floating button once navigation reaches the dashboard', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);
    JWP.ui.injectGlobalButton();
    assert.ok(dom.byId.get(GLOBAL_BTN_ID), 'expected the button to exist first');

    dom.body.classList.add('dashboardDocument');
    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined, 'expected it removed once the dashboard is active');
  });

  it('recreates the floating button after leaving the dashboard', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    dom.routes.avatar = makeAvatar(dom.makeElement);
    dom.body.classList.add('dashboardDocument');
    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);

    dom.body.classList.remove('dashboardDocument');
    JWP.ui.injectGlobalButton();

    assert.ok(dom.byId.get(GLOBAL_BTN_ID), 'expected the button to be (re)created');
  });
});
