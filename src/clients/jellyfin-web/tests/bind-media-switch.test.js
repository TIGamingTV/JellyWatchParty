const { describe, it, beforeEach, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// playback/bind.js needs utils.getVideo/getPlaybackManager (utils/video.js),
// utils.getCurrentItemId/normalizeItemId/refreshServerNowPlaying
// (utils/media.js) and utils.log (utils/log.js).
require('../utils/video.js');
require('../utils/media.js');
require('../utils/log.js');
require('../playback/bind.js');

const ITEM_A = 'abcdef0123456789abcdef0123456789';
const ITEM_B = 'ffffffffffffffffffffffffffffffff';

describe('playback/bind set_media detection (issue #71: room media follows the host)', () => {
  let sent;

  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    delete window.playbackManager;
    delete window.PlaybackManager;
    sent = [];
    JWP.actions = { send: (type, payload) => sent.push({ type, payload }) };
    Object.assign(JWP.state, {
      isHost: true,
      inRoom: true,
      roomMediaId: ITEM_A,
      serverNowPlayingId: '',
      mediaSwitchPending: false,
      mediaSwitchPendingUntil: 0
    });
  });

  afterEach(() => {
    // beginMediaResolution schedules a real setTimeout (NOW_PLAYING_REFRESH_
    // DELAY_MS) via scheduleNowPlayingRefresh; cleanupVideoListeners is the
    // only exported way to cancel it, so a stray timer can't fire against a
    // later test's state.
    JWP.playback.cleanupVideoListeners();
  });

  describe('maybeSendSetMedia (no global playbackManager - Jellyfin 12.1+)', () => {
    it('sends set_media when the server-confirmed item differs from the room media', () => {
      JWP.state.serverNowPlayingId = ITEM_B;
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 1);
      assert.equal(sent[0].type, 'set_media');
      assert.equal(sent[0].payload.media_id, ITEM_B);
      assert.equal(JWP.state.roomMediaId, ITEM_B, 'applied optimistically - host gets no echo');
    });

    it('does nothing when the item matches the room media already', () => {
      JWP.state.serverNowPlayingId = ITEM_A;
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 0);
    });

    it('does nothing when nothing is confirmed playing yet', () => {
      JWP.state.serverNowPlayingId = '';
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 0);
      assert.equal(JWP.state.roomMediaId, ITEM_A, 'unchanged');
    });

    it('does nothing for a guest', () => {
      JWP.state.isHost = false;
      JWP.state.serverNowPlayingId = ITEM_B;
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 0);
    });

    it('does nothing outside a room', () => {
      JWP.state.inRoom = false;
      JWP.state.serverNowPlayingId = ITEM_B;
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 0);
    });

    it('handles the empty-media room case (first item after creating with nothing playing)', () => {
      JWP.state.roomMediaId = '';
      JWP.state.serverNowPlayingId = ITEM_A;
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 1);
      assert.equal(sent[0].payload.media_id, ITEM_A);
    });

    it('always clears mediaSwitchPending, even on a no-op', () => {
      JWP.state.mediaSwitchPending = true;
      JWP.state.serverNowPlayingId = ITEM_A; // matches - no-op
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(JWP.state.mediaSwitchPending, false);
    });

    it('ignores DOM heuristics when there is no playbackManager - the report showed a false match there (rating button)', () => {
      // A misleading DOM element carrying the same data-id attribute the
      // fallback selector looks for (see utils/media.js getItemIdFromDom).
      globalThis.document = {
        querySelector: (sel) => (sel.includes('data-id') ? { dataset: { id: ITEM_B } } : null)
      };
      JWP.state.serverNowPlayingId = ''; // server hasn't confirmed anything
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 0, 'must not trust the DOM fallback on this path');
    });
  });

  describe('maybeSendSetMedia (with a global playbackManager)', () => {
    beforeEach(() => {
      window.playbackManager = {};
    });

    it('uses local item detection (DOM/global) rather than the server session', () => {
      window.NowPlayingItem = { Id: ITEM_B };
      JWP._bindInternal.maybeSendSetMedia();
      assert.equal(sent.length, 1);
      assert.equal(sent[0].payload.media_id, ITEM_B);
      delete window.NowPlayingItem;
    });
  });

  describe('isMediaSwitchPending safety timeout', () => {
    it('self-clears once past mediaSwitchPendingUntil, so the host is never wedged silent forever', () => {
      JWP.state.mediaSwitchPending = true;
      JWP.state.mediaSwitchPendingUntil = JWP.utils.nowMs() - 1; // already expired
      assert.equal(JWP._bindInternal.isMediaSwitchPending(), false);
      assert.equal(JWP.state.mediaSwitchPending, false);
    });

    it('stays pending before the timeout', () => {
      JWP.state.mediaSwitchPending = true;
      JWP.state.mediaSwitchPendingUntil = JWP.utils.nowMs() + 60000;
      assert.equal(JWP._bindInternal.isMediaSwitchPending(), true);
    });
  });

  describe('beginMediaResolution', () => {
    it('arms mediaSwitchPending for a host in a room', () => {
      JWP._bindInternal.beginMediaResolution();
      assert.equal(JWP.state.mediaSwitchPending, true);
      assert.ok(JWP.state.mediaSwitchPendingUntil > JWP.utils.nowMs());
    });

    it('does not arm it for a guest', () => {
      JWP.state.isHost = false;
      JWP._bindInternal.beginMediaResolution();
      assert.equal(JWP.state.mediaSwitchPending, false);
    });
  });
});
