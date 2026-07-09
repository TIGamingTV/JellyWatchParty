const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

describe('validateWsUrl', () => {
  const { validateWsUrl } = JWP.utils;

  it('returns no warnings for an empty value', () => {
    assert.deepEqual(validateWsUrl(''), []);
  });

  it('returns no warnings for a valid wss URL', () => {
    assert.deepEqual(validateWsUrl('wss://party.example.com/ws'), []);
  });

  it('returns no warnings for localhost', () => {
    assert.deepEqual(validateWsUrl('wss://localhost:3000/ws'), []);
  });

  it('returns no warnings for an IP address host', () => {
    assert.deepEqual(validateWsUrl('wss://192.168.1.5:3000/ws'), []);
  });

  it('warns on a malformed URL', () => {
    const warnings = validateWsUrl('not a url');
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /not a valid url/i);
  });

  it('warns on the wrong scheme', () => {
    const warnings = validateWsUrl('http://party.example.com/ws');
    assert.ok(warnings.some((w) => /ws:\/\/ or wss:\/\//.test(w)));
  });

  it('warns on insecure ws:// used on an https page (mixed content, per setup.js)', () => {
    const warnings = validateWsUrl('ws://party.example.com/ws');
    assert.ok(warnings.some((w) => /mixed content/i.test(w)));
  });

  it('warns on a bare Docker/Compose-style hostname', () => {
    const warnings = validateWsUrl('ws://jwp-session:3000/ws');
    assert.ok(warnings.some((w) => /internal\/Docker hostname/.test(w)));
  });
});
