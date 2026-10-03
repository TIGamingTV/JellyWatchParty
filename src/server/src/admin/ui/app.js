(() => {
  'use strict';

  const POLL_MS = 2000;
  const $ = (id) => document.getElementById(id);

  let pollTimer = null;
  let overview = null;
  let devices = null; // last api/jellyfin/sessions answer

  // --- tiny DOM helper: never uses innerHTML, so names can't inject markup.
  const el = (tag, props = {}, ...children) => {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
      if (v === undefined || v === null || v === false) continue;
      if (k === 'text') node.textContent = v;
      else if (k === 'className') node.className = v;
      else if (k.startsWith('on')) node.addEventListener(k.slice(2), v);
      else node.setAttribute(k, v === true ? '' : v);
    }
    for (const c of children.flat()) {
      if (c === null || c === undefined || c === false) continue;
      node.append(c instanceof Node ? c : document.createTextNode(String(c)));
    }
    return node;
  };

  // --- API ---------------------------------------------------------------
  const api = async (method, path, body) => {
    const opts = { method, credentials: 'same-origin', headers: { 'x-jwp-admin': '1' } };
    if (body !== undefined) {
      opts.headers['content-type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
    const res = await fetch(path, opts);
    let data = null;
    try { data = await res.json(); } catch (e) { /* empty body */ }
    if (res.status === 401 && path !== 'api/login') {
      showLogin();
      throw new Error('Your session expired. Please sign in again.');
    }
    if (!res.ok) throw new Error((data && data.error) || `Request failed (${res.status})`);
    return data;
  };

  const enc = encodeURIComponent;

  // --- banner ------------------------------------------------------------
  let bannerTimer = null;
  const banner = (text, kind = 'ok') => {
    const b = $('banner');
    b.textContent = text;
    b.className = `banner ${kind}`;
    clearTimeout(bannerTimer);
    bannerTimer = setTimeout(() => b.classList.add('hidden'), kind === 'error' ? 8000 : 3000);
  };

  // Runs an admin action, reports the outcome, refreshes the view.
  const act = async (label, fn) => {
    try {
      await fn();
      banner(label);
    } catch (e) {
      banner(e.message, 'error');
    }
    await refresh(true);
  };

  // --- formatting --------------------------------------------------------
  const fmtTime = (secs) => {
    if (typeof secs !== 'number' || !isFinite(secs)) return '-';
    const s = Math.max(0, Math.floor(secs));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, '0');
    return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
  };

  const fmtUptime = (secs) => {
    const d = Math.floor(secs / 86400);
    const h = Math.floor((secs % 86400) / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : `${m}m`;
  };

  const fmtDrift = (d) => (typeof d === 'number' ? `${d > 0 ? '+' : ''}${d.toFixed(1)}s` : '');

  const KIND_LABEL = { web: 'Watch Party', jellyfin: 'Jellyfin device', plugin_bridge: 'Plugin bridge' };

  const generatePassword = () => {
    const alphabet = 'abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789';
    const bytes = new Uint8Array(12);
    crypto.getRandomValues(bytes);
    return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('');
  };

  // --- views -------------------------------------------------------------
  const showLogin = () => {
    stopPolling();
    $('app-view').classList.add('hidden');
    $('logout').classList.add('hidden');
    $('server-info').textContent = '';
    $('login-view').classList.remove('hidden');
    $('login-user').focus();
  };

  const showApp = () => {
    $('login-view').classList.add('hidden');
    $('app-view').classList.remove('hidden');
    $('logout').classList.remove('hidden');
    startPolling();
  };

  // Who an admin can put into `roomId`: signed-in web clients and bridged
  // devices that are not already in it, and Jellyfin devices not yet
  // bridged anywhere.
  const candidates = (ov, roomId) => {
    const out = [];
    for (const c of ov.unassigned) {
      if (c.authenticated) out.push({ value: `c:${c.id}`, label: `${c.name} (no room)`, group: 'Watch Party clients' });
    }
    for (const r of ov.rooms) {
      if (r.id === roomId) continue;
      for (const m of r.members) {
        out.push({
          value: `c:${m.id}`,
          label: `${m.name} (in ${r.name})`,
          group: m.kind === 'jellyfin' ? 'Jellyfin devices' : 'Watch Party clients'
        });
      }
    }
    if (devices && devices.enabled) {
      for (const d of devices.sessions) {
        if (d.bridged_as) continue;
        const playing = d.now_playing ? ` - ${d.now_playing.name || 'playing'}` : '';
        out.push({
          value: `j:${d.id}`,
          label: `${d.user_name || 'Jellyfin'} - ${d.device_name || d.client} (${d.client})${playing}${d.remote_control ? '' : ' [host only]'}`,
          group: 'Jellyfin devices'
        });
      }
    }
    return out;
  };

  const groupedOptions = (items) => {
    const groups = new Map();
    for (const i of items) {
      if (!groups.has(i.group)) groups.set(i.group, []);
      groups.get(i.group).push(el('option', { value: i.value, text: i.label }));
    }
    return [...groups].map(([label, opts]) => el('optgroup', { label }, opts));
  };

  // Adds `value` ("c:<client id>" or "j:<jellyfin session id>") to a room,
  // as host or receiver.
  const addToRoom = (roomId, value, role) => {
    const [kind, id] = [value.slice(0, 1), value.slice(2)];
    if (kind === 'j') {
      return act(role === 'host' ? 'Device added as host' : 'Device added',
        () => api('POST', `api/rooms/${enc(roomId)}/members`, { jellyfin_session_id: id, role }));
    }
    return act(role === 'host' ? 'Added as host' : 'Added', async () => {
      await api('POST', `api/rooms/${enc(roomId)}/members`, { client_id: id });
      if (role === 'host') await api('PUT', `api/rooms/${enc(roomId)}/host`, { member: id });
    });
  };

  const roleSelect = (label) => el('select', { 'aria-label': label },
    el('option', { value: 'receiver', text: 'as receiver' }),
    el('option', { value: 'host', text: 'as host' }));

  const memberRow = (room, m) => {
    const actions = el('td', { className: 'actions' });
    if (!m.is_host) {
      actions.append(el('button', {
        className: 'btn btn-ghost btn-small',
        type: 'button',
        text: 'Make host',
        onclick: () => act(`${m.name} is now the host of ${room.name}`,
          () => api('PUT', `api/rooms/${enc(room.id)}/host`, { member: m.id }))
      }));
    }
    actions.append(el('button', {
      className: 'btn btn-danger btn-small',
      type: 'button',
      text: 'Remove',
      onclick: () => {
        if (!confirm(`Remove ${m.name} from ${room.name}?`)) return;
        act(`Removed ${m.name}`, () => api('DELETE', `api/rooms/${enc(room.id)}/members/${enc(m.id)}`));
      }
    }));

    const status = m.status || 'unknown';
    const detail = [m.device ? m.device : null, m.detail ? m.detail : null].filter(Boolean).join(' - ');
    return el('tr', {},
      el('td', {},
        el('span', { className: `dot${m.connected ? '' : ' dot-off'}`, title: m.connected ? 'Connected' : 'Disconnected' }),
        m.name,
        detail ? el('div', { className: 'muted small', text: detail }) : null),
      el('td', {},
        m.is_host ? el('span', { className: 'badge badge-host', text: 'Host' }) : el('span', { className: 'badge', text: 'Receiver' }),
        el('span', { className: `badge${m.kind === 'jellyfin' ? ' badge-jellyfin' : ''}`, text: KIND_LABEL[m.kind] || m.kind })),
      el('td', {},
        el('span', { className: `status status-${status}`, text: status }),
        m.drift !== undefined && m.drift !== null ? el('span', { className: 'muted small', text: ` ${fmtDrift(m.drift)}` }) : null),
      actions);
  };

  const roomCard = (ov, room) => {
    const head = el('div', { className: 'room-head' },
      el('h3', { text: room.name }),
      room.admin_created ? el('span', { className: 'badge badge-admin', text: 'Admin group' }) : el('span', { className: 'badge', text: 'User room' }),
      room.has_password ? el('span', { className: 'badge', text: 'Password' }) : el('span', { className: 'badge', text: 'Open' }),
      el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: 'Rename',
        onclick: () => {
          const name = prompt('New name', room.name);
          if (name === null || !name.trim()) return;
          act('Room renamed', () => api('PATCH', `api/rooms/${enc(room.id)}`, { name }));
        }
      }),
      el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: room.has_password ? 'Change password' : 'Set password',
        onclick: () => {
          const pw = prompt('New password (members already in the room stay)', generatePassword());
          if (pw === null || !pw) return;
          act(`Password set to: ${pw}`, () => api('PATCH', `api/rooms/${enc(room.id)}`, { password: pw }));
        }
      }),
      room.has_password ? el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: 'Remove password',
        onclick: () => act('Password removed', () => api('PATCH', `api/rooms/${enc(room.id)}`, { password: null }))
      }) : null,
      el('button', {
        className: 'btn btn-danger btn-small', type: 'button', text: 'Close',
        onclick: () => {
          if (!confirm(`Close ${room.name}? Everyone in it is sent back to the lobby.`)) return;
          act('Room closed', () => api('DELETE', `api/rooms/${enc(room.id)}`));
        }
      }));

    const meta = el('div', { className: 'room-meta' },
      el('span', { text: `${room.members.length} member${room.members.length === 1 ? '' : 's'}` }),
      room.host_id ? null : el('span', { text: 'No host yet' }),
      el('span', { text: `${room.play_state} at ${fmtTime(room.position)}${room.pending_play ? ' (waiting for everyone)' : ''}` }),
      room.media_name ? el('span', { text: room.media_name })
        : el('span', { text: room.media_id ? `Item ${room.media_id.slice(0, 8)}...` : 'Nothing playing yet' }));

    const table = room.members.length
      ? el('table', {},
        el('thead', {}, el('tr', {}, el('th', { text: 'Member' }), el('th', { text: 'Role' }), el('th', { text: 'Sync' }), el('th', {}))),
        el('tbody', {}, room.members.map((m) => memberRow(room, m))))
      : el('p', { className: 'empty', text: 'Nobody here yet.' });

    const options = candidates(ov, room.id);
    const select = el('select', { 'aria-label': `Who to add to ${room.name}` },
      el('option', { value: '', text: options.length ? 'Add someone...' : 'Nobody else to add' }),
      groupedOptions(options));
    const role = roleSelect(`Role in ${room.name}`);
    const addRow = el('div', { className: 'row add-row' }, select, role,
      el('button', {
        className: 'btn btn-small', type: 'button', text: 'Add',
        onclick: () => { if (select.value) addToRoom(room.id, select.value, role.value); }
      }));

    return el('div', { className: 'card' }, head, meta, table, addRow);
  };

  const renderRooms = (ov) => {
    const list = $('rooms');
    list.replaceChildren();
    $('room-count').textContent = `(${ov.rooms.length})`;
    if (!ov.rooms.length) {
      list.append(el('p', { className: 'empty', text: 'No rooms right now.' }));
      return;
    }
    for (const room of ov.rooms) list.append(roomCard(ov, room));
  };

  const renderUnassigned = (ov) => {
    const box = $('unassigned');
    box.replaceChildren();
    if (!ov.unassigned.length) {
      box.append(el('p', { className: 'empty', text: 'Everyone connected is in a room.' }));
      return;
    }
    const rows = ov.unassigned.map((c) => {
      const select = el('select', { 'aria-label': `Room for ${c.name}` },
        el('option', { value: '', text: ov.rooms.length ? 'Choose a room...' : 'No rooms' }),
        ov.rooms.map((r) => el('option', { value: r.id, text: r.name })));
      const cell = el('td', { className: 'actions' });
      if (c.authenticated) {
        const role = roleSelect(`Role for ${c.name}`);
        cell.append(select, role, el('button', {
          className: 'btn btn-small', type: 'button', text: 'Add',
          onclick: () => { if (select.value) addToRoom(select.value, `c:${c.id}`, role.value); }
        }));
      } else {
        cell.append(el('span', { className: 'muted small', text: 'Not signed in' }));
      }
      return el('tr', {},
        el('td', {}, el('span', { className: `dot${c.connected ? '' : ' dot-off'}` }), c.name),
        el('td', {}, el('span', { className: 'badge', text: KIND_LABEL[c.kind] || c.kind })),
        cell);
    });
    box.append(el('table', {}, el('tbody', {}, rows)));
  };

  // Where a bridged device is, from the overview.
  const findMember = (ov, clientId) => {
    for (const r of ov.rooms) {
      const m = r.members.find((x) => x.id === clientId);
      if (m) return { room: r, member: m };
    }
    return null;
  };

  const renderDevices = (ov) => {
    const section = $('devices-section');
    const box = $('devices');
    const jf = (ov.server && ov.server.jellyfin) || {};
    section.classList.remove('hidden');
    box.replaceChildren();
    if (!jf.enabled) {
      box.append(el('p', { className: 'muted', text: jf.reason || 'Jellyfin devices are not set up.' }),
        el('p', { className: 'muted small', text: 'With JELLYFIN_URL and JELLYFIN_API_KEY set, you can put TV apps and other Jellyfin clients that can\'t show the Watch Party panel into rooms from here.' }));
      return;
    }
    if (devices && devices.error) {
      box.append(el('p', { className: 'error', text: devices.error }));
    }
    const list = (devices && devices.sessions) || [];
    if (!list.length) {
      box.append(el('p', { className: 'empty', text: 'No other Jellyfin clients are active. Open a Jellyfin app on the device and it shows up here.' }));
      return;
    }
    const rows = list.map((d) => {
      const where = d.bridged_as ? findMember(ov, d.bridged_as) : null;
      const actions = el('td', { className: 'actions' });
      if (where) {
        actions.append(el('span', { className: 'muted small', text: `${where.member.is_host ? 'Host' : 'Receiver'} in ${where.room.name} ` }),
          el('button', {
            className: 'btn btn-danger btn-small', type: 'button', text: 'Remove',
            onclick: () => act('Device removed', () => api('DELETE', `api/rooms/${enc(where.room.id)}/members/${enc(where.member.id)}`))
          }));
      } else {
        const room = el('select', { 'aria-label': `Room for ${d.device_name}` },
          el('option', { value: '', text: ov.rooms.length ? 'Choose a room...' : 'Create a group first' }),
          ov.rooms.map((r) => el('option', { value: r.id, text: r.name })));
        const role = roleSelect(`Role for ${d.device_name}`);
        if (!d.remote_control) {
          role.value = 'host';
          role.querySelector('option[value="receiver"]').disabled = true;
        }
        actions.append(room, role, el('button', {
          className: 'btn btn-small', type: 'button', text: 'Add',
          onclick: () => { if (room.value) addToRoom(room.value, `j:${d.id}`, role.value); }
        }));
      }
      const playing = d.now_playing
        ? `${d.paused ? 'Paused' : 'Playing'}: ${d.now_playing.name || d.now_playing.id.slice(0, 8)} at ${fmtTime(d.position)}`
        : 'Nothing playing';
      return el('tr', {},
        el('td', {}, `${d.user_name || '-'}`, el('div', { className: 'muted small', text: `${d.device_name} - ${d.client}` })),
        el('td', {}, playing),
        el('td', {}, d.remote_control
          ? el('span', { className: 'badge', text: 'Host or receiver' })
          : el('span', { className: 'badge', title: 'This app does not accept remote control from Jellyfin', text: 'Host only' })),
        actions);
    });
    box.append(el('table', {},
      el('thead', {}, el('tr', {}, el('th', { text: 'User / device' }), el('th', { text: 'Now' }), el('th', { text: 'Can be' }), el('th', {}))),
      el('tbody', {}, rows)));
  };

  const renderServerInfo = (ov) => {
    const s = ov.server || {};
    const jf = s.jellyfin || {};
    const parts = [`v${s.version}`, `up ${fmtUptime(s.uptime_secs || 0)}`,
      `JWT ${s.auth_enabled ? 'on' : 'off'}`, `${ov.totals.clients} connected`,
      `Jellyfin ${!jf.enabled ? 'off' : jf.error ? 'unreachable' : 'connected'}`];
    $('server-info').textContent = parts.join(' | ');
  };

  // Don't rebuild the DOM under the admin's cursor: a re-render would reset
  // an open <select> or a half-typed field.
  const isInteracting = () => {
    const a = document.activeElement;
    return a && $('app-view').contains(a) && (a.tagName === 'SELECT' || a.tagName === 'INPUT');
  };

  const render = (ov) => {
    renderServerInfo(ov);
    renderRooms(ov);
    renderUnassigned(ov);
    renderDevices(ov);
  };

  const refresh = async (force = false) => {
    try {
      const [ov, dev] = await Promise.all([
        api('GET', 'api/overview'),
        api('GET', 'api/jellyfin/sessions').catch(() => null)
      ]);
      overview = ov;
      devices = dev;
      if (force || !isInteracting()) render(overview);
    } catch (e) {
      if (!$('app-view').classList.contains('hidden')) banner(e.message, 'error');
    }
  };

  const startPolling = () => {
    stopPolling();
    refresh(true);
    pollTimer = setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, POLL_MS);
  };

  const stopPolling = () => {
    if (pollTimer) clearInterval(pollTimer);
    pollTimer = null;
  };

  // --- wiring ------------------------------------------------------------
  const init = async () => {
    $('login-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      $('login-error').textContent = '';
      try {
        await api('POST', 'api/login', { username: $('login-user').value, password: $('login-pass').value });
        $('login-pass').value = '';
        showApp();
      } catch (err) {
        $('login-error').textContent = err.message;
      }
    });

    $('logout').addEventListener('click', async () => {
      try { await api('POST', 'api/logout'); } catch (e) { /* ignore */ }
      showLogin();
    });

    $('create-gen').addEventListener('click', () => { $('create-pass').value = generatePassword(); });

    $('create-form').addEventListener('submit', (e) => {
      e.preventDefault();
      const name = $('create-name').value.trim();
      const password = $('create-pass').value;
      if (!name) return;
      act(password ? `Group created. Password: ${password}` : 'Group created', async () => {
        await api('POST', 'api/rooms', { name, password });
        $('create-name').value = '';
        $('create-pass').value = '';
      });
    });

    try {
      await api('GET', 'api/me');
      showApp();
    } catch (e) {
      showLogin();
    }
  };

  document.addEventListener('DOMContentLoaded', init);
})();
