const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// playback/sync.js needs utils.getVideo/isVideoReady (utils/video.js) and
// utils.log (utils/log.js); it tolerates a missing JWP.ui (guarded with
// `JWP.ui &&` everywhere it's used).
require('../utils/video.js');
require('../utils/log.js');
require('../playback/sync.js');

const { DRIFT_CORRECTION_ENTER_SEC, DRIFT_CORRECTION_EXIT_SEC, DRIFT_SOFT_MAX_SEC, PLAYBACK_RATE_MIN, PLAYBACK_RATE_MAX } = JWP.constants;

const makeVideo = (currentTime) => ({
  currentTime,
  paused: false,
  playbackRate: 1,
  readyState: 4,
  networkState: 1,
  seeking: false
});

describe('playback/sync syncLoop drift correction', () => {
  let video;

  beforeEach(() => {
    // syncLoop reaches the drift-correction math only once these gates pass:
    // in a room, not the host, has a time-sync baseline, host is playing,
    // not buffering, and the local video isn't paused.
    video = makeVideo(0);
    document.querySelector = (sel) => (sel === 'video' ? video : null);

    JWP.state.inRoom = true;
    JWP.state.isHost = false;
    JWP.state.isBuffering = false;
    JWP.state.lastSyncPlayState = 'playing';
    JWP.state.isDriftCorrecting = false;
    JWP.state.isInitialSync = false;
    JWP.state.syncCooldownUntil = 0;
    JWP.state.syncStatus = 'unknown';
    JWP.state.serverOffsetMs = 0;

    // Baseline: host was at position 10.0 exactly "now" (no elapsed time),
    // so `expected` position equals lastSyncPosition and drift is driven
    // purely by where we place video.currentTime.
    JWP.state.lastSyncServerTs = JWP.utils.getServerNow();
    JWP.state.lastSyncPosition = 10.0;
  });

  it('stays quiet (1x) when drift is under the enter threshold', () => {
    video.currentTime = 10.0 - (DRIFT_CORRECTION_ENTER_SEC - 0.05); // small drift
    JWP.playback.syncLoop();
    assert.equal(video.playbackRate, 1);
    assert.equal(JWP.state.isDriftCorrecting, false);
    assert.equal(JWP.state.syncStatus, 'synced');
  });

  it('speeds up when behind (positive drift) past the enter threshold', () => {
    video.currentTime = 10.0 - 1.0; // ~1s behind -> drift ~= +1.0
    JWP.playback.syncLoop();
    // Real wall-clock time elapses between the beforeEach baseline and this
    // call, so drift is ~1.0s but not bit-exact — compare with tolerance.
    const expectedRate = Math.min(Math.max(1 + Math.sqrt(1.0) * 0.5, PLAYBACK_RATE_MIN), PLAYBACK_RATE_MAX);
    assert.ok(Math.abs(video.playbackRate - expectedRate) < 0.01);
    assert.ok(video.playbackRate > 1, 'should speed up to catch up when behind');
    assert.equal(JWP.state.isDriftCorrecting, true);
    assert.equal(JWP.state.syncStatus, 'syncing');
  });

  it('slows down when ahead (negative drift) past the enter threshold', () => {
    video.currentTime = 10.0 + 1.0; // 1s ahead -> drift = -1.0
    JWP.playback.syncLoop();
    assert.ok(video.playbackRate < 1, 'should slow down when ahead of host');
  });

  it('clamps the correction rate to PLAYBACK_RATE_MIN/MAX for large drift', () => {
    video.currentTime = 10.0 + (DRIFT_SOFT_MAX_SEC - 0.01); // large-but-under-hard-seek negative drift
    JWP.playback.syncLoop();
    assert.ok(video.playbackRate >= PLAYBACK_RATE_MIN);
    assert.ok(video.playbackRate <= PLAYBACK_RATE_MAX);
  });

  it('hard-seeks and resets rate once drift reaches the soft max', () => {
    video.currentTime = 10.0 - DRIFT_SOFT_MAX_SEC - 1; // well past the hard-seek threshold
    JWP.playback.syncLoop();
    // Hard-seeks to "expected", which includes whatever wall-clock time has
    // elapsed since the beforeEach baseline — allow a small tolerance.
    assert.ok(Math.abs(video.currentTime - 10.0) < 0.05);
    assert.equal(video.playbackRate, 1);
    assert.equal(JWP.state.isDriftCorrecting, false);
  });

  it('hysteresis: keeps correcting between the exit and enter thresholds once started', () => {
    // Cross the enter threshold to start a correction burst.
    video.currentTime = 10.0 - 1.0;
    JWP.playback.syncLoop();
    assert.equal(JWP.state.isDriftCorrecting, true);

    // Drift falls back to a level between EXIT and ENTER — a fresh call
    // starting from quiet wouldn't trigger correction here, but since a
    // burst is already active it should keep correcting, not go quiet.
    const midDrift = (DRIFT_CORRECTION_EXIT_SEC + DRIFT_CORRECTION_ENTER_SEC) / 2;
    video.currentTime = 10.0 - midDrift;
    JWP.playback.syncLoop();
    assert.equal(JWP.state.isDriftCorrecting, true, 'should still be correcting inside the hysteresis band');
    assert.notEqual(video.playbackRate, 1);
  });

  it('hysteresis: snaps back to 1x once drift falls under the exit threshold', () => {
    video.currentTime = 10.0 - 1.0;
    JWP.playback.syncLoop();
    assert.equal(JWP.state.isDriftCorrecting, true);

    video.currentTime = 10.0 - (DRIFT_CORRECTION_EXIT_SEC - 0.02);
    JWP.playback.syncLoop();
    assert.equal(JWP.state.isDriftCorrecting, false);
    assert.equal(video.playbackRate, 1);
    assert.equal(JWP.state.syncStatus, 'synced');
  });

  it('does nothing when the host (no follower sync applies)', () => {
    JWP.state.isHost = true;
    video.currentTime = 10.0 - 5.0;
    JWP.playback.syncLoop();
    assert.equal(video.playbackRate, 1);
    assert.equal(JWP.state.isDriftCorrecting, false);
  });

  it('does nothing when not in a room', () => {
    JWP.state.inRoom = false;
    video.currentTime = 10.0 - 5.0;
    JWP.playback.syncLoop();
    assert.equal(video.playbackRate, 1);
  });
});
