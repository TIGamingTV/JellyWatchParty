const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// setup.js provides JWP.state / JWP.constants; render.js attaches
// injectGlobalButton to JWP.ui.
require('../ui/render.js');

const GLOBAL_BTN_ID = 'jwp-global-btn';

// Regression coverage for the "button not appearing on Jellyfin 12" class of
// bug, and for the strategy that replaced the old floating-button workaround.
//
// Two things about Jellyfin 12's default "modern" layout drive all of this:
//
// 1. It wraps the entire legacy header — including .headerRight, which
//    scripts/libraryMenu.js still builds unconditionally — in a display:none
//    ancestor (`<AppHeader isHidden={layoutManager.modern || isNewLayoutPath} />`
//    in RootAppRouter.tsx). So .headerRight still *exists* on v12, it's just
//    invisible, and an existence-only check falsely "succeeds" into a hidden
//    node instead of falling through to the v12-aware strategy.
//
// 2. It renders a React/MUI toolbar (components/toolbar/AppToolbar.tsx) whose
//    action buttons live in a Box immediately preceding the avatar's Box.
//    Contrary to a long-standing assumption in this plugin, React does not
//    remove foreign children from containers it manages — it reconciles
//    against its own fiber tree and never enumerates the real child list
//    (hydration is the sole exception, and jellyfin-web uses createRoot).
//    Verified empirically against the real 12.0 production build in Chromium:
//    an appended button survives 200 navigations, MUI menu churn and
//    breakpoint changes. So the button is injected in-flow, as a normal
//    sibling of SyncPlay/RemotePlay/Search, with no positioning math at all.
//
// All structure below mirrors jellyfin-web release-12.z, not guesswork.

function makeClassList(el) {
  return {
    add(c) {
      const parts = el.className ? el.className.split(/\s+/).filter(Boolean) : [];
      if (!parts.includes(c)) { parts.push(c); el.className = parts.join(' '); }
    },
    remove(c) {
      const parts = el.className ? el.className.split(/\s+/).filter(Boolean) : [];
      el.className = parts.filter((p) => p !== c).join(' ');
    },
    contains(c) {
      const parts = el.className ? el.className.split(/\s+/).filter(Boolean) : [];
      return parts.includes(c);
    }
  };
}

// Minimal fake DOM covering exactly what ui/render.js's global button
// injection touches: id-based registration (getElementById), prepend/
// appendChild/remove that maintain parentElement and sibling order, classList
// backed by className, offsetWidth/offsetHeight/getClientRects (the
// jQuery-:visible-style check), lastElementChild, and closest()/contains()/
// querySelectorAll for the MUI actions-box lookup.
function makeFakeDom() {
  const byId = new Map();
  const register = (el) => { if (el && el.id) byId.set(el.id, el); };
  const unregister = (el) => { if (el && el.id && byId.get(el.id) === el) byId.delete(el.id); };

  function detach(child) {
    if (!child.parentElement) return;
    const idx = child.parentElement.children.indexOf(child);
    if (idx >= 0) child.parentElement.children.splice(idx, 1);
    child.parentElement = null;
  }

  function makeElement(className = '') {
    const el = {
      id: '', className, type: '', title: '', innerHTML: '', onclick: null,
      style: {},
      parentElement: null,
      children: [],
      offsetWidth: 0,
      offsetHeight: 0,
      rect: { top: 0, left: 0, width: 0, height: 0 },
      setAttribute() {},
      getClientRects() { return []; },
      getBoundingClientRect() { return el.rect; },
      get lastElementChild() { return el.children[el.children.length - 1] || null; },
      get previousElementSibling() {
        if (!el.parentElement) return null;
        const idx = el.parentElement.children.indexOf(el);
        return idx > 0 ? el.parentElement.children[idx - 1] : null;
      },
      prepend(child) { detach(child); el.children.unshift(child); child.parentElement = el; register(child); },
      appendChild(child) { detach(child); el.children.push(child); child.parentElement = el; register(child); },
      remove() { detach(el); unregister(el); },
      contains(other) {
        for (let n = other; n; n = n.parentElement) if (n === el) return true;
        return false;
      },
      // Supports only the selector shapes render.js actually uses:
      // '.MuiToolbar-root' and '.MuiToolbar-root > *'.
      closest(sel) {
        const childOfToolbar = sel === '.MuiToolbar-root > *';
        const wanted = childOfToolbar ? sel.slice(0, sel.indexOf(' ')).slice(1) : sel.replace(/^\./, '');
        for (let n = el; n; n = n.parentElement) {
          const matches = childOfToolbar
            ? !!(n.parentElement && n.parentElement.classList.contains(wanted))
            : n.classList.contains(wanted);
          if (matches) return n;
        }
        return null;
      },
      querySelectorAll() {
        const out = [];
        (function walk(node) {
          for (const c of node.children) { out.push(c); walk(c); }
        })(el);
        return out;
      },
      querySelector() { return null; }
    };
    el.classList = makeClassList(el);
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
  const el = makeElement('headerRight');
  el.offsetWidth = 100;
  el.offsetHeight = 40;
  return el;
}

// .headerRight the way Jellyfin 12's default layout actually renders it:
// present in the DOM but wrapped in a display:none ancestor, so it reports
// zero size everywhere isRendered() checks.
function makeHiddenHeaderRight(makeElement) {
  const el = makeElement('headerRight');
  el.offsetWidth = 0;
  el.offsetHeight = 0;
  return el;
}

// Jellyfin 12's MUI toolbar, matching components/toolbar/AppToolbar.tsx:
//
//   <Toolbar class="MuiToolbar-root">
//     <Box>{children}</Box>          <- ServerButton / UserViewNav
//     <Box>{buttons}</Box>           <- SyncPlay / RemotePlay / Search
//     <Box><UserMenuButton/></Box>   <- the avatar
//   </Toolbar>
//
// `actions` names the buttons to place in the actions Box.
function makeMuiToolbar(dom, { actions = ['Search'] } = {}) {
  const toolbar = dom.makeElement('MuiToolbar-root MuiToolbar-dense');
  dom.body.appendChild(toolbar);

  const childrenBox = dom.makeElement('MuiBox-root css-children');
  toolbar.appendChild(childrenBox);

  const actionsBox = dom.makeElement('MuiBox-root css-actions');
  toolbar.appendChild(actionsBox);
  const created = {};
  for (const name of actions) {
    const btn = dom.makeElement(
      `MuiButtonBase-root MuiIconButton-root MuiIconButton-colorInherit MuiIconButton-sizeLarge css-${name.toLowerCase()}`
    );
    actionsBox.appendChild(btn);
    created[name] = btn;
    if (name === 'SyncPlay') dom.routes.syncPlayBtn = btn;
  }

  const avatarBox = dom.makeElement('MuiBox-root css-avatar');
  toolbar.appendChild(avatarBox);
  const avatar = dom.makeElement('MuiButtonBase-root MuiIconButton-root css-avatarbtn');
  avatarBox.appendChild(avatar);
  dom.routes.avatar = avatar;

  return { toolbar, actionsBox, avatarBox, avatar, buttons: created };
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
    assert.ok(btn.classList.contains('jwp-global-btn-legacy'));
  });

  it('is a no-op on the next poll once inserted (no repositioning needed)', () => {
    dom.routes.headerRight = makeVisibleHeaderRight(dom.makeElement);
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);

    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), btn, 'expected the same button instance, not recreated');
    assert.equal(btn.parentElement, dom.routes.headerRight, 'expected it to stay put in .headerRight');
  });

  it('is never removed by the MUI upkeep path, even on the dashboard', () => {
    // The legacy button's visibility is entirely CSS-driven by Jellyfin's own
    // header wrapper, so the v12 gating rules must not touch it.
    dom.routes.headerRight = makeVisibleHeaderRight(dom.makeElement);
    JWP.ui.injectGlobalButton();
    dom.body.classList.add('dashboardDocument');

    JWP.ui.injectGlobalButton();

    assert.ok(dom.byId.get(GLOBAL_BTN_ID), 'expected the legacy button to survive');
  });

  it('survives the legacy video OSD hiding the whole header', () => {
    // .skinHeader is hidden during legacy playback; there is no MUI toolbar to
    // move to, so the button must stay put rather than churn every OSD entry.
    const headerRight = makeVisibleHeaderRight(dom.makeElement);
    dom.routes.headerRight = headerRight;
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);

    headerRight.offsetWidth = 0;
    headerRight.offsetHeight = 0;
    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), btn, 'expected the same button, untouched');
    assert.equal(btn.parentElement, headerRight);
  });

  it('migrates to the MUI toolbar after a live layout switch', () => {
    // Settings -> Display -> Layout calls layoutManager.setLayout() without a
    // page reload, so a legacy button can be stranded in a now-hidden header
    // while the MUI toolbar takes over.
    const headerRight = makeVisibleHeaderRight(dom.makeElement);
    dom.routes.headerRight = headerRight;
    JWP.ui.injectGlobalButton();
    assert.ok(dom.byId.get(GLOBAL_BTN_ID).classList.contains('jwp-global-btn-legacy'));

    headerRight.offsetWidth = 0;
    headerRight.offsetHeight = 0;
    const ui = makeMuiToolbar(dom);
    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected a button to still exist');
    assert.equal(btn.parentElement, ui.actionsBox, 'expected it moved into the MUI actions Box');
    assert.ok(!btn.classList.contains('jwp-global-btn-legacy'), 'expected the MUI variant');
    assert.equal(headerRight.children.length, 0, 'expected the stranded legacy button removed');
  });
});

describe('injectGlobalButton — Jellyfin 12 default "modern" layout', () => {
  let dom;
  beforeEach(() => {
    dom = makeFakeDom();
    JWP.state.hideNativeSyncButton = false;
  });

  it('falls through to the MUI toolbar when .headerRight exists but is hidden', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom);

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected the v12 button to be created');
    assert.equal(btn.parentElement, ui.actionsBox,
      'expected it inside the MUI actions Box, not document.body and not the hidden .headerRight');
  });

  it('places the button in-flow with no positioning styles at all', () => {
    // The whole point of the rewrite: an in-flow flex child needs no
    // coordinates, so nothing may set position/top/left.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    makeMuiToolbar(dom);

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(btn.style.position, undefined);
    assert.equal(btn.style.top, undefined);
    assert.equal(btn.style.left, undefined);
  });

  it('sits last in the actions Box, next to the avatar', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: ['SyncPlay', 'RemotePlay', 'Search'] });

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.equal(ui.actionsBox.lastElementChild, btn);
    assert.equal(ui.actionsBox.children.length, 4);
  });

  it('clones a neighbouring MUI IconButton\'s classes so it is styled natively', () => {
    // MUI 6 keeps real styling in emotion hash classes; the stable Mui* names
    // carry none. Copying a live neighbour's class list is the only way to
    // match without hardcoding a hash that changes every MUI release.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: ['Search'] });

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn.className.startsWith(ui.buttons.Search.className),
      `expected the donor's classes to be cloned verbatim, got: ${btn.className}`);
    assert.ok(btn.classList.contains('css-search'), 'expected the emotion hash class to be carried over');
    assert.ok(btn.classList.contains('jwp-global-btn'));
  });

  it('falls back to standalone styling when the actions Box has no donor button', () => {
    // e.g. SyncPlay/RemotePlay/Search all hidden for this user on this page.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    makeMuiToolbar(dom, { actions: [] });

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected the button to still be created');
    assert.ok(btn.classList.contains('jwp-global-btn-standalone'));
    assert.ok(btn.classList.contains('MuiIconButton-root'));
  });

  it('never picks a JellyWatchParty button as its own style donor', () => {
    // A stale button can be left behind in the actions Box (e.g. carried over
    // from a torn-down toolbar). The donor scan must skip anything carrying
    // our id, or the class list would compound on every pass.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: [] });

    const stale = dom.makeElement('MuiButtonBase-root MuiIconButton-root css-stale-marker');
    stale.id = GLOBAL_BTN_ID;
    ui.actionsBox.appendChild(stale);
    dom.byId.delete(GLOBAL_BTN_ID); // not discoverable via getElementById

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected a fresh button');
    assert.ok(!btn.classList.contains('css-stale-marker'),
      `expected the stale JWP button to be skipped as a donor, got: ${btn.className}`);
    assert.ok(btn.classList.contains('jwp-global-btn-standalone'),
      'expected the standalone fallback since no genuine MUI donor exists');
  });

  it('does not create a button when neither the legacy header nor the MUI toolbar is present', () => {
    // The video OSD and public (login/select-server) paths render
    // isUserMenuAvailable={false}, so there is no avatar to anchor to.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);
  });

  it('is idempotent — repeated calls neither duplicate nor move it', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: ['SyncPlay', 'Search'] });
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);

    for (let i = 0; i < 10; i++) JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), btn, 'expected the same instance');
    assert.equal(ui.actionsBox.children.length, 3, 'expected no duplicates');
    assert.equal(ui.actionsBox.lastElementChild, btn);
  });

  it('re-pins itself to the trailing slot when React re-adds its own buttons in front', () => {
    // Observed in a real browser: after React tears down and rebuilds the
    // action buttons (e.g. leaving a public path), it re-inserts them relative
    // to its own fiber siblings, which can leave ours ahead of them.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: [] });
    JWP.ui.injectGlobalButton();
    const btn = dom.byId.get(GLOBAL_BTN_ID);

    const rebuilt = dom.makeElement('MuiButtonBase-root MuiIconButton-root css-search');
    ui.actionsBox.appendChild(rebuilt);
    assert.equal(ui.actionsBox.children[0], btn, 'precondition: ours is now first');

    JWP.ui.injectGlobalButton();

    assert.equal(ui.actionsBox.lastElementChild, btn, 'expected it moved back to the trailing slot');
    assert.equal(ui.actionsBox.children.length, 2, 'expected it moved, not duplicated');
  });

  it('survives a toolbar rebuild by being re-injected into the new one', () => {
    // Route transitions that unmount the toolbar (e.g. via /video) drop the
    // button with it; the next pass must recreate it in the new toolbar.
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const first = makeMuiToolbar(dom);
    JWP.ui.injectGlobalButton();
    assert.ok(dom.byId.get(GLOBAL_BTN_ID));

    first.toolbar.remove();
    dom.routes.avatar = null;
    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined, 'expected it gone with the unmounted toolbar');

    const second = makeMuiToolbar(dom);
    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected it recreated in the rebuilt toolbar');
    assert.equal(btn.parentElement, second.actionsBox);
  });

  it('removes itself when navigating to a page without the toolbar actions', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    makeMuiToolbar(dom);
    JWP.ui.injectGlobalButton();
    assert.ok(dom.byId.get(GLOBAL_BTN_ID), 'expected the button to exist first');

    // Public path: AppToolbar renders isUserMenuAvailable={false}.
    dom.routes.avatar = null;
    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);
  });

  it('does not create the button on the admin dashboard (dashboardDocument)', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    makeMuiToolbar(dom);
    dom.body.classList.add('dashboardDocument');

    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);
  });

  it('removes an existing button once navigation reaches the dashboard', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    makeMuiToolbar(dom);
    JWP.ui.injectGlobalButton();
    assert.ok(dom.byId.get(GLOBAL_BTN_ID), 'expected the button to exist first');

    dom.body.classList.add('dashboardDocument');
    JWP.ui.injectGlobalButton();

    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined, 'expected it removed once the dashboard is active');
  });

  it('recreates the button after leaving the dashboard', () => {
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom);
    dom.body.classList.add('dashboardDocument');
    JWP.ui.injectGlobalButton();
    assert.equal(dom.byId.get(GLOBAL_BTN_ID), undefined);

    dom.body.classList.remove('dashboardDocument');
    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn, 'expected the button to be (re)created');
    assert.equal(btn.parentElement, ui.actionsBox);
  });

  it('takes the freed slot when the native SyncPlay button is hidden', () => {
    // hideNativeSyncButton hides SyncPlay with display:none; because our
    // button is a sibling in the same flex container it simply moves up. No
    // rect measurement is involved, so nothing needs to change here — this
    // asserts the injection is unaffected by the setting either way.
    JWP.state.hideNativeSyncButton = true;
    dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
    const ui = makeMuiToolbar(dom, { actions: ['SyncPlay', 'Search'] });

    JWP.ui.injectGlobalButton();

    const btn = dom.byId.get(GLOBAL_BTN_ID);
    assert.ok(btn);
    assert.equal(btn.parentElement, ui.actionsBox);
    assert.equal(ui.actionsBox.lastElementChild, btn);
    JWP.state.hideNativeSyncButton = false;
  });
});

describe('observeToolbar', () => {
  let dom;
  beforeEach(() => { dom = makeFakeDom(); });

  it('is exported alongside a matching disconnect', () => {
    assert.equal(typeof JWP.ui.observeToolbar, 'function');
    assert.equal(typeof JWP.ui.disconnectToolbarObserver, 'function');
  });

  it('is a safe no-op where MutationObserver is unavailable', () => {
    // The node:test harness has no MutationObserver; init() must not throw.
    assert.equal(typeof window.MutationObserver, 'undefined');
    assert.doesNotThrow(() => JWP.ui.observeToolbar());
    assert.doesNotThrow(() => JWP.ui.disconnectToolbarObserver());
  });

  it('observes document.body and re-injects on mutation, coalesced per frame', () => {
    const observed = [];
    let cb = null;
    let disconnected = 0;
    const frames = [];

    window.MutationObserver = function (fn) {
      cb = fn;
      this.observe = (target, opts) => observed.push({ target, opts });
      this.disconnect = () => { disconnected++; };
    };
    window.requestAnimationFrame = (fn) => { frames.push(fn); return frames.length; };

    try {
      dom.routes.headerRight = makeHiddenHeaderRight(dom.makeElement);
      const ui = makeMuiToolbar(dom);

      JWP.ui.observeToolbar();
      assert.equal(observed.length, 1, 'expected exactly one observer');
      assert.equal(observed[0].target, dom.body);
      assert.deepEqual(observed[0].opts, { childList: true, subtree: true });

      // A single React commit emits many records; they must collapse into one
      // frame callback, or injectGlobalButton would re-enter its own observer.
      cb(); cb(); cb();
      assert.equal(frames.length, 1, 'expected the burst to be coalesced into one frame');

      frames[0]();
      const btn = dom.byId.get(GLOBAL_BTN_ID);
      assert.ok(btn, 'expected the observer to have injected the button');
      assert.equal(btn.parentElement, ui.actionsBox);

      // After the frame runs, a further mutation schedules again.
      cb();
      assert.equal(frames.length, 2);

      JWP.ui.disconnectToolbarObserver();
      assert.equal(disconnected, 1);

      // Re-arming after a disconnect must work (init after cleanup).
      JWP.ui.observeToolbar();
      assert.equal(observed.length, 2);
      JWP.ui.disconnectToolbarObserver();
    } finally {
      delete window.MutationObserver;
      delete window.requestAnimationFrame;
    }
  });

  it('does not attach a second observer while one is already active', () => {
    let count = 0;
    window.MutationObserver = function () {
      count++;
      this.observe = () => {};
      this.disconnect = () => {};
    };
    try {
      JWP.ui.observeToolbar();
      JWP.ui.observeToolbar();
      JWP.ui.observeToolbar();
      assert.equal(count, 1);
      JWP.ui.disconnectToolbarObserver();
    } finally {
      delete window.MutationObserver;
    }
  });
});
