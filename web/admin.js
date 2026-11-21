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
  const [integrations, entities] = await Promise.all([api('GET', '/integrations'), api('GET', '/entities')]);
  const hasFavs = (id) => entities.some((e) => e.id.startsWith(`${id}:`) && e.capabilities?.arrange_favourites);
  render(
    h('h1', {}, 'Integrations'),
    integrations.map((i) => h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('div', {}, h('a', { href: `#/integration-info/${i.id}`, class: 'title' }, i.name), h('div', { class: 'small muted' }, i.typeName, ' · ',
          h('span', { class: i.status === 'running' ? 'ok' : i.status.startsWith('error') ? 'bad' : '' }, i.status))),
        h('div', { class: 'row' },
          hasFavs(i.id) ? h('button', { onclick: () => (location.hash = `#/favourites/${i.id}`) }, 'Favourites') : null,
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

// ---------------------------------------------------------------- screen editor
// Pages on the left, the selected page laid out as the screen shows it in the
// middle, and the devices to add on the right (a sheet on phones).

// The screen's built-in pages: each shows every device of its kind on the screen.
const MENU_PAGES = [
  ['music', 'Music', ['speaker'], 'speakers'],
  ['lights', 'Lights', ['light', 'socket'], 'lights and sockets'],
  ['climate', 'Climate', ['thermostat', 'heater', 'sensor'], 'thermostats, heaters and sensors'],
  ['flows', 'Flows', ['button'], 'buttons and Flows'],
  ['ships', 'Ships', ['ais'], 'ships nearby (AIS)'],
  ['departures', 'Departures', ['departures'], 'departure boards (Entur)'],
  ['calendar', 'Calendar', ['calendar'], 'calendars (iCal)'],
  ['cameras', 'Cameras', ['camera'], 'camera snapshots'],
];
const CARDS = [['card:clock', 'Clock'], ['card:climate', 'Indoor / outdoor'], ['card:forecast', 'Forecast'], ['card:music', 'Music']];
const PAGE_ICONS = ['home', 'music', 'bulb', 'thermo', 'bolt', 'cloud', 'sun', 'moon', 'chart', 'radio', 'flame', 'plug'];
const KINDS = [['all', 'All', null], ['lights', 'Lights', ['light', 'socket']], ['climate', 'Climate', ['thermostat', 'heater', 'sensor']], ['buttons', 'Buttons', ['button']], ['speakers', 'Speakers', ['speaker']]];
const HOME_TILES = 6; // what fits in the screen's tile row

// Leaving the editor with unsaved changes asks first (see the hashchange handler).
let leaveGuard = null;
let ignoreHash = false;

function moveItem(list, from, to) {
  const [x] = list.splice(from, 1);
  list.splice(Math.max(0, Math.min(to, list.length)), 0, x);
}

async function deviceView(id) {
  const [devices, entities] = await Promise.all([api('GET', '/devices'), api('GET', '/entities')]);
  const device = devices.find((d) => d.id === id);
  if (!device) return render(errorBox(new Error('No such screen.')));
  main.classList.add('wide');
  const ent = new Map(entities.map((e) => [e.id, e]));
  const s = { name: device.name, tiles: [...device.tiles], more: [...device.more], panels: { ...device.panels }, pages: structuredClone(device.pages || []), favRows: device.favRows ?? 1, names: { ...(device.names || {}) } };
  const here = location.hash;
  let dirty = false;
  let current = 'home';
  let query = '';
  let kind = 'all';
  let note = null;
  let dragging = null; // { from: index } inside the page, or { id } from the library

  const has = (e, cap) => !!(e && e.capabilities && cap in e.capabilities);
  const nameOf = (item) => CARDS.find(([c]) => c === item)?.[1] ?? s.names[item] ?? ent.get(item)?.name ?? 'Unavailable';
  // Only names of things still on the screen are kept.
  const shownNames = () => {
    const on = new Set([...s.tiles, ...s.more, ...s.pages.flatMap((p) => p.items), ...Object.values(s.panels).filter(Boolean)]);
    return Object.fromEntries(Object.entries(s.names).filter(([id]) => on.has(id)));
  };
  function rename(item) {
    const own = ent.get(item)?.name ?? '';
    const name = prompt(`Name on this screen (empty for "${own}")`, s.names[item] ?? own);
    if (name === null) return;
    if (name.trim() && name.trim() !== own) s.names[item] = name.trim().slice(0, 32); else delete s.names[item];
    changed();
  }
  const menuOf = (key) => MENU_PAGES.find((p) => p[0] === key);
  const ownOf = (key) => s.pages.find((p) => `own:${p.id}` === key);
  const fitsMenu = (m, item) => m[2].includes(ent.get(item)?.class);
  const pageName = (key) => key === 'home' ? 'Home' : key === 'settings' ? 'Settings' : menuOf(key)?.[1] ?? ownOf(key)?.name ?? '';

  // Where a device shows on this screen, for the library's tags.
  const usage = (item) => {
    const out = [];
    if (s.tiles.includes(item)) out.push('Home');
    const m = MENU_PAGES.find((p) => fitsMenu(p, item));
    if (m && (s.more.includes(item) || s.tiles.includes(item) || s.pages.some((p) => p.items.includes(item)))) out.push(m[1]);
    for (const p of s.pages) if (p.items.includes(item)) out.push(p.name || 'Page');
    return [...new Set(out)];
  };

  // The current page's own list, the one adding, removing and moving work on.
  const list = () => {
    if (current === 'home') return s.tiles;
    const m = menuOf(current);
    if (m) return s.more.filter((i) => fitsMenu(m, i));
    return ownOf(current)?.items ?? [];
  };
  const setList = (next) => {
    if (current === 'home') s.tiles = next;
    else if (menuOf(current)) {
      // Write the menu page's devices back into their places among the others.
      const m = menuOf(current);
      let k = 0;
      s.more = s.more.filter((i) => !fitsMenu(m, i) || next.includes(i)).map((i) => (fitsMenu(m, i) ? next[k++] : i));
      for (; k < next.length; k++) s.more.push(next[k]);
    } else if (ownOf(current)) ownOf(current).items = next;
  };
  const onPage = (item) => list().includes(item);
  const add = (item, at) => {
    const l = list();
    if (l.includes(item)) return;
    l.splice(at ?? l.length, 0, item);
    setList(l);
    changed();
  };
  const remove = (item) => { setList(list().filter((i) => i !== item)); changed(); };
  const move = (from, to) => { const l = list(); moveItem(l, from, to); setList(l); changed(); };

  const changed = () => { dirty = true; note = null; draw(); };
  const fresh = () => { draw(); };

  // Drag and drop: rows in the page reorder; library rows drop in.
  const dropOn = (el, at) => {
    el.addEventListener('dragover', (e) => { if (dragging) { e.preventDefault(); el.classList.add('over'); } });
    el.addEventListener('dragleave', () => el.classList.remove('over'));
    el.addEventListener('drop', (e) => {
      e.preventDefault();
      e.stopPropagation();
      el.classList.remove('over');
      const d = dragging;
      dragging = null;
      if (!d) return;
      const to = at ?? list().length;
      if (d.id) add(d.id, to);
      else if (d.from !== to) move(d.from, d.from < to ? to - 1 : to);
    });
  };
  const dragFrom = (el, d) => {
    el.setAttribute('draggable', 'true');
    el.addEventListener('dragstart', (e) => { dragging = d; e.dataTransfer.effectAllowed = d.id ? 'copy' : 'move'; e.dataTransfer.setData('text/plain', d.id || String(d.from)); el.classList.add('dragging'); });
    el.addEventListener('dragend', () => { dragging = null; el.classList.remove('dragging'); });
  };

  const editor = h('div', { class: 'editor' });
  const top = h('div', { class: 'ed-top' });
  const nav = h('nav', { class: 'ed-pages', 'aria-label': 'Pages on this screen' });
  const canvas = h('section', { class: 'ed-canvas' });
  const search = h('input', { type: 'search', placeholder: 'Search devices', 'aria-label': 'Search devices', oninput: () => { query = search.value.trim().toLowerCase(); drawLibrary(); } });
  const kinds = h('div', { class: 'kinds-row' });
  const libHead = h('div', { class: 'lib-head' });
  const libList = h('div', { class: 'lib-list' });
  const lib = h('aside', { class: 'lib card', 'aria-label': 'Devices' }, libHead, search, kinds, libList);
  editor.append(nav, canvas, lib);
  const openLibrary = () => { editor.classList.add('adding'); if (matchMedia('(min-width: 901px)').matches) search.focus(); };

  const save = async () => {
    try {
      if (!s.name.trim()) throw new Error('Give the screen a name in Settings.');
      if (s.pages.some((p) => !p.name.trim())) throw new Error('Every page needs a name.');
      await api('PUT', `/devices/${id}`, { name: s.name.trim(), tiles: s.tiles, more: s.more, panels: s.panels, pages: s.pages, favRows: s.favRows, names: shownNames() });
      dirty = false;
      note = h('span', { class: 'ok small' }, 'Saved. The screen reloads by itself.');
    } catch (e) { note = h('span', { class: 'bad small' }, e.message); }
    drawTop();
  };

  function drawTop() {
    top.replaceChildren(
      h('div', { class: 'ed-title' }, h('h1', {}, s.name || 'Screen'), dirty ? h('span', { class: 'unsaved small' }, 'Unsaved changes') : note),
      h('div', { class: 'row' },
        h('a', { class: 'button', href: `/?preview=${encodeURIComponent(id)}`, target: '_blank', rel: 'noopener', title: 'Opens the screen as last saved' }, 'Preview'),
        h('button', { class: 'primary', onclick: save, title: 'Save (Ctrl+S)' }, 'Save'),
        h('button', { onclick: () => (location.hash = '#/') }, 'Back')));
  }

  function drawNav() {
    const btn = (key, label, count) => h('button', { class: current === key ? 'on' : '', 'aria-current': current === key ? 'page' : false, onclick: () => { current = key; editor.classList.remove('adding'); fresh(); } },
      h('span', {}, label), count != null ? h('span', { class: 'count' }, String(count)) : null);
    const shown = (m) => [...new Set([...s.tiles, ...s.more, ...s.pages.flatMap((p) => p.items)])].filter((i) => fitsMenu(m, i)).length;
    nav.replaceChildren(...[
      btn('home', 'Home', Math.min(s.tiles.length, HOME_TILES)),
      h('div', { class: 'nav-label' }, 'Menu pages'),
      MENU_PAGES.map((m) => btn(m[0], m[1], shown(m))),
      h('div', { class: 'nav-label' }, 'Your pages'),
      s.pages.map((p) => btn(`own:${p.id}`, p.name || 'Unnamed', p.items.length)),
      h('button', { class: 'add', onclick: () => {
        if (s.pages.length >= 12) return alert('A screen has room for 12 pages.');
        const pg = { id: Math.random().toString(36).slice(2, 10), name: 'New page', icon: 'home', items: [] };
        s.pages.push(pg);
        current = `own:${pg.id}`;
        changed();
      } }, '+ Add page'),
      h('div', { class: 'nav-label' }),
      btn('settings', 'Settings'),
    ].flat());
  }

  // One row or tile in the page: drag it, or use the buttons (touch, keyboard).
  function itemEl(item, i, n, { grid = false, extra = false } = {}) {
    const e = ent.get(item);
    const sub = item.startsWith('card:') ? 'Card' : e ? [s.names[item] ? `“${e.name}”` : null, e.zone, e.class].filter(Boolean).join(' · ') : 'Not found. Removed from its integration?';
    const el = h('div', { class: `${grid ? 'tile' : 'item'}${extra ? ' extra' : ''}${e || item.startsWith('card:') ? '' : ' gone'}` },
      h('span', { class: 'grip', 'aria-hidden': 'true' }, '⋮⋮'),
      h('span', { class: 'grow' }, h('span', { class: 't-name' }, nameOf(item)), h('span', { class: 't-sub' }, extra ? 'Not shown: 6 fit' : sub)),
      h('span', { class: 'tools' },
        h('button', { class: 'chip', 'aria-label': `Move ${nameOf(item)} earlier`, onclick: () => i > 0 && move(i, i - 1) }, grid ? '←' : '↑'),
        h('button', { class: 'chip', 'aria-label': `Move ${nameOf(item)} later`, onclick: () => i < n - 1 && move(i, i + 1) }, grid ? '→' : '↓'),
        e ? h('button', { class: 'chip', title: 'Rename on this screen', 'aria-label': `Rename ${nameOf(item)}`, onclick: () => rename(item) }, '✎') : null,
        h('button', { class: 'chip danger', 'aria-label': `Remove ${nameOf(item)}`, onclick: () => remove(item) }, '✕')));
    dragFrom(el, { from: i });
    dropOn(el, i);
    return el;
  }

  // A fixed row: on this page because it is elsewhere on the screen.
  function fixedEl(item, where) {
    return h('div', { class: 'item fixed' }, h('span', { class: 'grow' }, h('span', { class: 't-name' }, nameOf(item)), h('span', { class: 't-sub' }, `From ${where}. Change it there.`)));
  }

  function itemsEl(items) {
    const box = h('div', { class: 'items' }, items.map((item, i) => itemEl(item, i, items.length)));
    dropOn(box, null);
    return box;
  }

  const addButton = (label) => h('button', { class: 'only-mobile', onclick: openLibrary }, label);

  function panelSlot(key, title, fits, hint) {
    const groups = new Map();
    for (const e of entities.filter(fits)) groups.set(e.zone || 'No room', [...(groups.get(e.zone || 'No room') || []), e]);
    const sel = h('select', { 'aria-label': title, onchange: () => { s.panels[key] = sel.value || null; changed(); } },
      h('option', { value: '' }, '— none —'),
      [...groups].map(([zone, list]) => h('optgroup', { label: zone }, list.map((e) => h('option', { value: e.id, selected: s.panels[key] === e.id }, e.name)))));
    return h('div', { class: 'slot' }, h('div', { class: 'slot-title' }, title), sel, hint ? h('div', { class: 't-sub' }, hint) : null);
  }

  function favRowsSlot() {
    const sel = h('select', { 'aria-label': 'Favourite rows', onchange: () => { s.favRows = +sel.value; changed(); } },
      [0, 1, 2, 3].map((n) => h('option', { value: n, selected: s.favRows === n }, n ? `${n} row${n > 1 ? 's' : ''}` : 'None')));
    return h('div', { class: 'slot' }, h('div', { class: 'slot-title' }, 'Favourites'), sel, h('div', { class: 't-sub' }, '5 per row'));
  }

  function homeCanvas() {
    const temp = (e) => has(e, 'measure_temperature') && e.class !== 'weather';
    const tiles = s.tiles;
    const grid = h('div', { class: 'tiles' },
      tiles.map((item, i) => itemEl(item, i, tiles.length, { grid: true, extra: i >= HOME_TILES })),
      Array.from({ length: Math.max(0, HOME_TILES - tiles.length) }, (_, k) => {
        const el = h('button', { class: 'tile empty', 'aria-label': 'Add a tile', onclick: openLibrary }, '+');
        dropOn(el, tiles.length + k);
        return el;
      }));
    dropOn(grid, null);
    return [
      h('p', { class: 'muted small' }, 'Laid out as the screen shows it. Pick what each box shows; drag tiles to reorder them.'),
      h('div', { class: 'home-top' },
        h('div', { class: 'slot clock' }, h('div', { class: 'slot-title' }, 'Clock'), h('div', { class: 'big-time' }, '12:30'), h('div', { class: 't-sub' }, 'Always shown')),
        h('div', { class: 'slot climate' },
          panelSlot('indoor', 'Indoor', temp),
          panelSlot('outdoor', 'Outdoor', temp),
          panelSlot('wind', 'Wind', (e) => has(e, 'measure_wind_strength'), 'Only if Outdoor has no wind'))),
      panelSlot('weather', 'Forecast', (e) => e.class === 'weather'),
      h('div', { class: 'slot music-slot' },
        panelSlot('speaker', 'Music', (e) => e.class === 'speaker', h('span', {}, 'This screen\'s room. Which favourites: ', h('a', { href: '#/integrations' }, 'Integrations → Favourites'), '.')),
        favRowsSlot()),
      h('div', { class: 'section-label' }, `Tiles · ${Math.min(tiles.length, HOME_TILES)} of ${HOME_TILES}`),
      grid,
      addButton('+ Add tiles'),
    ];
  }

  function menuCanvas(m) {
    const mine = list();
    const elsewhere = [];
    for (const item of s.tiles) if (fitsMenu(m, item)) elsewhere.push([item, 'Home']);
    for (const p of s.pages) for (const item of p.items) if (fitsMenu(m, item) && !s.tiles.includes(item) && !elsewhere.some(([i]) => i === item)) elsewhere.push([item, p.name]);
    const extra = elsewhere.filter(([i]) => !mine.includes(i));
    return [
      h('p', { class: 'muted small' }, `Shows every one of the ${m[3]} on this screen: the ones added here, and those on Home and your pages. The screen hides the page while it is empty.`),
      extra.length ? [h('div', { class: 'section-label' }, 'Already on the screen'), extra.map(([i, where]) => fixedEl(i, where))] : null,
      h('div', { class: 'section-label' }, 'Added here'),
      mine.length ? null : h('p', { class: 'muted small' }, 'Nothing added here yet.'),
      itemsEl(mine),
      addButton(`+ Add ${m[1].toLowerCase()}`),
    ];
  }

  function ownCanvas(pg) {
    const name = h('input', { type: 'text', value: pg.name, maxlength: '32', 'aria-label': 'Page name', oninput: () => { pg.name = name.value; dirty = true; drawTop(); drawNav(); } });
    const icon = h('select', { 'aria-label': 'Icon', onchange: () => { pg.icon = icon.value; changed(); } }, PAGE_ICONS.map((i) => h('option', { value: i, selected: pg.icon === i }, i)));
    const cards = CARDS.filter(([c]) => !pg.items.includes(c));
    return [
      h('div', { class: 'row page-head' }, name, icon,
        h('button', { class: 'danger', onclick: () => { if (confirm(`Delete the page “${pg.name}”?`)) { s.pages = s.pages.filter((p) => p !== pg); current = 'home'; changed(); } } }, 'Delete page')),
      cards.length ? h('div', { class: 'row cards-row' }, h('span', { class: 'small muted' }, 'Add a card:'), cards.map(([c, l]) => h('button', { class: 'chip', onclick: () => add(c) }, `+ ${l}`))) : null,
      pg.items.length ? null : h('p', { class: 'muted small' }, 'An empty page. Add cards above, or devices from the list.'),
      itemsEl(pg.items),
      addButton('+ Add devices'),
    ];
  }

  function settingsCanvas() {
    const name = h('input', { type: 'text', id: 'd-name', value: s.name, maxlength: '64', oninput: () => { s.name = name.value; dirty = true; drawTop(); } });
    return [
      h('label', { for: 'd-name' }, 'Screen name'), name,
      h('p', { class: 'help' }, 'Light or dark, and the size of the clock, are chosen on the screen itself: the cog at the bottom of its menu.'),
    ];
  }

  function drawCanvas() {
    const m = menuOf(current);
    const pg = ownOf(current);
    if (!m && !pg && !['home', 'settings'].includes(current)) current = 'home';
    canvas.replaceChildren(
      h('h2', {}, pageName(current)),
      ...[current === 'home' ? homeCanvas() : current === 'settings' ? settingsCanvas() : m ? menuCanvas(m) : ownCanvas(pg)].flat(Infinity).filter(Boolean));
    lib.classList.toggle('hidden', current === 'settings');
  }

  function drawLibrary() {
    const m = menuOf(current);
    const fits = (e) => e.class !== 'weather' && (!m || m[2].includes(e.class));
    const k = KINDS.find((x) => x[0] === kind);
    const shown = entities.filter((e) => fits(e) && (m || !k[2] || k[2].includes(e.class)) && (!query || `${e.name} ${e.zone} ${e.class}`.toLowerCase().includes(query)));
    libHead.replaceChildren(h('strong', {}, `Add to ${pageName(current)}`), h('button', { class: 'only-mobile chip', onclick: () => editor.classList.remove('adding') }, 'Done'));
    kinds.replaceChildren(...(m ? [] : KINDS.map(([key, label]) => h('button', { class: `chip${kind === key ? ' on' : ''}`, onclick: () => { kind = key; drawLibrary(); } }, label))));
    let zone;
    libList.replaceChildren(...(shown.length ? shown.flatMap((e) => {
      const on = onPage(e.id);
      const tags = usage(e.id);
      // A menu page already shows what is elsewhere on the screen.
      const via = m && !on && (s.tiles.includes(e.id) || s.pages.some((p) => p.items.includes(e.id)));
      const row = h('div', { class: 'lib-row' },
        h('span', { class: 'grow' }, h('span', { class: 't-name' }, e.name), h('span', { class: 't-sub' }, e.class, tags.map((t) => h('span', { class: 'tag' }, t)))),
        via ? h('span', { class: 'small muted', title: 'Already on this page, through Home or one of your pages' }, 'Shown') : h('button', { class: `chip${on ? ' on' : ''}`, 'aria-label': `${on ? 'Remove' : 'Add'} ${e.name}`, title: on ? `Remove from ${pageName(current)}` : `Add to ${pageName(current)}`, onclick: () => (on ? remove(e.id) : add(e.id)) }, on ? '✓' : '+'));
      if (!on && !via) dragFrom(row, { id: e.id });
      const header = e.zone !== zone ? h('div', { class: 'zone' }, (zone = e.zone) || 'No room') : null;
      return header ? [header, row] : [row];
    }) : [h('p', { class: 'muted small' }, entities.length ? 'No devices match.' : 'No devices yet. Add an integration first.')]));
  }

  function draw() {
    drawTop();
    drawNav();
    drawCanvas();
    drawLibrary();
  }

  leaveGuard = () => {
    if (!dirty || confirm('Leave without saving your changes?')) return true;
    ignoreHash = true;
    location.hash = here;
    return false;
  };
  window.onbeforeunload = () => (dirty ? true : undefined);
  editor.addEventListener('keydown', (e) => { if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); save(); } });
  draw();
  render(top, editor);
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

async function favouritesView(id) {
  const [integrations, entities] = await Promise.all([api('GET', '/integrations'), api('GET', '/entities')]);
  const i = integrations.find((x) => x.id === id);
  const speaker = entities.find((e) => e.id.startsWith(`${id}:`) && e.capabilities?.arrange_favourites);
  if (!i || !speaker) return render(errorBox(new Error('No favourites here. Is the integration running?')));
  render(h('h1', {}, `${i.name}: favourites`), favouritesSection(id, speaker).slice(1), h('div', { class: 'actions' }, h('button', { onclick: () => (location.hash = '#/integrations') }, 'Back')));
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
  leaveGuard = null;
  window.onbeforeunload = null;
  main.classList.remove('wide');
  try {
    const status = await api('GET', '/status');
    document.getElementById('who').textContent = status.admin ? status.admin.name : '';
    const top = (location.hash.split('/')[1] || '').replace(/^(device|add-screen|pair)$/, '').replace(/^(integration(-info)?|favourites)$/, 'integrations').replace(/^invite-new$/, 'people');
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
    if (view === 'favourites' && arg) return await favouritesView(arg);
    if (view === 'people') return await peopleView();
    if (view === 'settings') return await settingsView();
    return await screensView();
  } catch (e) {
    render(errorBox(e));
  }
}

window.addEventListener('hashchange', () => {
  if (ignoreHash) { ignoreHash = false; return; }
  if (leaveGuard && !leaveGuard()) return;
  route();
});
route();
