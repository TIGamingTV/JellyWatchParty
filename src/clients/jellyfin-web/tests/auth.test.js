const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const JWP = require('./setup.js');

// setup.js provides JWP.state / JWP.utils; auth.js attaches fetchAuthToken and
// buildAuthHeader to JWP.actions.
require('../ws/auth.js');

// Stands in for jellyfin-web's ApiClient. Every accessor is a function, which
// is how the real one behaves (see src/apiclient.d.ts in jellyfin-web).
function fakeApiClient(overrides = {}) {
  const values = {
    appName: 'Jellyfin Web',
    deviceName: 'Firefox',
    deviceId: 'abc123',
    appVersion: '12.0.0',
    ...overrides
  };
  const client = {};
  for (const [name, value] of Object.entries(values)) {
    if (value === undefined) continue;
    client[name] = () => value;
  }
  return client;
}

// Pulls "Key" => value out of a MediaBrowser authorization header so the
// assertions read as intent rather than as string surgery.
function parseParts(header) {
  const parts = {};
  for (const match of header.slice('MediaBrowser '.length).split(', ')) {
    const eq = match.indexOf('=');
    parts[match.slice(0, eq)] = match.slice(eq + 1).replace(/^"|"$/g, '');
  }
  return parts;
}

describe('buildAuthHeader', () => {
  const { buildAuthHeader } = JWP.actions;

  it('uses the MediaBrowser scheme, which Jellyfin accepts with legacy auth off', () => {
    const header = buildAuthHeader(fakeApiClient(), 'tok');
    assert.ok(header.startsWith('MediaBrowser '), header);
  });

  it('carries the token and every descriptive part ApiClient exposes', () => {
    const parts = parseParts(buildAuthHeader(fakeApiClient(), 'tok'));
    assert.deepEqual(parts, {
      Client: 'Jellyfin Web',
      Device: 'Firefox',
      DeviceId: 'abc123',
      Version: '12.0.0',
      Token: 'tok'
    });
  });

  it('omits parts the ApiClient does not expose rather than sending them empty', () => {
    const parts = parseParts(buildAuthHeader(fakeApiClient({ deviceName: undefined }), 'tok'));
    assert.equal('Device' in parts, false);
    assert.equal(parts.Token, 'tok');
  });

  it('omits parts whose accessor returns nothing', () => {
    const parts = parseParts(buildAuthHeader(fakeApiClient({ appVersion: '' }), 'tok'));
    assert.equal('Version' in parts, false);
    assert.equal(parts.Client, 'Jellyfin Web');
  });

  it('survives an accessor that throws', () => {
    const client = fakeApiClient();
    client.deviceId = () => { throw new Error('nope'); };
    const parts = parseParts(buildAuthHeader(client, 'tok'));
    assert.equal('DeviceId' in parts, false);
    assert.equal(parts.Token, 'tok');
  });

  it('strips quotes so a hostile device name cannot break out of the header', () => {
    const parts = parseParts(buildAuthHeader(fakeApiClient({ deviceName: 'a"b' }), 'tok'));
    assert.equal(parts.Device, 'ab');
  });

  it('still produces a usable header when ApiClient exposes nothing at all', () => {
    assert.equal(buildAuthHeader({}, 'tok'), 'MediaBrowser Token="tok"');
  });
});
