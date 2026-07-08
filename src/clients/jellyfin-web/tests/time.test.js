const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

describe('nowMs', () => {
  const { nowMs } = JWP.utils;

  it('returns a reasonable timestamp', () => {
    const ts = nowMs();
    // After 2020-01-01
    assert.ok(ts > 1577836800000, 'should be after 2020');
    // Before 2100-01-01
    assert.ok(ts < 4102444800000, 'should be before 2100');
  });
});

describe('getServerNow', () => {
  const { nowMs, getServerNow } = JWP.utils;

  it('applies server offset', () => {
    const origOffset = JWP.state.serverOffsetMs;
    JWP.state.serverOffsetMs = 1000;
    const serverNow = getServerNow();
    const localNow = nowMs();
    // Server time should be roughly 1000ms ahead of local time
    assert.ok(serverNow > localNow + 900, `serverNow (${serverNow}) should be > localNow+900 (${localNow + 900})`);
    JWP.state.serverOffsetMs = origOffset;
  });

  it('returns ~local time with zero offset', () => {
    const origOffset = JWP.state.serverOffsetMs;
    JWP.state.serverOffsetMs = 0;
    const serverNow = getServerNow();
    const localNow = nowMs();
    assert.ok(Math.abs(serverNow - localNow) < 50, 'should be close to local time');
    JWP.state.serverOffsetMs = origOffset;
  });
});

describe('adjustedPosition', () => {
  const { adjustedPosition } = JWP.utils;
  const { SYNC_LEAD_MS } = JWP.constants;

  it('advances position by elapsed time plus lead', () => {
    const origOffset = JWP.state.serverOffsetMs;
    JWP.state.serverOffsetMs = 0;
    const serverTs = Date.now() - 2000; // 2 seconds ago
    const pos = adjustedPosition(10.0, serverTs);
    // Should be ~10 + 2 + SYNC_LEAD_MS/1000
    const expected = 10.0 + 2.0 + SYNC_LEAD_MS / 1000;
    assert.ok(Math.abs(pos - expected) < 0.2, `pos (${pos}) should be close to ${expected}`);
    JWP.state.serverOffsetMs = origOffset;
  });

  it('does not go below original position when serverTs is now', () => {
    const origOffset = JWP.state.serverOffsetMs;
    JWP.state.serverOffsetMs = 0;
    const serverTs = Date.now();
    const pos = adjustedPosition(5.0, serverTs);
    // elapsed ~0, so position should be ~5 + SYNC_LEAD_MS/1000
    assert.ok(pos >= 5.0, `pos (${pos}) should be >= 5.0`);
    JWP.state.serverOffsetMs = origOffset;
  });
});
