const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

describe('escapeHtml', () => {
  const { escapeHtml } = JWP.utils;

  it('escapes special characters', () => {
    assert.equal(escapeHtml('<script>"a"&\'b\''), '&lt;script&gt;&quot;a&quot;&amp;&#39;b&#39;');
  });

  it('returns unchanged string without special chars', () => {
    assert.equal(escapeHtml('hello world'), 'hello world');
  });

  it('returns empty string for empty input', () => {
    assert.equal(escapeHtml(''), '');
  });

  it('returns empty string for non-string input', () => {
    assert.equal(escapeHtml(null), '');
    assert.equal(escapeHtml(undefined), '');
    assert.equal(escapeHtml(42), '');
  });
});

describe('suppress / shouldSend', () => {
  const { suppress, shouldSend } = JWP.utils;

  it('shouldSend returns false during suppression', () => {
    suppress(1000);
    assert.equal(shouldSend(), false);
  });

  it('shouldSend returns true after suppression expires', () => {
    // Set suppressUntil to the past so shouldSend() returns true
    JWP.state.suppressUntil = 0;
    assert.equal(shouldSend(), true);
  });
});
