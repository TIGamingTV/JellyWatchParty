(() => {
  'use strict';

  const POLL_MS = 2000;
  const $ = (id) => document.getElementById(id);

  let pollTimer = null;
  let overview = null;
  let devices = null; // last api/jellyfin/sessions answer
  let chat = null; // last api/integrations answer
  let users = null; // last api/users answer
  let audit = null; // last api/audit answer
  let formDirty = false; // unsaved edits in the Discord settings form

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
    for (const c of children.flat(Infinity)) {
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

  // --- toasts ------------------------------------------------------------
  // `secret` (a password) is shown in a box to copy, and the toast stays
  // until closed: the password can't be looked up again later.
  const toast = (text, { kind = 'ok', secret = null } = {}) => {
    const close = el('button', { className: 'btn btn-ghost btn-small', type: 'button', text: secret ? 'Done' : 'Close' });
    const body = el('div', { className: 'toast-text' }, text,
      secret ? el('div', {}, el('code', { text: secret })) : null);
    const copy = secret ? el('button', {
      className: 'btn btn-small', type: 'button', text: 'Copy',
      onclick: async () => {
        try {
          await navigator.clipboard.writeText(secret);
          copy.textContent = 'Copied';
        } catch (e) {
          copy.textContent = 'Select it';
        }
      }
    }) : null;
    const t = el('div', { className: `toast${kind === 'error' ? ' error' : ''}`, role: kind === 'error' ? 'alert' : 'status' },
      body, copy, close);
    close.addEventListener('click', () => t.remove());
    $('toasts').append(t);
    if (!secret) setTimeout(() => t.remove(), kind === 'error' ? 8000 : 3500);
  };

  // --- dialog (instead of prompt/confirm) ---------------------------------
  const ask = ({ title, message = '', input = null, value = '', generate = false, okLabel = 'OK', danger = false }) =>
    new Promise((resolve) => {
      const dlg = $('dialog');
      $('dialog-title').textContent = title;
      $('dialog-message').textContent = message;
      $('dialog-message').classList.toggle('hidden', !message);
      $('dialog-field').classList.toggle('hidden', input === null);
      $('dialog-label').textContent = input || '';
      $('dialog-input').value = value;
      $('dialog-input').required = input !== null;
      $('dialog-gen').classList.toggle('hidden', !generate);
      $('dialog-ok').textContent = okLabel;
      $('dialog-ok').className = danger ? 'btn btn-danger-solid' : 'btn';
      const done = () => {
        dlg.removeEventListener('close', done);
        if (dlg.returnValue !== 'ok') return resolve(null);
        resolve(input === null ? true : $('dialog-input').value.trim());
      };
      dlg.returnValue = '';
      dlg.addEventListener('close', done);
      dlg.showModal();
      if (input !== null) $('dialog-input').select();
    });

  // Runs an admin action, reports the outcome, refreshes the view.
  const act = async (label, fn, opts = {}) => {
    // Hand focus back so the next poll may re-render the lists.
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
    try {
      await fn();
      toast(label, opts);
    } catch (e) {
      toast(e.message, { kind: 'error' });
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

  const fmtDrift = (d) => `${d > 0 ? '+' : ''}${d.toFixed(1)}s`;

  const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

  const fmtAgo = (ms) => {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return `${Math.floor(s / 86400)} d ago`;
  };

  const fmtClock = (ms) => new Date(ms).toLocaleString([], { dateStyle: 'short', timeStyle: 'medium' });

  const KIND_BADGE = {
    web: ['Watch Party', 'badge'],
    plugin_bridge: ['Plugin bridge', 'badge badge-plugin'],
    jellyfin: ['Jellyfin device', 'badge badge-jellyfin']
  };

  // Member statuses as people read them.
  const STATUS = {
    synced: ['In sync', 'good'],
    playing: ['Playing', 'good'],
    syncing: ['Catching up', 'warn'],
    buffering: ['Buffering', 'warn'],
    loading: ['Loading', 'warn'],
    paused: ['Paused', 'muted'],
    idle: ['Not watching', 'muted'],
    unknown: ['No status yet', 'muted'],
    offline: ['Offline', 'bad'],
    error: ['Problem', 'bad']
  };

  const pill = (label, tone) => el('span', { className: `pill pill-${tone}`, text: label });

  const statusPill = (status) => {
    const [label, tone] = STATUS[status] || [status, 'muted'];
    return pill(label, tone);
  };

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
    $('server-info').replaceChildren();
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
      if (c.authenticated) out.push({ value: `c:${c.id}`, label: `${c.name} (no room)`, group: 'Watch Party' });
    }
    for (const r of ov.rooms) {
      if (r.id === roomId) continue;
      for (const m of r.members) {
        // A plugin bridge belongs to the room its user picked in the panel.
        if (m.kind === 'plugin_bridge') continue;
        out.push({
          value: `c:${m.id}`,
          label: `${m.name} (in ${r.name})`,
          group: m.kind === 'jellyfin' ? 'Jellyfin devices' : 'Watch Party'
        });
      }
    }
    if (devices && devices.enabled) {
      for (const d of devices.sessions) {
        if (d.bridged_as) continue;
        const what = d.now_playing ? ` - ${d.now_playing.name || 'playing'}` : '';
        out.push({
          value: `j:${d.id}`,
          label: `${d.user_name || 'Jellyfin'}: ${d.device_name || d.client} (${d.client})${what}${d.remote_control ? '' : ' - host only'}`,
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
      return act(role === 'host' ? 'Device added as host' : 'Device added as receiver',
        () => api('POST', `api/rooms/${enc(roomId)}/members`, { jellyfin_session_id: id, role }));
    }
    return act(role === 'host' ? 'Added as host' : 'Added', async () => {
      await api('POST', `api/rooms/${enc(roomId)}/members`, { client_id: id });
      if (role === 'host') await api('PUT', `api/rooms/${enc(roomId)}/host`, { member: id });
    });
  };

  const roleSelect = (label, key) => el('select', { 'aria-label': label, 'data-key': key },
    el('option', { value: 'receiver', text: 'as receiver' }),
    el('option', { value: 'host', text: 'as host' }));

  // A "pick something, then Add" control. Disabled when there's nothing to pick.
  const addControls = (select, role, onAdd) => {
    const add = el('button', { className: 'btn btn-small', type: 'button', text: 'Add' });
    const sync = () => { add.disabled = !select.value; };
    const empty = select.options.length <= 1;
    select.disabled = empty;
    role.disabled = empty;
    select.addEventListener('change', sync);
    add.addEventListener('click', () => { if (select.value) onAdd(select.value, role.value); });
    sync();
    return { add, sync };
  };

  const memberRow = (room, m) => {
    const actions = el('td', { className: 'actions' });
    if (!m.is_host) {
      actions.append(el('button', {
        className: 'btn btn-ghost btn-small',
        type: 'button',
        text: 'Make host',
        title: 'Everyone else follows this member',
        onclick: () => act(`${m.name} is now the host`,
          () => api('PUT', `api/rooms/${enc(room.id)}/host`, { member: m.id }))
      }));
    }
    actions.append(el('button', {
      className: 'btn btn-danger btn-small',
      type: 'button',
      text: 'Remove',
      onclick: async () => {
        const ok = await ask({
          title: `Remove ${m.name}?`,
          message: m.kind === 'jellyfin'
            ? 'The device stops being controlled and leaves the room.'
            : `They go back to the lobby. They can join ${room.name} again${room.has_password ? ' with the password' : ''}.`,
          okLabel: 'Remove',
          danger: true
        });
        if (ok) act(`Removed ${m.name}`, () => api('DELETE', `api/rooms/${enc(room.id)}/members/${enc(m.id)}`));
      }
    }));

    const [kindLabel, kindClass] = KIND_BADGE[m.kind] || [m.kind, 'badge'];
    const problem = m.status === 'error' || m.status === 'offline';
    return el('tr', {},
      el('td', {},
        el('div', { className: 'name' },
          el('span', { className: `dot${m.connected ? '' : ' dot-off'}`, title: m.connected ? 'Connected' : 'Disconnected' }),
          m.name),
        m.device ? el('div', { className: 'sub', text: m.device }) : null,
        m.detail ? el('div', { className: `sub${problem ? ' problem' : ''}`, text: m.detail }) : null),
      el('td', {},
        m.is_host ? el('span', { className: 'badge badge-host', text: 'Host' }) : el('span', { className: 'badge', text: 'Receiver' }),
        el('span', { className: kindClass, text: kindLabel })),
      el('td', {},
        statusPill(m.status || 'unknown'),
        typeof m.drift === 'number' ? el('span', { className: 'drift', title: 'Ahead (+) or behind (-) the host', text: fmtDrift(m.drift) }) : null),
      actions);
  };

  const playbackPill = (room) => {
    if (!room.media_id) return pill('Nothing playing yet', 'muted');
    if (room.pending_play) return pill('Waiting for everyone', 'warn');
    const label = `${room.play_state === 'playing' ? 'Playing' : 'Paused'} at ${fmtTime(room.position)}`;
    return pill(label, room.play_state === 'playing' ? 'good' : 'muted');
  };

  const roomCard = (ov, room) => {
    const title = el('div', { className: 'room-title' },
      el('h3', { text: room.name }),
      room.admin_created ? el('span', { className: 'badge badge-admin', text: 'Group' }) : el('span', { className: 'badge', text: 'User room' }),
      el('span', { className: 'badge', text: room.has_password ? 'Password' : 'Open' }));

    const actions = el('div', { className: 'room-actions' },
      el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: 'Rename',
        onclick: async () => {
          const name = await ask({ title: 'Rename room', input: 'Name', value: room.name, okLabel: 'Rename' });
          if (name) act('Room renamed', () => api('PATCH', `api/rooms/${enc(room.id)}`, { name }));
        }
      }),
      el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: room.has_password ? 'Change password' : 'Set password',
        onclick: async () => {
          const pw = await ask({
            title: room.has_password ? 'Change password' : 'Set password',
            message: 'People already in the room stay. New people need this password.',
            input: 'Password', value: generatePassword(), generate: true, okLabel: 'Save'
          });
          if (pw) act('New password for the room:', () => api('PATCH', `api/rooms/${enc(room.id)}`, { password: pw }), { secret: pw });
        }
      }),
      room.has_password ? el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: 'Remove password',
        onclick: async () => {
          const ok = await ask({ title: 'Remove the password?', message: 'Anyone will be able to join this room.', okLabel: 'Remove password' });
          if (ok) act('Password removed', () => api('PATCH', `api/rooms/${enc(room.id)}`, { password: null }));
        }
      }) : null,
      el('button', {
        className: 'btn btn-danger btn-small', type: 'button', text: 'Close',
        onclick: async () => {
          const ok = await ask({
            title: `Close ${room.name}?`,
            message: 'Everyone in it is sent back to the lobby, and devices stop being controlled.',
            okLabel: 'Close room',
            danger: true
          });
          if (ok) act('Room closed', () => api('DELETE', `api/rooms/${enc(room.id)}`));
        }
      }));

    const meta = el('div', { className: 'room-meta' },
      playbackPill(room),
      room.media_name ? el('span', { text: room.media_name }) : null,
      el('span', { text: plural(room.members.length, 'member') }),
      room.host_id ? null : pill('No host yet', 'warn'));

    const body = el('div', { className: 'room-body' },
      room.members.length
        ? el('div', { className: 'table-wrap' }, el('table', { className: 'stack' },
          el('thead', {}, el('tr', {}, el('th', { text: 'Member' }), el('th', { text: 'Role' }), el('th', { text: 'Sync' }), el('th', {}))),
          el('tbody', {}, room.members.map((m) => memberRow(room, m)))))
        : el('p', { className: 'empty', text: 'Nobody here yet. Add someone below, or share the name and password.' }));

    const options = candidates(ov, room.id);
    const select = el('select', { 'aria-label': `Who to add to ${room.name}`, 'data-key': `room-add:${room.id}` },
      el('option', { value: '', text: options.length ? 'Add someone or a device...' : 'Nobody else to add right now' }),
      groupedOptions(options));
    const role = roleSelect(`Role in ${room.name}`, `room-role:${room.id}`);
    const { add } = addControls(select, role, (value, r) => addToRoom(room.id, value, r));
    const foot = el('div', { className: 'room-foot' }, select, role, add);

    return el('article', { className: 'card room' },
      el('div', { className: 'room-top' }, title, actions), meta, body, foot);
  };

  const renderRooms = (ov) => {
    const list = $('rooms');
    list.replaceChildren();
    $('room-count').textContent = String(ov.rooms.length);
    if (!ov.rooms.length) {
      list.append(el('p', { className: 'empty', text: 'No rooms right now. Create a group above, or wait for someone to start a watch party.' }));
      return;
    }
    for (const room of ov.rooms) list.append(roomCard(ov, room));
  };

  const roomOptions = (ov, placeholder) => [
    el('option', { value: '', text: ov.rooms.length ? placeholder : 'Create a group first' }),
    ov.rooms.map((r) => el('option', { value: r.id, text: r.name }))
  ];

  const renderUnassigned = (ov) => {
    const box = $('unassigned');
    box.replaceChildren();
    $('unassigned-count').textContent = String(ov.unassigned.length);
    if (!ov.unassigned.length) {
      box.append(el('p', { className: 'empty', text: 'Everyone with the Watch Party panel open is in a room.' }));
      return;
    }
    const rows = ov.unassigned.map((c) => {
      const cell = el('td', { className: 'actions' });
      if (c.authenticated) {
        const select = el('select', { 'aria-label': `Room for ${c.name}`, 'data-key': `unassigned:${c.id}` },
          roomOptions(ov, 'Choose a room...'));
        const role = roleSelect(`Role for ${c.name}`, `unassigned-role:${c.id}`);
        const { add } = addControls(select, role, (roomId, r) => addToRoom(roomId, `c:${c.id}`, r));
        cell.append(select, role, add);
      } else {
        cell.append(el('span', { className: 'muted small', text: 'Not signed in yet' }));
      }
      return el('tr', {},
        el('td', {}, el('div', { className: 'name' }, el('span', { className: `dot${c.connected ? '' : ' dot-off'}` }), c.name)),
        cell);
    });
    box.append(el('div', { className: 'table-wrap' }, el('table', { className: 'stack' }, el('tbody', {}, rows))));
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
      $('devices-count').textContent = '';
      box.append(el('p', { className: 'note' },
        `${jf.reason || 'Jellyfin devices are not set up.'} With JELLYFIN_URL and JELLYFIN_API_KEY set, you can put TV apps and other Jellyfin clients into rooms from here.`));
      return;
    }
    if (devices && devices.error) {
      box.append(el('p', { className: 'note problem', text: `Can't reach Jellyfin: ${devices.error}` }));
    }
    const list = (devices && devices.sessions) || [];
    $('devices-count').textContent = String(list.length);
    if (!list.length) {
      box.append(el('p', { className: 'empty', text: 'No other Jellyfin apps are active. Open a Jellyfin app on the device and it shows up here.' }));
      return;
    }
    const rows = list.map((d) => {
      const where = d.bridged_as ? findMember(ov, d.bridged_as) : null;
      const actions = el('td', { className: 'actions' });
      if (where) {
        const via = d.bridged_by_plugin ? ' (from the Watch Party panel)' : '';
        actions.append(el('span', { className: 'muted small', text: `${where.member.is_host ? 'Host' : 'Receiver'} in ${where.room.name}${via}` }),
          el('button', {
            className: 'btn btn-danger btn-small', type: 'button', text: 'Remove',
            onclick: () => act('Device removed', () => api('DELETE', `api/rooms/${enc(where.room.id)}/members/${enc(where.member.id)}`))
          }));
      } else {
        const room = el('select', { 'aria-label': `Room for ${d.device_name}`, 'data-key': `device:${d.id}` },
          roomOptions(ov, 'Choose a room...'));
        const role = roleSelect(`Role for ${d.device_name}`, `device-role:${d.id}`);
        if (!d.remote_control) {
          role.value = 'host';
          role.querySelector('option[value="receiver"]').disabled = true;
        }
        const { add } = addControls(room, role, (roomId, r) => addToRoom(roomId, `j:${d.id}`, r));
        actions.append(room, role, add);
      }
      const now = d.now_playing
        ? el('span', {}, pill(d.paused ? 'Paused' : 'Playing', d.paused ? 'muted' : 'good'),
          ` ${d.now_playing.name || d.now_playing.id.slice(0, 8)} at ${fmtTime(d.position)}`)
        : el('span', { className: 'muted', text: 'Nothing playing' });
      return el('tr', {},
        el('td', {}, el('div', { className: 'name', text: d.device_name || d.client }),
          el('div', { className: 'sub', text: `${d.user_name || 'No user'} - ${d.client}` })),
        el('td', {}, now),
        el('td', {}, d.remote_control
          ? el('span', { className: 'badge', text: 'Host or receiver' })
          : el('span', { className: 'badge', title: 'This app does not accept remote control from Jellyfin', text: 'Host only' })),
        actions);
    });
    box.append(el('div', { className: 'table-wrap' }, el('table', { className: 'stack' },
      el('thead', {}, el('tr', {}, el('th', { text: 'Device' }), el('th', { text: 'Now' }), el('th', { text: 'Can be' }), el('th', {}))),
      el('tbody', {}, rows))));
  };

  const renderServerInfo = (ov) => {
    const s = ov.server || {};
    const jf = s.jellyfin || {};
    const jfState = !jf.enabled ? ['Jellyfin devices off', 'muted'] : jf.error ? ['Jellyfin unreachable', 'bad'] : ['Jellyfin connected', 'good'];
    $('server-info').replaceChildren(
      el('span', { className: 'chip', text: `${plural(ov.totals.clients, 'client')} online` }),
      el('span', { className: 'chip', text: plural(ov.totals.rooms, 'room') }),
      el('span', { className: 'chip', title: 'Whether the session server checks Jellyfin sign-in tokens (JWT_SECRET)', text: `Sign-in check ${s.auth_enabled ? 'on' : 'off'}` }),
      pill(jfState[0], jfState[1]),
      el('span', { className: 'chip', text: `v${s.version} - up ${fmtUptime(s.uptime_secs || 0)}` }));
  };

  // The lists are rebuilt on every poll. Don't do that while the admin has a
  // dropdown in them focused (it would snap shut), but only for a while, so
  // a select that simply kept focus doesn't freeze the dashboard.
  const LIVE_AREAS = ['rooms', 'unassigned', 'devices', 'users'];
  let focusedAt = 0;
  document.addEventListener('focusin', () => { focusedAt = Date.now(); });
  const isInteracting = () => {
    const a = document.activeElement;
    if ($('dialog').open) return true;
    return !!a && a.tagName === 'SELECT' && Date.now() - focusedAt < 15000
      && LIVE_AREAS.some((id) => $(id) && $(id).contains(a));
  };

  // Choices made in the lists' dropdowns survive a re-render.
  const saveChoices = () => {
    const saved = {};
    for (const sel of document.querySelectorAll('select[data-key]')) {
      saved[sel.dataset.key] = sel.value;
    }
    return saved;
  };
  const restoreChoices = (saved) => {
    for (const sel of document.querySelectorAll('select[data-key]')) {
      const v = saved[sel.dataset.key];
      if (v && [...sel.options].some((o) => o.value === v && !o.disabled)) {
        sel.value = v;
        sel.dispatchEvent(new Event('change'));
      }
    }
  };

  // --- Discord bot ---------------------------------------------------------
  const DC_FIELDS = {
    enabled: ['dc-enabled', 'bool'],
    guild_id: ['dc-guild', 'text'],
    required_role_id: ['dc-role', 'text'],
    admin_role_id: ['dc-admin-role', 'text'],
    max_rooms_per_user: ['dc-per-user', 'int'],
    max_rooms_total: ['dc-total', 'int'],
    empty_room_minutes: ['dc-empty', 'int'],
    require_password: ['dc-require-pw', 'bool'],
    allow_host: ['dc-host', 'bool'],
    allow_receiver: ['dc-receiver', 'bool']
  };

  const fillDiscordForm = (s) => {
    for (const [key, [id, type]] of Object.entries(DC_FIELDS)) {
      if (type === 'bool') $(id).checked = !!s[key];
      else $(id).value = s[key] ?? '';
    }
    $('dc-channels').value = (s.channel_ids || []).join(', ');
  };

  const readDiscordForm = () => {
    const out = {};
    for (const [key, [id, type]] of Object.entries(DC_FIELDS)) {
      out[key] = type === 'bool' ? $(id).checked : type === 'int' ? parseInt($(id).value, 10) : $(id).value.trim();
    }
    out.channel_ids = $('dc-channels').value.split(/[\s,]+/).filter(Boolean);
    return out;
  };

  const setDirty = (dirty) => {
    formDirty = dirty;
    $('dc-dirty').classList.toggle('hidden', !dirty);
  };

  const discord = () => chat && chat.available && chat.providers.find((p) => p.provider === 'discord');

  const renderChat = () => {
    const note = $('chat-note');
    const status = $('chat-status');
    const form = $('discord-form');
    note.replaceChildren();
    status.replaceChildren();
    const p = discord();
    for (const id of ['users-section', 'activity']) $(id).classList.toggle('hidden', !p);
    if (!p) {
      form.classList.add('hidden');
      status.append(pill('Not set up', 'muted'));
      note.append(el('p', { className: 'note' },
        chat ? chat.reason : 'Could not load the bot settings.',
        ' The bot needs DATA_DIR, the Jellyfin devices settings and DISCORD_INTEGRATION_TOKEN on the session server, plus the bot container.'));
      return;
    }
    const s = p.settings || {};
    status.append(s.enabled ? pill('On', 'good') : pill('Off', 'muted'));
    if (!p.token_set) status.append(pill('No bot token', 'warn'));
    else if (p.sidecar && p.sidecar.online) status.append(pill(`Bot online${p.sidecar.bot_name ? `: ${p.sidecar.bot_name}` : ''}`, 'good'));
    else status.append(pill(p.sidecar ? `Bot offline since ${fmtAgo(p.sidecar.seen_at)}` : 'Bot not connected', 'bad'));
    if (!p.token_set) {
      note.append(el('p', { className: 'note' },
        `Set ${p.token_var} (the same long random value) on the session server and on the bot so they can talk.`));
    } else if (!chat.listening) {
      note.append(el('p', { className: 'note problem', text: 'The integration API is not listening; check the server log.' }));
    }
    form.classList.remove('hidden');
    const editing = form.contains(document.activeElement);
    if (!formDirty && !editing) fillDiscordForm(s);
  };

  const assignCode = async (u) => {
    const link = u.links && u.links.discord;
    if (u.code) {
      const ok = await ask({
        title: `New code for ${u.name}?`,
        message: `The current code stops working${link ? ` and ${link.display_name || 'their Discord account'} is disconnected` : ''}. Rooms they own stay theirs.`,
        okLabel: 'New code'
      });
      if (!ok) return;
    }
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
    try {
      const r = await api('POST', `api/users/${enc(u.id)}/code`);
      toast(`Code for ${u.name}. They run /jwp link in Discord and type "${u.name}" and this code. It is not shown again:`, { secret: r.code });
    } catch (e) {
      toast(e.message, { kind: 'error' });
    }
    await refresh(true);
  };

  const userRow = (u) => {
    const link = u.links && u.links.discord;
    const c = u.code;
    const actions = el('td', { className: 'actions' });
    if (!u.missing && !u.disabled) {
      actions.append(el('button', {
        className: c ? 'btn btn-ghost btn-small' : 'btn btn-small', type: 'button',
        text: c ? 'New code' : 'Assign code',
        onclick: () => assignCode(u)
      }));
    }
    if (link) {
      actions.append(el('button', {
        className: 'btn btn-ghost btn-small', type: 'button', text: 'Unlink',
        onclick: async () => {
          const ok = await ask({
            title: `Unlink ${u.name}?`,
            message: `${link.display_name || 'Their Discord account'} can't act as ${u.name} anymore. They can link again with the same code.`,
            okLabel: 'Unlink'
          });
          if (ok) act('Unlinked', () => api('DELETE', `api/users/${enc(u.id)}/links/discord`));
        }
      }));
    }
    if (c) {
      actions.append(el('button', {
        className: 'btn btn-danger btn-small', type: 'button', text: 'Remove code',
        onclick: async () => {
          const ok = await ask({
            title: `Remove ${u.name}'s code?`,
            message: 'The code stops working and any linked Discord account is disconnected.',
            okLabel: 'Remove code', danger: true
          });
          if (ok) act('Code removed', () => api('DELETE', `api/users/${enc(u.id)}/code`));
        }
      }));
    }
    const codeCell = !c ? el('span', { className: 'muted', text: 'No code' })
      : c.frozen ? pill('Locked: too many wrong tries', 'bad')
        : el('span', {}, el('span', { text: `Assigned ${fmtAgo(c.assigned_at)}` }),
          c.failed ? el('div', { className: 'sub problem', text: `${plural(c.failed, 'wrong try')} so far` }) : null);
    return el('tr', {},
      el('td', {},
        el('div', { className: 'name' }, u.name),
        u.is_admin ? el('span', { className: 'badge badge-admin', text: 'Admin' }) : null,
        u.disabled ? el('span', { className: 'badge', text: 'Disabled' }) : null,
        u.missing ? el('span', { className: 'badge', text: 'Not in Jellyfin anymore' }) : null),
      el('td', {}, codeCell),
      el('td', {}, link
        ? el('span', {}, el('div', { text: link.display_name || link.external_id }), el('div', { className: 'sub', text: `Linked ${fmtAgo(link.linked_at)}` }))
        : el('span', { className: 'muted', text: 'Not linked' })),
      actions);
  };

  const renderUsers = () => {
    const box = $('users');
    box.replaceChildren();
    if (!discord()) return;
    if (!users) {
      box.append(el('p', { className: 'note problem', text: 'Could not load the users.' }));
      return;
    }
    if (users.error) box.append(el('p', { className: 'note problem', text: `Can't read the Jellyfin users: ${users.error}` }));
    $('users-count').textContent = String(users.users.length);
    if (!users.users.length) {
      box.append(el('p', { className: 'empty', text: 'No Jellyfin users found.' }));
      return;
    }
    box.append(el('div', { className: 'table-wrap' }, el('table', { className: 'stack' },
      el('thead', {}, el('tr', {}, el('th', { text: 'Jellyfin user' }), el('th', { text: 'Code' }), el('th', { text: 'Discord' }), el('th', {}))),
      el('tbody', {}, users.users.map(userRow)))));
  };

  const renderAudit = () => {
    const box = $('audit');
    if (!$('activity').open) return;
    box.replaceChildren();
    const entries = (audit && audit.entries) || [];
    if (!entries.length) {
      box.append(el('p', { className: 'empty', text: 'Nothing yet. Links, failed codes and room changes from the bot show up here (kept until the server restarts).' }));
      return;
    }
    box.append(el('ul', { className: 'audit-list' }, entries.slice(0, 200).map((e) =>
      el('li', { className: e.warn ? 'problem' : '' },
        el('time', { text: fmtClock(e.ts) }),
        el('span', { className: 'audit-actor', text: e.actor }),
        el('span', { text: e.detail })))));
  };

  const render = (ov) => {
    const saved = saveChoices();
    renderServerInfo(ov);
    renderRooms(ov);
    renderUnassigned(ov);
    renderDevices(ov);
    renderChat();
    renderUsers();
    renderAudit();
    restoreChoices(saved);
  };

  const refresh = async (force = false) => {
    try {
      const [ov, dev, ch] = await Promise.all([
        api('GET', 'api/overview'),
        api('GET', 'api/jellyfin/sessions').catch(() => null),
        api('GET', 'api/integrations').catch(() => null)
      ]);
      overview = ov;
      devices = dev;
      chat = ch;
      if (chat && chat.available) {
        const [us, au] = await Promise.all([
          api('GET', 'api/users').catch(() => null),
          $('activity').open ? api('GET', 'api/audit').catch(() => audit) : audit
        ]);
        users = us;
        audit = au;
      }
      if (force || !isInteracting()) render(overview);
    } catch (e) {
      if (!$('app-view').classList.contains('hidden')) toast(e.message, { kind: 'error' });
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
    $('dialog-gen').addEventListener('click', () => {
      $('dialog-input').value = generatePassword();
      $('dialog-input').select();
    });

    // Remember whether the help is open across visits.
    const help = $('help');
    try { help.open = localStorage.getItem('jwp-admin-help') === 'open'; } catch (e) { /* no storage */ }
    help.addEventListener('toggle', () => {
      try { localStorage.setItem('jwp-admin-help', help.open ? 'open' : 'closed'); } catch (e) { /* no storage */ }
    });

    const form = $('discord-form');
    form.addEventListener('input', () => setDirty(true));
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      act('Discord settings saved', async () => {
        await api('PUT', 'api/integrations/discord', readDiscordForm());
        setDirty(false);
      });
    });
    $('activity').addEventListener('toggle', () => { if ($('activity').open) refresh(true); });

    $('create-form').addEventListener('submit', (e) => {
      e.preventDefault();
      const name = $('create-name').value.trim();
      const password = $('create-pass').value;
      if (!name) return;
      act(password ? `Group "${name}" created. Its password:` : `Group "${name}" created (no password)`, async () => {
        await api('POST', 'api/rooms', { name, password });
        $('create-name').value = '';
        $('create-pass').value = '';
      }, password ? { secret: password } : {});
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
