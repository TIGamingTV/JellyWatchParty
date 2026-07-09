(() => {
  const JWP = window.JellyWatchParty = window.JellyWatchParty || {};
  const utils = JWP.utils = JWP.utils || {};

  // Checks a Session Server URL for common misconfigurations and returns
  // human-readable warnings. Does not reject anything - an empty array
  // means no issues were found. Empty input always passes (it means
  // "use the auto-detected default").
  const validateWsUrl = (url) => {
    const warnings = [];
    if (!url) return warnings;

    let parsed;
    try {
      parsed = new URL(url);
    } catch (e) {
      return ['Not a valid URL.'];
    }

    if (parsed.protocol !== 'ws:' && parsed.protocol !== 'wss:') {
      warnings.push(`Should use ws:// or wss:// (got ${parsed.protocol})`);
    }

    if (parsed.protocol === 'ws:' && window.location.protocol === 'https:') {
      warnings.push('Using insecure ws:// on an https:// page — the browser will likely block this connection (mixed content).');
    }

    if (parsed.hostname && !parsed.hostname.includes('.') && parsed.hostname !== 'localhost' && !/^[\d.]+$/.test(parsed.hostname)) {
      warnings.push(`'${parsed.hostname}' looks like an internal/Docker hostname — it may not be reachable from your browser.`);
    }

    return warnings;
  };

  Object.assign(utils, { validateWsUrl });
})();
