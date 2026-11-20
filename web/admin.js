// oldpad admin page. Runs on the admin's phone, so modern JavaScript is fine.
// Builds DOM with h() and textContent only: no user data ever goes through innerHTML.

const main = document.getElementById('main');

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, '');
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const c of children.flat()) if (c != null && c !== false) el.append(c);
  return el;
}

function render(...nodes) {
  // Same rules as h(): nested arrays flatten, null/false render nothing.
  main.replaceChildren(...nodes.flat(Infinity).filter((n) => n != null && n !== false));
  window.scrollTo(0, 0);
}

async function api(method, path, body) {
  const res = await fetch('/api/admin' + path, {
    method,
    headers: { 'X-Oldpad': '1', ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    credentials: 'same-origin',
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

function ago(ts) {
  const s = Date.now() / 1000 - ts;
  if (s < 120) return 'just now';
  if (s < 7200) return `${Math.round(s / 60)} min ago`;
  if (s < 172800) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} days ago`;
}

function errorBox(err) {
  return h('p', { class: 'notice bad' }, err.message || String(err));
}

// ------------------------------------------------------------- passkeys

const b64u = {
  enc: (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''),
  dec: (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0)),
};

async function createPasskey(opts) {
  const cred = await navigator.credentials.create({
    publicKey: {
      ...opts,
      challenge: b64u.dec(opts.challenge),
      user: { ...opts.user, id: b64u.dec(opts.user.id) },
      excludeCredentials: opts.excludeCredentials.map((c) => ({ ...c, id: b64u.dec(c.id) })),
    },
  });
  return {
    id: cred.id,
    rawId: b64u.enc(cred.rawId),
    type: cred.type,
    response: {
      clientDataJSON: b64u.enc(cred.response.clientDataJSON),
      attestationObject: b64u.enc(cred.response.attestationObject),
    },
  };
}

async function getPasskey(opts) {
  const cred = await navigator.credentials.get({ publicKey: { ...opts, challenge: b64u.dec(opts.challenge) } });
  return {
    id: cred.id,
    rawId: b64u.enc(cred.rawId),
    type: cred.type,
    response: {
      clientDataJSON: b64u.enc(cred.response.clientDataJSON),
      authenticatorData: b64u.enc(cred.response.authenticatorData),
      signature: b64u.enc(cred.response.signature),
      userHandle: cred.response.userHandle ? b64u.enc(cred.response.userHandle) : null,
    },
  };
}

async function register(extra) {
  const opts = await api('POST', '/register/options', extra);
  const credential = await createPasskey(opts);
  await api('POST', '/register/verify', { challenge: opts.challenge, credential });
}

async function signIn() {
  const opts = await api('POST', '/login/options', {});
  const credential = await getPasskey(opts);
  await api('POST', '/login/verify', { challenge: opts.challenge, credential });
}

// ------------------------------------------------------------- views

function setupView() {
  const name = h('input', { type: 'text', id: 'name', autocomplete: 'name', placeholder: 'Your name' });
  const code = h('input', { type: 'text', id: 'code', autocapitalize: 'characters', autocomplete: 'off', placeholder: 'ABCD2345' });
  const out = h('div');
  const go = h('button', { class: 'primary', onclick: async () => {
    go.disabled = true;
    try {
      await register({ name: name.value, setupCode: code.value });
      route();
    } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  } }, 'Create passkey');
  const pairing = location.hash.startsWith('#/pair/');
  render(
    h('h1', {}, 'Welcome to oldpad'),
    pairing ? h('p', { class: 'notice' }, 'Before you can approve this screen, create the admin account. You will come straight back to the pairing afterwards.') : null,
    h('p', {}, 'Create the first admin. You will sign in with a passkey (Face ID, Touch ID or your device PIN) — there is no password.'),
    h('label', { for: 'name' }, 'Your name'), name,
    h('label', { for: 'code' }, 'Setup code'), code,
    h('p', { class: 'help' }, 'Printed in the server log when oldpad starts for the first time (kubectl logs, docker logs). This is not the code shown on the screen you are pairing.'),
    h('div', { class: 'actions' }, go), out,
  );
}

function loginView() {
  const out = h('div');
  const go = h('button', { class: 'primary', onclick: async () => {
    go.disabled = true;
    try { await signIn(); route(); } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  } }, 'Sign in with passkey');
  render(h('h1', {}, 'Sign in'), h('div', { class: 'actions' }, go), out);
}

// A music service or similar the integration can sign in to. The sign-in page opens
// in a new tab; the server collects the token itself, so we only wait and refresh.
function accountRow(i, a) {
  const status = h('span', { class: a.connected ? 'ok' : 'muted' }, a.connected ? 'Connected' : 'Not connected');
  const go = h('button', { onclick: async () => {
    const tab = window.open('about:blank', '_blank'); // before awaiting, or it counts as a popup
    go.disabled = true;
    try {
      const { url } = await api('POST', `/integrations/${i.id}/connect`, { account: a.id });
      if (tab) tab.location = url; else location.href = url;
      status.textContent = 'Waiting for you to sign in…';
      for (let n = 0; n < 100; n++) {
        await new Promise((r) => setTimeout(r, 3000));
        const now = (await api('GET', '/integrations')).find((x) => x.id === i.id);
        if (now?.accounts?.find((x) => x.id === a.id)?.connected) { route(); return; }
      }
      status.textContent = 'Sign-in timed out. Try again.';
    } catch (e) {
      tab?.close();
      status.textContent = e.message;
    }
    go.disabled = false;
  } }, a.connected ? 'Reconnect' : 'Connect');
  return h('div', { class: 'row account' }, h('div', {}, a.name, ' · ', status), go);
}

const day = (ts) => new Date(ts * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });

function peopleSection({ people, invites }) {
  return [
    h('h2', {}, 'People'),
    people.map((p) => h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('strong', {}, p.name, p.me ? h('span', { class: 'small muted' }, ' · you') : null),
        p.me
          ? h('button', { onclick: async () => { try { await register({}); route(); } catch (e) { alert(e.message); } } }, 'Add passkey on this device')
          : h('button', { class: 'danger', onclick: async () => {
              if (!confirm(`Remove ${p.name}? They can no longer sign in.`)) return;
              try { await api('DELETE', `/people/${p.id}`); route(); } catch (e) { alert(e.message); }
            } }, 'Remove')),
      p.passkeys.map((k) => h('div', { class: 'row account' },
        h('div', { class: 'small' }, `Passkey added ${day(k.created)}`, h('span', { class: 'muted' }, k.lastUsed ? ` · last used ${ago(k.lastUsed)}` : '')),
        p.passkeys.length > 1 ? h('button', { class: 'chip danger', onclick: async () => {
          if (!confirm('Remove this passkey? The device it lives on can no longer sign in.')) return;
          try { await api('DELETE', `/passkeys/${encodeURIComponent(k.id)}`); route(); } catch (e) { alert(e.message); }
        } }, 'Remove') : null)))),
    invites.length ? h('p', { class: 'small muted' }, `${invites.length} open invitation${invites.length > 1 ? 's' : ''}, valid until ${new Date(Math.max(...invites) * 1000).toLocaleString()}. `,
      h('button', { class: 'chip', onclick: async () => { await api('DELETE', '/invites'); route(); } }, 'Cancel invitations')) : null,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: () => (location.hash = '#/invite-new') }, 'Invite someone'),
      h('button', { onclick: async () => { await api('POST', '/logout', {}); route(); } }, 'Sign out')),
  ];
}

// A one-time link for another adult: they open it on their phone and make their own passkey.
async function inviteNewView() {
  const inv = await api('POST', '/invites', {});
  const link = h('input', { type: 'text', readonly: true, value: inv.url, onfocus: (e) => e.target.select() });
  render(
    h('h1', {}, 'Invite someone'),
    h('p', {}, 'Let them scan this code with their phone, or send them the link. They choose a name and make their own passkey. The link works once, for 24 hours.'),
    h('img', { class: 'qr', alt: 'QR code for the invitation link', src: `data:image/svg+xml;base64,${btoa(inv.qr)}` }),
    link,
    h('div', { class: 'actions' },
      navigator.share ? h('button', { class: 'primary', onclick: () => navigator.share({ title: 'oldpad', url: inv.url }).catch(() => {}) }, 'Share link') : null,
      h('button', { onclick: async () => { await navigator.clipboard.writeText(inv.url); alert('Copied.'); } }, 'Copy link'),
      h('button', { onclick: () => (location.hash = '#/people') }, 'Done')),
  );
}

function inviteView(code) {
  const name = h('input', { type: 'text', id: 'name', autocomplete: 'name', placeholder: 'Your name', maxlength: '64' });
  const out = h('div');
  const go = h('button', { class: 'primary', onclick: async () => {
    go.disabled = true;
    try { await register({ name: name.value, invite: code }); location.hash = '#/'; route(); } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  } }, 'Create passkey');
  render(
    h('h1', {}, 'Join oldpad'),
    h('p', {}, 'You were invited to help run the screens in this home. You will sign in with a passkey (Face ID, Touch ID or your device PIN) — there is no password.'),
    h('label', { for: 'name' }, 'Your name'), name,
    h('div', { class: 'actions' }, go), out,
  );
}

async function screensView() {
  const devices = await api('GET', '/devices');
  render(
    h('h1', {}, 'Screens'),
    devices.length
      ? devices.map((d) => h('div', { class: 'card' },
          h('div', { class: 'row' },
            h('div', {}, h('strong', {}, d.name), h('div', { class: 'small muted' },
              h('span', { class: d.online ? 'ok' : '' }, d.online ? '● Online' : `Offline · seen ${ago(d.lastSeen)}`),
              ` · ${d.tiles.length} tiles${d.pages?.length ? ` · ${d.pages.length} own page${d.pages.length > 1 ? 's' : ''}` : ''}`)),
            h('div', { class: 'row' },
              h('a', { class: 'button', href: `/?preview=${encodeURIComponent(d.id)}`, target: '_blank' }, 'Preview'),
              h('button', { disabled: !d.online, title: 'Reload the page on the screen', onclick: async (ev) => {
                ev.target.disabled = true;
                await api('POST', `/devices/${d.id}/reload`, {});
                setTimeout(route, 4000);
              } }, 'Reload'),
              h('button', { onclick: () => (location.hash = `#/device/${d.id}`) }, 'Layout'),
              h('button', { class: 'danger', onclick: async () => {
                if (!confirm(`Remove ${d.name}? It will have to be paired again.`)) return;
                await api('DELETE', `/devices/${d.id}`); route();
              } }, 'Remove')))))
      : h('p', { class: 'muted' }, 'No screens yet.'),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => (location.hash = '#/add-screen') }, 'Add screen')),
  );
}

async function integrationsView() {
  const integrations = await api('GET', '/integrations');
  render(
    h('h1', {}, 'Integrations'),
    integrations.map((i) => h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('div', {}, h('a', { href: `#/integration-info/${i.id}`, class: 'title' }, i.name), h('div', { class: 'small muted' }, i.typeName, ' · ',
          h('span', { class: i.status === 'running' ? 'ok' : i.status.startsWith('error') ? 'bad' : '' }, i.status))),
        h('div', { class: 'row' },
          h('button', { onclick: () => (location.hash = `#/integration/${i.id}`) }, 'Edit'),
          h('button', { class: 'danger', onclick: async () => {
            if (!confirm(`Remove ${i.name}? Its tiles will show as unavailable.`)) return;
            await api('DELETE', `/integrations/${i.id}`); route();
          } }, 'Remove'))),
      (i.accounts || []).map((a) => accountRow(i, a)))),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => (location.hash = '#/integration/new') }, 'Add integration')),
  );
}

async function peopleView() {
  render(peopleSection(await api('GET', '/people')));
}

async function settingsView() {
  const info = await api('GET', '/settings');
  const st = { ...info.settings };
  const hours = (key) => h('select', { id: `s-${key}`, onchange: (e) => { st[key] = Number(e.target.value); } },
    Array.from({ length: 24 }, (_, i) => h('option', { value: i, selected: st[key] === i }, `${String(i).padStart(2, '0')}:00`)));
  const choice = (key, options) => h('select', { id: `s-${key}`, onchange: (e) => { st[key] = e.target.value; } },
    options.map(([v, label]) => h('option', { value: v, selected: st[key] === v }, label)));
  const out = h('div');
  const file = h('input', { type: 'file', accept: 'application/json,.json', id: 'restore', class: 'hidden', onchange: async () => {
    const f = file.files[0];
    if (!f) return;
    try {
      const backup = JSON.parse(await f.text());
      if (!confirm(`Restore this backup from ${backup.exported ? new Date(backup.exported * 1000).toLocaleString() : 'an unknown date'}? Screens, integrations and settings are replaced; people in it are added.`)) return;
      await api('POST', '/restore', backup);
      out.replaceChildren(h('p', { class: 'notice ok' }, 'Restored. Screens reload by themselves.'));
    } catch (e) { out.replaceChildren(errorBox(e)); }
    file.value = '';
  } });
  render(
    h('h1', {}, 'Settings'),
    h('h2', {}, 'Screens'),
    h('label', { for: 's-theme' }, 'Theme'),
    choice('theme', [['auto', 'Light by day, dark at night'], ['day', 'Always light'], ['night', 'Always dark']]),
    h('div', { class: 'row' }, h('div', {}, h('label', { for: 's-day_start' }, 'Day starts'), hours('day_start')), h('div', {}, h('label', { for: 's-day_end' }, 'Night starts'), hours('day_end'))),
    h('label', { for: 's-language' }, 'Language'),
    choice('language', [['auto', 'Each screen\'s own language'], ['nb', 'Norsk'], ['en', 'English']]),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: async () => {
      try { await api('PUT', '/settings', st); out.replaceChildren(h('p', { class: 'notice ok' }, 'Saved. Screens reload by themselves.')); } catch (e) { out.replaceChildren(errorBox(e)); }
    } }, 'Save')),
    h('h2', {}, 'Backup'),
    h('p', { class: 'muted' }, 'Screens, their layouts, integrations, people and settings in one file. It includes integration secrets such as the Homey key and the Spotify sign-in, so keep it somewhere private.'),
    h('div', { class: 'actions' },
      h('button', { onclick: async () => {
        const res = await fetch('/api/admin/backup', { credentials: 'same-origin' });
        if (!res.ok) { out.replaceChildren(errorBox(new Error(`HTTP ${res.status}`))); return; }
        const a = h('a', { href: URL.createObjectURL(await res.blob()), download: `oldpad-backup-${new Date().toISOString().slice(0, 10)}.json` });
        document.body.append(a); a.click(); a.remove();
      } }, 'Download backup'),
      h('button', { onclick: () => file.click() }, 'Restore from backup…'), file),
    out,
    h('h2', {}, 'About'),
    h('p', { class: 'small muted' }, `oldpad ${info.version}${info.commit ? ` (${info.commit})` : ''} · ${info.publicUrl} · ${info.counts.screens} screens, ${info.counts.integrations} integrations, ${info.counts.people} people`),
  );
}

// Pairing starts on the new screen; this explains how and takes the code it shows.
function addScreenView() {
  const code = h('input', { type: 'text', id: 'p-code', maxlength: '8', autocapitalize: 'characters', autocomplete: 'off', placeholder: 'ABCD2345' });
  const go = () => { const c = code.value.trim().toUpperCase(); if (c) location.hash = `#/pair/${encodeURIComponent(c)}`; };
  code.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
  render(
    h('h1', {}, 'Add screen'),
    h('ol', {},
      h('li', {}, 'On the tablet, open ', h('strong', {}, location.origin.replace('https://', 'http://')), '.'),
      h('li', {}, 'It shows a QR code. Scan it with this phone — or type the code shown under it here:')),
    h('label', { for: 'p-code' }, 'Code from the screen'),
    code,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: go }, 'Continue'),
      h('button', { onclick: () => (location.hash = '#/') }, 'Cancel')),
  );
}

async function pairView(code) {
  const { status } = await api('GET', `/pair/${encodeURIComponent(code)}`);
  if (status !== 'pending') {
    render(h('h1', {}, 'Pair a screen'), h('p', { class: 'notice' },
      status === 'approved' ? 'This screen is already paired.' : 'This code has expired. Start again on the screen.'));
    return;
  }
  const name = h('input', { type: 'text', id: 'name', value: 'Kitchen', maxlength: '64' });
  const out = h('div');
  const go = h('button', { class: 'primary', onclick: async () => {
    go.disabled = true;
    try {
      const { id } = await api('POST', `/pair/${encodeURIComponent(code)}`, { name: name.value });
      location.hash = `#/device/${id}`;
    } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  } }, 'Approve');
  render(
    h('h1', {}, 'Pair a screen'),
    h('p', {}, 'Check that the screen shows this code:'),
    h('p', { class: 'code' }, code.toUpperCase()),
    h('label', { for: 'name' }, 'Name this screen'), name,
    h('p', { class: 'help' }, 'Usually the room it hangs in.'),
    h('div', { class: 'actions' }, go, h('button', { onclick: () => (location.hash = '#/') }, 'Cancel')), out,
  );
}

async function deviceView(id) {
  const [devices, entities] = await Promise.all([api('GET', '/devices'), api('GET', '/entities')]);
  const device = devices.find((d) => d.id === id);
  if (!device) return render(errorBox(new Error('No such screen.')));
  const tiles = [...device.tiles];
  const more = new Set(device.more);
  const panels = { ...device.panels };
  const has = (e, cap) => e.capabilities && cap in e.capabilities;

  // Which entities fit each home panel.
  const panelDefs = [
    ['indoor', 'Indoor', (e) => has(e, 'measure_temperature') && e.class !== 'weather'],
    ['outdoor', 'Outdoor', (e) => has(e, 'measure_temperature') && e.class !== 'weather'],
    ['wind', 'Wind (optional, if outdoor has none)', (e) => has(e, 'measure_wind_strength')],
    ['weather', 'Forecast', (e) => e.class === 'weather'],
    ['speaker', 'Music: this screen\'s room', (e) => e.class === 'speaker'],
  ];
  const selects = panelDefs.map(([key, label, fits]) => {
    const options = entities.filter(fits);
    const sel = h('select', { id: `p-${key}`, onchange: () => { panels[key] = sel.value || null; } },
      h('option', { value: '' }, '— none —'),
      options.map((e) => h('option', { value: e.id, selected: panels[key] === e.id }, `${e.name}${e.zone && e.zone !== e.name ? ` (${e.zone})` : ''}`)));
    return [h('label', { for: `p-${key}` }, label), sel];
  });

  const list = h('div');
  const draw = () => {
    let zone = null;
    list.replaceChildren(...entities.filter((e) => e.class !== 'weather').flatMap((e) => {
      const i = tiles.indexOf(e.id);
      const home = h('button', { class: i >= 0 ? 'chip on' : 'chip', onclick: () => {
        i >= 0 ? tiles.splice(i, 1) : tiles.push(e.id);
        draw();
      } }, i >= 0 ? `Home ${i + 1}` : 'Home');
      const menu = h('button', { class: more.has(e.id) ? 'chip on' : 'chip', onclick: () => {
        more.has(e.id) ? more.delete(e.id) : more.add(e.id);
        draw();
      } }, 'Menu');
      const row = h('div', { class: 'pick' }, h('span', { class: 'grow' }, e.name, h('span', { class: 'small muted' }, ` · ${e.class}`)), home, menu);
      const header = e.zone !== zone ? h('div', { class: 'zone' }, (zone = e.zone) || 'No room') : null;
      return header ? [header, row] : [row];
    }));
  };
  draw();

  // Own pages: a name, an icon and an ordered list of cards and entities.
  const pages = structuredClone(device.pages || []);
  const cards = [['card:clock', 'Clock'], ['card:climate', 'Indoor / outdoor'], ['card:forecast', 'Forecast'], ['card:music', 'Music']];
  const icons = ['home', 'music', 'bulb', 'thermo', 'bolt', 'cloud', 'sun', 'moon', 'chart', 'radio', 'flame', 'plug'];
  const label = (item) => cards.find(([id]) => id === item)?.[1] ?? (entities.find((e) => e.id === item)?.name || 'Unavailable');
  const pageList = h('div');
  const drawPages = () => {
    pageList.replaceChildren(...pages.map((pg, n) => {
      const pname = h('input', { type: 'text', value: pg.name, maxlength: '32', 'aria-label': 'Page name', oninput: () => { pg.name = pname.value; } });
      const picon = h('select', { 'aria-label': 'Icon', onchange: () => { pg.icon = picon.value; } }, icons.map((i) => h('option', { value: i, selected: pg.icon === i }, i)));
      const add = h('select', { 'aria-label': 'Add to page', onchange: () => { if (add.value) { pg.items.push(add.value); drawPages(); } } },
        h('option', { value: '' }, '+ Add…'),
        h('optgroup', { label: 'Cards' }, cards.filter(([id]) => !pg.items.includes(id)).map(([id, l]) => h('option', { value: id }, l))),
        h('optgroup', { label: 'Devices' }, entities.filter((e) => e.class !== 'weather' && !pg.items.includes(e.id)).map((e) => h('option', { value: e.id }, `${e.name}${e.zone && e.zone !== e.name ? ` (${e.zone})` : ''}`))));
      const move = (i, d) => { const [x] = pg.items.splice(i, 1); pg.items.splice(i + d, 0, x); drawPages(); };
      return h('div', { class: 'card' },
        h('div', { class: 'row' }, pname, picon,
          h('button', { class: 'danger', onclick: () => { if (confirm(`Delete the page “${pg.name}”?`)) { pages.splice(n, 1); drawPages(); } } }, 'Delete')),
        pg.items.map((item, i) => h('div', { class: 'pick' }, h('span', { class: 'grow' }, label(item)),
          h('button', { class: 'chip', disabled: i === 0, onclick: () => move(i, -1), 'aria-label': 'Up' }, '↑'),
          h('button', { class: 'chip', disabled: i === pg.items.length - 1, onclick: () => move(i, 1), 'aria-label': 'Down' }, '↓'),
          h('button', { class: 'chip', onclick: () => { pg.items.splice(i, 1); drawPages(); }, 'aria-label': 'Remove' }, '×'))),
        add);
    }), h('button', { onclick: () => { pages.push({ id: Math.random().toString(36).slice(2, 10), name: 'New page', icon: 'home', items: [] }); drawPages(); } }, 'Add page'));
  };
  drawPages();

  const out = h('div');
  const name = h('input', { type: 'text', id: 'd-name', value: device.name, maxlength: '64' });
  render(
    h('h1', {}, device.name),
    h('label', { for: 'd-name' }, 'Name'),
    name,
    h('h2', {}, 'Home panels'),
    selects,
    h('h2', {}, 'Tiles and menu pages'),
    h('p', { class: 'muted' }, '“Home” puts it in the tile row, in the order you tap (six fit). “Menu” shows it on the lights, climate, Flows or music page.'),
    entities.length ? list : h('p', { class: 'notice' }, 'Nothing to show yet. Add an integration first.'),
    h('h2', {}, 'Own pages'),
    h('p', { class: 'muted' }, 'Extra pages in the screen\'s side rail, for what doesn\'t fit on the home page. Devices on a page can be controlled from it.'),
    pageList,
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => {
        try {
          const n = name.value.trim();
          if (!n) throw new Error('Give the screen a name.');
          if (pages.some((pg) => !pg.name.trim())) throw new Error('Every page needs a name.');
          await api('PUT', `/devices/${id}`, { name: n, tiles, more: [...more], panels, pages });
          location.hash = '#/';
        } catch (e) { out.replaceChildren(errorBox(e)); }
      } }, 'Save'),
      h('button', { onclick: () => (location.hash = '#/') }, 'Cancel')),
    out,
  );
}

// What an integration brings: its status, its devices with their current values,
// and which screens use each one.
function showValue(v) {
  if (v === null || v === undefined) return '—';
  if (typeof v === 'boolean') return v ? 'on' : 'off';
  if (typeof v === 'number') return String(Math.round(v * 100) / 100);
  if (Array.isArray(v)) return `${v.length} items`;
  if (typeof v === 'object') return '…';
  return String(v).length > 40 ? `${String(v).slice(0, 40)}…` : String(v);
}

async function integrationInfoView(id) {
  const [integrations, entities, devices] = await Promise.all([api('GET', '/integrations'), api('GET', '/entities'), api('GET', '/devices')]);
  const i = integrations.find((x) => x.id === id);
  if (!i) return render(errorBox(new Error('No such integration.')));
  const mine = entities.filter((e) => e.id.startsWith(`${id}:`));
  const usedOn = (eid) => devices.filter((d) => d.tiles.includes(eid) || d.more.includes(eid) || Object.values(d.panels).includes(eid) || (d.pages || []).some((p) => p.items.includes(eid))).map((d) => d.name);
  const filter = h('input', { type: 'search', placeholder: `Filter ${mine.length} devices`, 'aria-label': 'Filter devices' });
  const list = h('div');
  const draw = () => {
    const q = filter.value.trim().toLowerCase();
    let zone = null;
    list.replaceChildren(...mine.filter((e) => !q || `${e.name} ${e.zone} ${e.class}`.toLowerCase().includes(q)).flatMap((e) => {
      const caps = Object.entries(e.capabilities || {}).filter(([, c]) => c.value !== null && typeof c.value !== 'object').map(([k, c]) => `${k} ${showValue(c.value)}`);
      const used = usedOn(e.id);
      const row = h('div', { class: 'pick' }, h('span', { class: 'grow' }, e.name, h('span', { class: 'small muted' }, ` · ${e.class}`),
        h('div', { class: 'small muted' }, caps.join(' · ') || 'no values'),
        used.length ? h('div', { class: 'small ok' }, `On ${used.join(', ')}`) : null));
      const header = e.zone !== zone ? h('div', { class: 'zone' }, (zone = e.zone) || 'No room') : null;
      return header ? [header, row] : [row];
    }));
  };
  filter.addEventListener('input', draw);
  draw();
  const restart = h('button', { onclick: async () => {
    restart.disabled = true;
    await api('POST', `/integrations/${id}/restart`, {});
    setTimeout(route, 2500);
  } }, 'Restart');
  render(
    h('h1', {}, i.name),
    h('p', { class: `notice ${i.status === 'running' ? 'ok' : i.status.startsWith('error') ? 'bad' : ''}` }, `${i.typeName}: ${i.status}`),
    h('div', { class: 'actions' }, restart, h('button', { onclick: () => (location.hash = `#/integration/${id}`) }, 'Edit settings'), h('button', { onclick: () => (location.hash = '#/integrations') }, 'Back')),
    (i.accounts || []).length ? [h('h2', {}, 'Accounts'), h('div', { class: 'card' }, i.accounts.map((a) => accountRow(i, a)))] : null,
    mine.some((e) => e.capabilities?.arrange_favourites) ? favouritesSection(id, mine.find((e) => e.capabilities.arrange_favourites)) : null,
    h('h2', {}, 'Devices'),
    mine.length ? [filter, list] : h('p', { class: 'muted' }, i.status === 'running' ? 'This integration has no devices.' : 'Devices show up here once it is running.'),
  );
}

// The household's favourites (Sonos): oldpad's order, which go on the front page, and removing.
function favouritesSection(iid, speaker) {
  let favs = speaker.capabilities.favourites.value.map((f) => ({ ...f }));
  const list = h('div', { class: 'card' });
  const note = h('div');
  const save = async () => {
    draw();
    try {
      await api('POST', '/set', { id: speaker.id, capability: 'arrange_favourites', value: { order: favs.map((f) => f.id), front: favs.filter((f) => f.front).map((f) => f.id) } });
      note.replaceChildren();
    } catch (e) { note.replaceChildren(errorBox(e)); }
  };
  const move = (i, by) => { const [f] = favs.splice(i, 1); favs.splice(i + by, 0, f); save(); };
  const draw = () => list.replaceChildren(...(favs.length ? favs.map((f, i) => h('div', { class: 'fav-row' },
    f.art ? h('img', { src: `/api/image/${iid}/${f.art}`, alt: '' }) : h('span', { class: 'noart' }, f.title.slice(0, 1)),
    h('span', { class: 'grow' }, f.title),
    h('button', { class: `chip${f.front ? ' on' : ''}`, title: 'Show on the front page', 'aria-pressed': String(f.front), onclick: () => { f.front = !f.front; save(); } }, 'Front'),
    h('button', { class: 'chip', 'aria-label': `Move ${f.title} up`, disabled: i === 0, onclick: () => move(i, -1) }, '↑'),
    h('button', { class: 'chip', 'aria-label': `Move ${f.title} down`, disabled: i === favs.length - 1, onclick: () => move(i, 1) }, '↓'),
    h('button', { class: 'chip danger', 'aria-label': `Remove ${f.title}`, onclick: async () => {
      if (!confirm(`Remove "${f.title}" from Sonos favourites? This removes it in the Sonos app too.`)) return;
      try {
        await api('POST', '/set', { id: speaker.id, capability: 'remove_favourite', value: f.id });
        favs = favs.filter((x) => x !== f);
        draw();
      } catch (e) { note.replaceChildren(errorBox(e)); }
    } }, '✕'))) : [h('p', { class: 'muted' }, 'No favourites yet. Star music or radio in a screen\'s search to add some.')]));
  draw();
  return [
    h('h2', {}, 'Favourites'),
    h('p', { class: 'muted small' }, 'The front page shows the ones marked Front, in this order. The music page shows them all.'),
    h('div', { class: 'actions' }, h('button', { class: 'chip', onclick: () => { favs.sort((a, b) => a.title.localeCompare(b.title)); save(); } }, 'Sort A–Z')),
    note, list,
  ];
}

// The integration wizard: 1. pick a type, 2. fill in its fields, 3. test, 4. save.
async function integrationView(id) {
  const kinds = await api('GET', '/kinds');
  const existing = id === 'new' ? null : (await api('GET', '/integrations')).find((i) => i.id === id);
  if (id !== 'new' && !existing) return render(errorBox(new Error('No such integration.')));
  if (!existing) {
    render(
      h('h1', {}, 'Add integration'),
      h('p', { class: 'muted' }, 'Step 1 of 2 — what do you want to connect?'),
      h('div', { class: 'kinds' }, kinds.map((k) => h('button', { class: 'card kind', onclick: () => integrationForm(k, null) },
        h('strong', {}, k.name), h('span', { class: 'small muted' }, k.description)))),
    );
  } else {
    integrationForm(kinds.find((k) => k.id === existing.type), existing);
  }
}

function integrationForm(kind, existing) {
  const inputs = {};
  const name = h('input', { type: 'text', id: 'f-name', value: existing ? existing.name : kind.name, maxlength: '64' });
  const fields = kind.fields.map((f) => {
    const type = { secret: 'password', url: 'url', number: 'number', text: 'text' }[f.kind] || 'text';
    const current = existing ? existing.config[f.key] : undefined;
    const input = f.kind === 'bool'
      ? h('input', { type: 'checkbox', id: `f-${f.key}`, checked: current === true })
      : h('input', {
          type, id: `f-${f.key}`, autocomplete: 'off', spellcheck: 'false',
          placeholder: f.kind === 'secret' && current ? 'Saved — leave empty to keep' : f.placeholder,
          value: f.kind === 'secret' || current == null ? '' : String(current),
        });
    inputs[f.key] = input;
    return [h('label', { for: `f-${f.key}` }, f.label + (f.required ? '' : ' (optional)')), input, f.help ? h('p', { class: 'help' }, f.help) : null];
  });
  const config = () => Object.fromEntries(kind.fields.map((f) => [f.key, f.kind === 'bool' ? inputs[f.key].checked : inputs[f.key].value]));
  const out = h('div');
  const test = h('button', { onclick: async () => {
    test.disabled = true;
    out.replaceChildren(h('p', { class: 'muted' }, 'Testing…'));
    try {
      const r = await api('POST', '/integrations/test', { type: kind.id, id: existing?.id, config: config() });
      out.replaceChildren(h('p', { class: `notice ${r.ok ? 'ok' : 'bad'}` }, r.message));
    } catch (e) { out.replaceChildren(errorBox(e)); }
    test.disabled = false;
  } }, 'Test connection');
  const save = h('button', { class: 'primary', onclick: async () => {
    save.disabled = true;
    try {
      const body = { type: kind.id, name: name.value, config: config() };
      if (existing) await api('PUT', `/integrations/${existing.id}`, body);
      else await api('POST', '/integrations', body);
      location.hash = '#/integrations';
    } catch (e) { out.replaceChildren(errorBox(e)); save.disabled = false; }
  } }, 'Save');
  render(
    h('h1', {}, existing ? `Edit ${existing.name}` : kind.name),
    existing ? null : h('p', { class: 'muted' }, 'Step 2 of 2 — connection details.'),
    h('p', {}, kind.description),
    h('label', { for: 'f-name' }, 'Name'), name,
    fields,
    h('div', { class: 'actions' }, test, save, h('button', { onclick: () => (location.hash = '#/integrations') }, 'Cancel')),
    out,
  );
}

// ------------------------------------------------------------- router

async function route() {
  try {
    const status = await api('GET', '/status');
    document.getElementById('who').textContent = status.admin ? status.admin.name : '';
    const top = (location.hash.split('/')[1] || '').replace(/^(device|add-screen|pair)$/, '').replace(/^integration(-info)?$/, 'integrations').replace(/^invite-new$/, 'people');
    document.getElementById('nav').replaceChildren(...(status.admin ? [['', 'Screens'], ['integrations', 'Integrations'], ['people', 'People'], ['settings', 'Settings']] : [])
      .map(([v, label]) => h('a', { href: `#/${v}`, class: top === v ? 'on' : '' }, label)));
    if (status.setupNeeded) return setupView();
    const [, view, arg] = (location.hash || '#/').split('/');
    if (!status.admin) return view === 'invite' && arg ? inviteView(arg) : loginView();
    if (view === 'invite') { location.hash = '#/'; return; }
    if (view === 'invite-new') return await inviteNewView();
    if (view === 'pair' && arg) return await pairView(arg);
    if (view === 'add-screen') return addScreenView();
    if (view === 'device' && arg) return await deviceView(arg);
    if (view === 'integration' && arg) return await integrationView(arg);
    if (view === 'integration-info' && arg) return await integrationInfoView(arg);
    if (view === 'integrations') return await integrationsView();
    if (view === 'people') return await peopleView();
    if (view === 'settings') return await settingsView();
    return await screensView();
  } catch (e) {
    render(errorBox(e));
  }
}

window.addEventListener('hashchange', route);
route();
