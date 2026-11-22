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
  if (res.status === 403 && data.reauth) {
    await confirmPasskey();
    return api(method, path, body);
  }
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

// Backups, new admins and the like need a recent passkey. A button, because
// Safari only opens the passkey sheet from a tap.
function confirmPasskey() {
  return new Promise((resolve, reject) => {
    const out = h('div');
    const close = () => box.remove();
    const go = h('button', { class: 'primary', onclick: async () => {
      go.disabled = true;
      try { await signIn(); close(); resolve(); } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
    } }, 'Confirm with passkey');
    const cancel = h('button', { onclick: () => { close(); reject(new Error('Cancelled.')); } }, 'Cancel');
    const box = h('div', { class: 'reauth', role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Confirm it is you' },
      h('div', { class: 'reauth-card' }, h('h2', {}, 'Confirm it is you'),
        h('p', {}, 'This action needs a fresh passkey check.'), h('div', { class: 'actions' }, go, cancel), out));
    document.body.append(box);
    go.focus();
  });
}

function ago(ts) {
  const s = Date.now() / 1000 - ts;
  if (s < 120) return 'just now';
  if (s < 7200) return `${Math.round(s / 60)} min ago`;
  if (s < 172800) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} days ago`;
}

function errorBox(err) {
  return h('p', { class: 'notice bad', role: 'alert' }, err.message || String(err));
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

// ------------------------------------------------------------- shared pieces

function toast(message, bad = false) {
  let box = document.getElementById('toasts');
  if (!box) { box = h('div', { id: 'toasts' }); document.body.append(box); }
  const t = h('div', { class: `toast${bad ? ' bad' : ''}`, role: bad ? 'alert' : 'status' }, message);
  box.append(t);
  setTimeout(() => t.remove(), bad ? 7000 : 3000);
}

// A confirmation that fits the page; resolves to true or false.
function ask({ title, body, action = 'OK', danger = false }) {
  return new Promise((resolve) => {
    const before = document.activeElement;
    const key = (e) => { if (e.key === 'Escape') done(false); };
    const done = (answer) => {
      box.remove();
      document.removeEventListener('keydown', key);
      before?.focus?.();
      resolve(answer);
    };
    const no = h('button', { type: 'button', onclick: () => done(false) }, 'Cancel');
    const yes = h('button', { type: 'button', class: danger ? 'danger solid' : 'primary', onclick: () => done(true) }, action);
    const box = h('div', { class: 'reauth', role: 'alertdialog', 'aria-modal': 'true', 'aria-label': title, onclick: (e) => { if (e.target === box) done(false); } },
      h('div', { class: 'reauth-card' }, h('h2', {}, title), body ? h('p', {}, body) : null, h('div', { class: 'actions end' }, no, yes)));
    document.body.append(box);
    document.addEventListener('keydown', key);
    no.focus();
  });
}

// A button that shows it is busy and reports failure, whatever the action does.
function btn(label, fn, attrs = {}) {
  const b = h('button', { type: 'button', ...attrs, onclick: async () => {
    b.disabled = true;
    try { await fn(); } catch (e) { if (e.message !== 'Cancelled.') toast(e.message || String(e), true); }
    b.disabled = !!attrs.disabled;
  } }, label);
  return b;
}

// Redraws the current view without jumping to the top.
async function refresh() {
  const y = window.scrollY;
  await route();
  window.scrollTo(0, y);
}

function pageHead(title, sub, ...actions) {
  document.title = `${title} · oldpad`;
  return h('div', { class: 'page-top' },
    h('div', { class: 'grow' }, h('h1', {}, title), sub ? h('p', { class: 'muted small' }, sub) : null),
    actions.length ? h('div', { class: 'page-actions' }, actions) : null);
}

const back = (href, label) => h('a', { class: 'back', href }, `‹ ${label}`);
const linkButton = (label, href, cls = '', extra = {}) => h('a', { class: `button ${cls}`, href, ...extra }, label);
const field = (label, id, input, help) => h('div', { class: 'field' }, h('label', { for: id }, label), input, help ? h('p', { class: 'help' }, help) : null);
const badge = (kind, text) => h('span', { class: `badge ${kind}` }, text);
const empty = (title, text, action) => h('div', { class: 'card empty-state' }, h('h2', {}, title), h('p', { class: 'muted' }, text), action);

function moreMenu(label, ...items) {
  return h('details', { class: 'menu' }, h('summary', { 'aria-label': label, title: 'More' }, '⋯'), h('div', { class: 'menu-list' }, items));
}
document.addEventListener('click', (e) => {
  for (const m of document.querySelectorAll('details.menu[open]')) if (!m.contains(e.target) || e.target.closest('.menu-list button')) m.open = false;
});

function statusOf(status, down) {
  if (status === 'running' && down) return { kind: 'bad', text: 'Not responding', detail: 'Screens keep showing its last values and say so.' };
  if (status === 'running') return { kind: 'ok', text: 'Connected' };
  if (status === 'starting') return { kind: 'wait', text: 'Starting' };
  if (status.startsWith('error')) return { kind: 'bad', text: 'Needs attention', detail: status.replace(/^error:\s*/, '') };
  return { kind: '', text: 'Stopped' };
}

function authPage(title, lead, ...content) {
  document.title = `${title} · oldpad`;
  render(h('div', { class: 'auth' }, h('div', { class: 'card' }, h('h1', {}, title), lead ? h('p', { class: 'muted' }, lead) : null, content)));
}

const onSubmit = (fn, ...children) => h('form', { novalidate: true, onsubmit: (e) => { e.preventDefault(); fn(); } }, children);

// ------------------------------------------------------------- views

function setupView() {
  const name = h('input', { type: 'text', id: 'name', autocomplete: 'name', placeholder: 'Your name', maxlength: '64' });
  const code = h('input', { type: 'text', id: 'code', autocapitalize: 'characters', autocomplete: 'off', placeholder: 'ABCD2345' });
  const out = h('div');
  const go = h('button', { type: 'submit', class: 'primary wide-button' }, 'Create passkey');
  const submit = async () => {
    go.disabled = true;
    try {
      await register({ name: name.value, setupCode: code.value });
      route();
    } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  };
  const pairing = location.hash.startsWith('#/pair/');
  authPage('Welcome to oldpad', 'Create the first admin. You sign in with Face ID, Touch ID or your device PIN. There is no password.',
    pairing ? h('p', { class: 'notice' }, 'Create the admin account first. You will come straight back to the pairing afterwards.') : null,
    onSubmit(submit,
      field('Your name', 'name', name),
      field('Setup code', 'code', code, 'Printed in the server log when oldpad starts for the first time (kubectl logs, docker logs). This is not the code shown on a screen.'),
      h('div', { class: 'actions' }, go), out));
}

function loginView() {
  const out = h('div');
  const go = h('button', { class: 'primary wide-button', onclick: async () => {
    go.disabled = true;
    try { await signIn(); route(); } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  } }, 'Sign in with passkey');
  authPage('Sign in', 'Use Face ID, Touch ID or your device PIN.', h('div', { class: 'actions' }, go), out);
}

// A music service or similar the integration can sign in to. The sign-in page opens
// in a new tab; the server collects the token itself, so we only wait and refresh.
function accountRow(i, a) {
  const status = h('span', { class: a.connected ? 'ok' : 'muted' }, a.connected ? 'Connected' : 'Not connected');
  const go = h('button', { type: 'button', onclick: async () => {
    const tab = window.open('about:blank', '_blank'); // before awaiting, or it counts as a popup
    go.disabled = true;
    try {
      const { url } = await api('POST', `/integrations/${i.id}/connect`, { account: a.id });
      if (tab) tab.location = url; else location.href = url;
      status.textContent = 'Waiting for you to sign in…';
      for (let n = 0; n < 100; n++) {
        await new Promise((r) => setTimeout(r, 3000));
        const now = (await api('GET', '/integrations')).find((x) => x.id === i.id);
        if (now?.accounts?.find((x) => x.id === a.id)?.connected) { refresh(); return; }
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

function personCard(p, many) {
  return h('article', { class: 'card' },
    h('div', { class: 'card-head' },
      h('div', { class: 'grow' }, h('h2', { class: 'card-title' }, p.name), h('div', { class: 'small muted' }, `${p.passkeys.length} passkey${p.passkeys.length === 1 ? '' : 's'}`)),
      p.me ? badge('ok', 'You') : null),
    h('ul', { class: 'plain' }, p.passkeys.map((k) => h('li', { class: 'row account' },
      h('div', { class: 'small' }, `Added ${day(k.created)}`, h('div', { class: 'muted' }, k.lastUsed ? `Last used ${ago(k.lastUsed)}` : 'Not used yet')),
      p.passkeys.length > 1 ? btn('Remove', async () => {
        if (!await ask({ title: 'Remove this passkey?', body: 'The device it lives on can no longer sign in, and its sessions end.', action: 'Remove', danger: true })) return;
        await api('DELETE', `/passkeys/${encodeURIComponent(k.id)}`);
        toast('Passkey removed');
        await refresh();
      }, { class: 'chip danger', 'aria-label': `Remove passkey added ${day(k.created)}` }) : null))),
    h('div', { class: 'card-actions' },
      p.me ? btn('Add passkey on this device', async () => { await register({}); toast('Passkey added'); await refresh(); }) : null,
      !p.me && many ? btn('Remove person', async () => {
        if (!await ask({ title: `Remove ${p.name}?`, body: 'They can no longer sign in, and any open sessions end.', action: 'Remove', danger: true })) return;
        await api('DELETE', `/people/${p.id}`);
        toast(`${p.name} removed`);
        await refresh();
      }, { class: 'danger' }) : null));
}

function peopleSection({ people, invites }) {
  return [
    pageHead('People', 'Everyone here can manage screens and integrations.',
      btn('Sign out', async () => { await api('POST', '/logout', {}); route(); }),
      linkButton('Invite someone', '#/invite-new', 'primary')),
    invites.length ? h('div', { class: 'notice row' },
      h('span', {}, `${invites.length} open invitation${invites.length > 1 ? 's' : ''}, valid until ${new Date(Math.max(...invites) * 1000).toLocaleString()}`),
      btn('Cancel', async () => { await api('DELETE', '/invites'); toast('Invitations cancelled'); await refresh(); }, { class: 'chip', 'aria-label': 'Cancel invitations' })) : null,
    h('div', { class: 'grid' }, people.map((p) => personCard(p, people.length > 1))),
  ];
}

// A one-time link for another adult: they open it on their phone and make their own passkey.
async function inviteNewView() {
  let inv;
  try { inv = await api('POST', '/invites', {}); } catch (e) {
    if (e.message !== 'Cancelled.') toast(e.message, true);
    location.hash = '#/people';
    return;
  }
  const link = h('input', { type: 'text', id: 'invite-link', readonly: true, value: inv.url, onfocus: (e) => e.target.select() });
  render(h('div', { class: 'narrow' },
    back('#/people', 'People'),
    pageHead('Invite someone', 'The link works once, for 24 hours.'),
    h('div', { class: 'card center' },
      h('p', {}, 'Let them scan this with their phone, or send them the link. They choose a name and make their own passkey.'),
      h('img', { class: 'qr', alt: 'QR code for the invitation link', src: `data:image/svg+xml;base64,${btoa(inv.qr)}` }),
      field('Invitation link', 'invite-link', link),
      h('div', { class: 'actions center' },
        navigator.share ? btn('Share link', () => navigator.share({ title: 'oldpad', url: inv.url }).catch(() => {}), { class: 'primary' }) : null,
        btn('Copy link', async () => { await navigator.clipboard.writeText(inv.url); toast('Link copied'); }, navigator.share ? {} : { class: 'primary' }),
        linkButton('Done', '#/people')))));
}

function inviteView(code) {
  const name = h('input', { type: 'text', id: 'name', autocomplete: 'name', placeholder: 'Your name', maxlength: '64' });
  const out = h('div');
  const go = h('button', { type: 'submit', class: 'primary wide-button' }, 'Create passkey');
  const submit = async () => {
    go.disabled = true;
    try { await register({ name: name.value, invite: code }); location.hash = '#/'; route(); } catch (e) { out.replaceChildren(errorBox(e)); go.disabled = false; }
  };
  authPage('Join oldpad', 'You were invited to help run the screens in this home. You sign in with Face ID, Touch ID or your device PIN.',
    onSubmit(submit, field('Your name', 'name', name), h('div', { class: 'actions' }, go), out));
}

// Which layout a screen shows: its own, or another screen's.
function layoutChoice(d, devices) {
  const users = devices.filter((x) => x.follows === d.id);
  if (users.length) return h('p', { class: 'small muted shared' }, `Layout shared with ${users.map((x) => x.name).join(', ')}`);
  const others = devices.filter((x) => x.id !== d.id && !x.follows);
  if (!others.length) return null;
  const id = `layout-${d.id}`;
  const pick = h('select', { id, onchange: async () => {
    const follows = pick.value || null;
    const reset = () => { pick.value = d.follows || ''; };
    if (follows) {
      const leader = others.find((x) => x.id === follows);
      const yes = await ask({ title: `Use ${leader.name}'s layout?`, body: `${d.name} will show the same layout. If you stop sharing later, it keeps a copy of that layout instead of its current one.`, action: 'Use layout' });
      if (!yes) return reset();
    }
    try { await api('PUT', `/devices/${d.id}`, { follows }); toast(follows ? 'Layout shared' : 'Now has its own layout'); await refresh(); } catch (e) { toast(e.message, true); reset(); }
  } }, h('option', { value: '' }, 'Its own'),
    others.map((x) => h('option', { value: x.id, selected: d.follows === x.id }, `Same as ${x.name}`)));
  return field('Layout', id, pick);
}

function screenCard(d, devices) {
  const leader = devices.find((x) => x.id === d.follows);
  const pages = d.pages?.length || 0;
  const summary = `${d.tiles.length} tile${d.tiles.length === 1 ? '' : 's'}${pages ? ` · ${pages} page${pages > 1 ? 's' : ''}` : ''}`;
  return h('article', { class: 'card' },
    h('div', { class: 'card-head' },
      h('div', { class: 'grow' }, h('h2', { class: 'card-title' }, d.name),
        h('div', { class: 'small muted' }, leader ? `Uses ${leader.name}'s layout` : summary, d.online ? '' : ` · seen ${ago(d.lastSeen)}`)),
      badge(d.online ? 'ok' : '', d.online ? 'Online' : 'Offline')),
    layoutChoice(d, devices),
    h('div', { class: 'card-actions' },
      linkButton(leader ? 'Details' : 'Edit layout', `#/device/${d.id}`, 'primary'),
      linkButton('Preview', `/?preview=${encodeURIComponent(d.id)}`, '', { target: '_blank', rel: 'noopener' }),
      moreMenu(`More actions for ${d.name}`,
        btn('Reload screen', async () => {
          await api('POST', `/devices/${d.id}/reload`, {});
          toast(`${d.name} is reloading`);
          setTimeout(refresh, 4000);
        }, { disabled: !d.online }),
        btn('Remove screen', async () => {
          const users = devices.filter((x) => x.follows === d.id);
          const yes = await ask({ title: `Remove ${d.name}?`, body: `It will have to be paired again.${users.length ? ` ${users[0].name} takes over the shared layout.` : ''}`, action: 'Remove', danger: true });
          if (!yes) return;
          await api('DELETE', `/devices/${d.id}`);
          toast(`${d.name} removed`);
          await refresh();
        }, { class: 'danger' }))));
}

async function screensView() {
  const devices = await api('GET', '/devices');
  const online = devices.filter((d) => d.online).length;
  const add = linkButton('Add screen', '#/add-screen', 'primary');
  render(
    pageHead('Screens', devices.length ? `${online} of ${devices.length} online` : null, add),
    devices.length
      ? h('div', { class: 'grid' }, devices.map((d) => screenCard(d, devices)))
      : empty('No screens yet', 'Open oldpad on a tablet, then pair it with this device.', linkButton('Add your first screen', '#/add-screen', 'primary')),
  );
}

function integrationCard(i, entities) {
  const st = statusOf(i.status, i.down);
  const count = entities.filter((e) => e.id.startsWith(`${i.id}:`)).length;
  const hasFavs = entities.some((e) => e.id.startsWith(`${i.id}:`) && e.capabilities?.arrange_favourites);
  return h('article', { class: 'card' },
    h('div', { class: 'card-head' },
      h('div', { class: 'grow' }, h('h2', { class: 'card-title' }, h('a', { href: `#/integration-info/${i.id}` }, i.name)),
        h('div', { class: 'small muted' }, i.typeName, i.status === 'running' ? ` · ${count} device${count === 1 ? '' : 's'}` : '')),
      badge(st.kind, st.text)),
    st.detail ? h('p', { class: 'small bad detail' }, st.detail) : null,
    (i.accounts || []).map((a) => accountRow(i, a)),
    h('div', { class: 'card-actions' },
      linkButton('Details', `#/integration-info/${i.id}`, 'primary'),
      linkButton('Edit', `#/integration/${i.id}`),
      moreMenu(`More actions for ${i.name}`,
        hasFavs ? h('a', { href: `#/favourites/${i.id}` }, 'Favourites') : null,
        btn('Restart', async () => {
          await api('POST', `/integrations/${i.id}/restart`, {});
          toast(`Restarting ${i.name}`);
          setTimeout(refresh, 2500);
        }),
        btn('Remove', async () => {
          if (!await ask({ title: `Remove ${i.name}?`, body: 'Its tiles will show as unavailable on your screens.', action: 'Remove', danger: true })) return;
          await api('DELETE', `/integrations/${i.id}`);
          toast(`${i.name} removed`);
          await refresh();
        }, { class: 'danger' }))));
}

async function integrationsView() {
  const [integrations, entities] = await Promise.all([api('GET', '/integrations'), api('GET', '/entities')]);
  const trouble = integrations.filter((i) => i.down || i.status.startsWith('error')).length;
  render(
    pageHead('Integrations', integrations.length ? (trouble ? `${trouble} need${trouble === 1 ? 's' : ''} attention` : `${integrations.length} connected services`) : null,
      linkButton('Add integration', '#/integration/new', 'primary')),
    integrations.length
      ? h('div', { class: 'grid' }, integrations.map((i) => integrationCard(i, entities)))
      : empty('Nothing connected yet', 'Connect Homey, Home Assistant, Sonos, weather and more.', linkButton('Add your first integration', '#/integration/new', 'primary')),
  );
}

async function peopleView() {
  render(peopleSection(await api('GET', '/people')));
}

function updateNote(a) {
  const release = a.latest && h('a', { href: `https://github.com/bisand/oldpad/releases/tag/v${a.latest}`, target: '_blank', rel: 'noopener noreferrer' }, a.latest);
  if (a.state === 'available') return h('span', { class: 'bad' }, 'Update available: ', release);
  if (a.state === 'current') return h('span', { class: 'ok' }, 'Up to date');
  if (a.state === 'dev') return h('span', {}, 'Development build', release ? [' · latest release ', release] : '');
  return 'Could not check yet';
}

async function settingsView() {
  const info = await api('GET', '/settings');
  const st = { ...info.settings };
  const here = location.hash;
  let dirty = false;
  const changed = () => { dirty = true; save.disabled = false; saved.textContent = 'Unsaved changes'; };
  const hours = (key) => h('select', { id: `s-${key}`, onchange: (e) => { st[key] = Number(e.target.value); changed(); } },
    Array.from({ length: 24 }, (_, i) => h('option', { value: i, selected: st[key] === i }, `${String(i).padStart(2, '0')}:00`)));
  const choice = (key, options) => h('select', { id: `s-${key}`, onchange: (e) => { st[key] = e.target.value; changed(); } },
    options.map(([v, label]) => h('option', { value: v, selected: st[key] === v }, label)));
  // The hours also set when a screen's night clock may show, whatever the theme.
  const hoursRow = h('div', { class: 'two' }, field('Day starts', 's-day_start', hours('day_start')), field('Night starts', 's-day_end', hours('day_end')));
  const saved = h('span', { class: 'small muted', role: 'status' });
  const save = h('button', { type: 'button', class: 'primary', disabled: true, onclick: async () => {
    save.disabled = true;
    try {
      await api('PUT', '/settings', st);
      dirty = false;
      saved.textContent = '';
      toast('Saved. Screens reload by themselves.');
    } catch (e) { save.disabled = false; toast(e.message, true); }
  } }, 'Save');
  const file = h('input', { type: 'file', accept: 'application/json,.json', id: 'restore', class: 'hidden', onchange: async () => {
    const f = file.files[0];
    if (!f) return;
    if (f.size > 4 * 1024 * 1024) { toast('That backup is larger than the 4 MB limit.', true); file.value = ''; return; }
    try {
      const backup = JSON.parse(await f.text());
      const when = backup.exported ? new Date(backup.exported * 1000).toLocaleString() : 'an unknown date';
      if (await ask({ title: 'Restore this backup?', body: `From ${when}. Screens, integrations and settings are replaced. People in it are added.`, action: 'Restore', danger: true })) {
        await api('POST', '/restore', backup);
        toast('Restored. Screens reload by themselves.');
      }
    } catch (e) { if (e.message !== 'Cancelled.') toast(e instanceof SyntaxError ? 'That file is not an oldpad backup.' : e.message, true); }
    file.value = '';
  } });
  const download = btn('Download backup', async () => {
    const get = () => fetch('/api/admin/backup', { credentials: 'same-origin' });
    let res = await get();
    if (res.status === 403) { await confirmPasskey(); res = await get(); }
    if (!res.ok) throw new Error(`Could not make the backup (HTTP ${res.status}).`);
    const url = URL.createObjectURL(await res.blob());
    const a = h('a', { href: url, download: `oldpad-backup-${new Date().toISOString().slice(0, 10)}.json` });
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10000);
  });
  leaveGuard = () => {
    if (!dirty || confirm('Leave without saving your changes?')) return true;
    ignoreHash = true;
    location.hash = here;
    return false;
  };
  window.onbeforeunload = () => (dirty ? true : undefined);
  render(
    pageHead('Settings', 'For all screens in this home.'),
    h('div', { class: 'grid settings' },
      h('section', { class: 'card' },
        h('h2', { class: 'card-title' }, 'Appearance'),
        field('Theme', 's-theme', choice('theme', [['auto', 'Light by day, dark at night'], ['sun', 'Follow sunrise and sunset (screens with a MET forecast)'], ['day', 'Always light'], ['night', 'Always dark']])),
        hoursRow,
        field('Language', 's-language', choice('language', [['auto', 'Each screen\'s own language'], ['nb', 'Norsk'], ['en', 'English']])),
        h('div', { class: 'card-actions' }, save, saved)),
      h('section', { class: 'card' },
        h('h2', { class: 'card-title' }, 'Backup'),
        h('p', { class: 'muted small' }, 'Screens, layouts, integrations, people and settings in one file.'),
        h('p', { class: 'notice small' }, 'The file contains integration secrets, such as the Homey key. Keep it somewhere private.'),
        h('div', { class: 'card-actions' }, download, btn('Restore…', () => file.click()), file)),
      h('section', { class: 'card' },
        h('h2', { class: 'card-title' }, 'About'),
        h('dl', { class: 'facts' },
          h('dt', {}, 'Version'), h('dd', {}, `${info.about.version}${info.about.commit ? ` (${info.about.commit})` : ''}`),
          h('dt', {}, 'Updates'), h('dd', {}, updateNote(info.about)),
          h('dt', {}, 'Address'), h('dd', {}, info.publicUrl),
          h('dt', {}, 'Screens'), h('dd', {}, String(info.counts.screens)),
          h('dt', {}, 'Integrations'), h('dd', {}, String(info.counts.integrations)),
          h('dt', {}, 'People'), h('dd', {}, String(info.counts.people))))),
  );
}

// Pairing starts on the new screen; this explains how and takes the code it shows.
function addScreenView() {
  const code = h('input', { type: 'text', id: 'p-code', maxlength: '8', autocapitalize: 'characters', autocomplete: 'off', placeholder: 'ABCD2345' });
  const go = () => { const c = code.value.trim().toUpperCase(); if (c) location.hash = `#/pair/${encodeURIComponent(c)}`; else code.focus(); };
  render(h('div', { class: 'narrow' },
    back('#/', 'Screens'),
    pageHead('Add screen'),
    h('div', { class: 'card' },
      h('ol', { class: 'steps' },
        h('li', {}, 'On the tablet, open ', h('strong', { class: 'break' }, location.origin), '.', h('p', { class: 'help' }, 'An iPad on iOS 9 or older needs the ISRG Root X1 certificate installed first.')),
        h('li', {}, 'Scan the QR code it shows with this phone, or type the code below it.')),
      onSubmit(go, field('Code from the screen', 'p-code', code),
        h('div', { class: 'actions' }, h('button', { type: 'submit', class: 'primary' }, 'Continue'), linkButton('Cancel', '#/'))))));
}

async function pairView(code) {
  const { status } = await api('GET', `/pair/${encodeURIComponent(code)}`);
  if (status !== 'pending') {
    document.title = 'Pair a screen · oldpad';
    render(h('div', { class: 'narrow' }, back('#/', 'Screens'), pageHead('Pair a screen'), h('div', { class: 'card' },
      h('p', {}, status === 'approved' ? 'This screen is already paired.' : 'This code has expired. Start again on the screen.'),
      h('div', { class: 'actions' }, linkButton('Enter another code', '#/add-screen', 'primary')))));
    return;
  }
  const devices = await api('GET', '/devices');
  const leaders = devices.filter((d) => !d.follows);
  let kind = 'new';
  const name = h('input', { type: 'text', id: 'name', placeholder: 'Kitchen', maxlength: '64', oninput: () => check() });
  const naming = field('Name this screen', 'name', name, 'Usually the room it hangs in.');
  const pin = h('input', { type: 'text', id: 'pin', class: 'pin', inputmode: 'numeric', autocomplete: 'off', maxlength: '6', placeholder: '000000',
    oninput: () => { pin.value = pin.value.replace(/\D/g, ''); check(); } });
  const pickFollow = h('select', { id: 'follow' }, leaders.map((d) => h('option', { value: d.id }, d.name)));
  const pickReplace = h('select', { id: 'replace' }, devices.map((d) => h('option', { value: d.id }, d.name)));
  const followField = field('Use the layout of', 'follow', pickFollow, 'Changes to that layout appear on both screens.');
  const replaceField = field('Screen to replace', 'replace', pickReplace, 'Keeps its name and layout. Its previous sign-in stops working immediately.');
  const out = h('div');
  const go = h('button', { type: 'submit', class: 'primary wide-button', disabled: true }, 'Approve screen');
  const check = () => { go.disabled = !/^\d{6}$/.test(pin.value) || (kind !== 'replace' && !name.value.trim()); };
  const show = () => {
    followField.classList.toggle('hidden', kind !== 'follow');
    replaceField.classList.toggle('hidden', kind !== 'replace');
    naming.classList.toggle('hidden', kind === 'replace');
    check();
  };
  const option = (value, title, text) => h('label', { class: 'choice' },
    h('input', { type: 'radio', name: 'pair-kind', value, checked: value === 'new', onchange: () => { kind = value; show(); } }),
    h('span', {}, h('strong', {}, title), h('span', { class: 'small muted' }, text)));
  const submit = async () => {
    if (go.disabled) return;
    go.disabled = true;
    try {
      const body = kind === 'replace' ? { replace: pickReplace.value, pin: pin.value }
        : { name: name.value, pin: pin.value, ...(kind === 'follow' ? { follow: pickFollow.value } : {}) };
      const { id } = await api('POST', `/pair/${encodeURIComponent(code)}`, body);
      toast('Screen paired');
      location.hash = kind === 'new' ? `#/device/${id}` : '#/';
    } catch (e) {
      if (e.message !== 'Cancelled.') out.replaceChildren(errorBox(e));
      check();
    }
  };
  document.title = 'Pair a screen · oldpad';
  render(h('div', { class: 'narrow' },
    back('#/', 'Screens'),
    pageHead('Pair a screen'),
    h('div', { class: 'notice' }, h('strong', {}, 'Is the screen in front of you?'), ' If someone sent you this link, cancel.'),
    h('div', { class: 'card' }, onSubmit(submit,
      h('p', { class: 'small muted' }, 'Screen code'), h('p', { class: 'code' }, code.toUpperCase()),
      field('Number shown on the screen', 'pin', pin),
      h('fieldset', { class: 'choices' }, h('legend', {}, 'What is this screen?'),
        option('new', 'A new screen', 'Starts with an empty layout'),
        leaders.length ? option('follow', 'A new screen, same layout', 'Shares a layout you already made') : null,
        devices.length ? option('replace', 'A replacement', 'Takes over an existing screen') : null),
      followField, replaceField, naming,
      h('div', { class: 'actions' }, go, linkButton('Cancel', '#/')), out))));
  show();
  pin.focus();
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
  ['calendar', 'Calendar', ['calendar'], 'calendars'],
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
  if (device.follows) {
    const leader = devices.find((d) => d.id === device.follows);
    const title = h('input', { type: 'text', id: 'name', value: device.name, maxlength: '64' });
    return render(h('div', { class: 'narrow' },
      back('#/', 'Screens'),
      pageHead(device.name, 'Shares a layout'),
      h('p', { class: 'notice' }, `This screen shows the layout of ${leader?.name ?? 'another screen'}. Changes there appear here too.`),
      h('div', { class: 'card' },
        field('Screen name', 'name', title),
        h('div', { class: 'card-actions' },
          btn('Save name', async () => { await api('PUT', `/devices/${id}`, { name: title.value }); toast('Saved'); await refresh(); }, { class: 'primary' }),
          linkButton('Edit shared layout', `#/device/${device.follows}`),
          btn('Stop sharing', async () => {
            if (!await ask({ title: 'Stop sharing?', body: 'This screen keeps a copy of the layout, and later changes no longer reach it.', action: 'Stop sharing' })) return;
            await api('PUT', `/devices/${id}`, { follows: null });
            toast('Now has its own layout');
            await refresh();
          })))));
  }
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
      h('div', { class: 'slot door' },
        panelSlot('doorbell', 'Doorbell', (e) => Object.keys(e.capabilities || {}).some((c) => c.startsWith('alarm_')), 'Shows a pop-up when it goes off. A motion sensor works too.'),
        panelSlot('door_camera', 'Doorbell camera', (e) => has(e, 'camera'), 'Optional')),
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
  if (!i) return render(back('#/integrations', 'Integrations'), errorBox(new Error('No such integration.')));
  const st = statusOf(i.status, i.down);
  const mine = entities.filter((e) => e.id.startsWith(`${id}:`));
  const usedOn = (eid) => devices.filter((d) => d.tiles.includes(eid) || d.more.includes(eid) || Object.values(d.panels).includes(eid) || (d.pages || []).some((p) => p.items.includes(eid))).map((d) => d.name);
  const filter = h('input', { type: 'search', placeholder: `Filter ${mine.length} devices`, 'aria-label': 'Filter devices' });
  const list = h('div', { class: 'device-list' });
  const draw = () => {
    const q = filter.value.trim().toLowerCase();
    let zone = null;
    const shown = mine.filter((e) => !q || `${e.name} ${e.zone} ${e.class}`.toLowerCase().includes(q));
    list.replaceChildren(...(shown.length ? shown.flatMap((e) => {
      const caps = Object.entries(e.capabilities || {}).filter(([, c]) => c.value !== null && typeof c.value !== 'object').map(([k, c]) => `${k.replace(/_/g, ' ')} ${showValue(c.value)}`);
      const used = usedOn(e.id);
      const row = h('div', { class: 'info-row' },
        h('div', { class: 'row' }, h('strong', {}, e.name), h('span', { class: 'small muted' }, e.class)),
        h('div', { class: 'small muted' }, caps.join(' · ') || 'No values'),
        used.length ? h('div', { class: 'small ok' }, `On ${used.join(', ')}`) : null);
      const header = e.zone !== zone ? h('div', { class: 'zone' }, (zone = e.zone) || 'No room') : null;
      return header ? [header, row] : [row];
    }) : [h('p', { class: 'muted' }, 'No devices match.')]));
  };
  filter.addEventListener('input', draw);
  draw();
  const favSpeaker = mine.find((e) => e.capabilities?.arrange_favourites);
  render(
    back('#/integrations', 'Integrations'),
    pageHead(i.name, i.typeName,
      btn('Restart', async () => {
        await api('POST', `/integrations/${id}/restart`, {});
        toast(`Restarting ${i.name}`);
        setTimeout(refresh, 2500);
      }),
      linkButton('Edit settings', `#/integration/${id}`, 'primary')),
    h('p', {}, badge(st.kind, st.text)),
    st.detail ? h('p', { class: 'notice bad' }, st.detail) : null,
    (i.accounts || []).length ? [h('h2', {}, 'Accounts'), h('div', { class: 'card' }, i.accounts.map((a) => accountRow(i, a)))] : null,
    favSpeaker ? favouritesSection(id, favSpeaker) : null,
    h('h2', {}, 'Devices'),
    mine.length ? [filter, list] : h('p', { class: 'muted' }, i.status === 'running' ? 'This integration has no devices.' : 'Devices show up here once it is running.'),
  );
}

// The household's favourites (Sonos): oldpad's order, which go on the front page, and removing.
function favouritesSection(iid, speaker) {
  let favs = speaker.capabilities.favourites.value.map((f) => ({ ...f }));
  const list = h('div', { class: 'card' });
  const save = async () => {
    draw();
    try {
      await api('POST', '/set', { id: speaker.id, capability: 'arrange_favourites', value: { order: favs.map((f) => f.id), front: favs.filter((f) => f.front).map((f) => f.id) } });
    } catch (e) { toast(e.message, true); }
  };
  const move = (i, by) => { const [f] = favs.splice(i, 1); favs.splice(i + by, 0, f); save(); };
  const draw = () => list.replaceChildren(...(favs.length ? favs.map((f, i) => h('div', { class: 'fav-row' },
    f.art ? h('img', { src: `/api/image/${iid}/${f.art}`, alt: '' }) : h('span', { class: 'noart' }, f.title.slice(0, 1)),
    h('span', { class: 'grow' }, f.title),
    h('button', { type: 'button', class: `chip${f.front ? ' on' : ''}`, title: 'Show on the front page', 'aria-pressed': String(f.front), onclick: () => { f.front = !f.front; save(); } }, 'Front'),
    h('button', { type: 'button', class: 'chip', 'aria-label': `Move ${f.title} up`, disabled: i === 0, onclick: () => move(i, -1) }, '↑'),
    h('button', { type: 'button', class: 'chip', 'aria-label': `Move ${f.title} down`, disabled: i === favs.length - 1, onclick: () => move(i, 1) }, '↓'),
    h('button', { type: 'button', class: 'chip danger', 'aria-label': `Remove ${f.title}`, onclick: async () => {
      if (!await ask({ title: `Remove “${f.title}”?`, body: 'This removes it from Sonos favourites in the Sonos app too.', action: 'Remove', danger: true })) return;
      try {
        await api('POST', '/set', { id: speaker.id, capability: 'remove_favourite', value: f.id });
        favs = favs.filter((x) => x !== f);
        draw();
      } catch (e) { toast(e.message, true); }
    } }, '✕'))) : [h('p', { class: 'muted' }, 'No favourites yet. Star music or radio in a screen\'s search to add some.')]));
  draw();
  return [
    h('h2', {}, 'Favourites'),
    h('p', { class: 'muted small' }, 'The front page shows the ones marked Front, in this order. The music page shows them all.'),
    h('div', { class: 'actions tight' }, h('button', { type: 'button', class: 'chip', onclick: () => { favs.sort((a, b) => a.title.localeCompare(b.title)); save(); } }, 'Sort A–Z')),
    list,
  ];
}

async function favouritesView(id) {
  const [integrations, entities] = await Promise.all([api('GET', '/integrations'), api('GET', '/entities')]);
  const i = integrations.find((x) => x.id === id);
  const speaker = entities.find((e) => e.id.startsWith(`${id}:`) && e.capabilities?.arrange_favourites);
  if (!i || !speaker) return render(back('#/integrations', 'Integrations'), errorBox(new Error('No favourites here. Is the integration running?')));
  render(h('div', { class: 'narrow' }, back(`#/integration-info/${id}`, i.name), pageHead('Favourites', i.name), favouritesSection(id, speaker).slice(1)));
}

// The integration wizard: 1. pick a type, 2. fill in its fields, 3. test, 4. save.
async function integrationView(id) {
  const kinds = await api('GET', '/kinds');
  const all = await api('GET', '/integrations');
  const existing = id === 'new' ? null : all.find((i) => i.id === id);
  if (id !== 'new' && !existing) return render(back('#/integrations', 'Integrations'), errorBox(new Error('No such integration.')));
  if (!existing) {
    render(
      back('#/integrations', 'Integrations'),
      pageHead('Add integration', 'Step 1 of 2: what do you want to connect?'),
      h('div', { class: 'grid' }, kinds.map((k) => {
        const added = all.filter((i) => i.type === k.id);
        return h('button', { type: 'button', class: `card kind${added.length ? ' added' : ''}`, onclick: () => integrationForm(k, null) },
          h('span', { class: 'kind-head' }, h('strong', {}, k.name), added.length ? badge('ok', added.length > 1 ? `${added.length} added` : 'Added') : null),
          h('span', { class: 'small muted' }, k.description),
          added.length ? h('span', { class: 'small kind-names' }, `${added.map((i) => i.name).join(', ')} · Add another`) : null);
      })),
    );
  } else {
    integrationForm(kinds.find((k) => k.id === existing.type), existing);
  }
}

function integrationForm(kind, existing) {
  const inputs = {};
  const here = location.hash;
  let dirty = false;
  const name = h('input', { type: 'text', id: 'f-name', value: existing ? existing.name : kind.name, maxlength: '64' });
  const fields = kind.fields.map((f) => {
    const type = { secret: 'password', url: 'url', number: 'number', text: 'text' }[f.kind] || 'text';
    const current = existing ? existing.config[f.key] : undefined;
    const label = f.label + (f.required ? '' : ' (optional)');
    if (f.kind === 'bool') {
      const box = h('input', { type: 'checkbox', id: `f-${f.key}`, checked: current === true });
      inputs[f.key] = box;
      return h('div', { class: 'field' }, h('label', { class: 'check' }, box, label), f.help ? h('p', { class: 'help' }, f.help) : null);
    }
    const input = h('input', {
      type, id: `f-${f.key}`, autocomplete: f.kind === 'secret' ? 'new-password' : 'off', spellcheck: 'false',
      step: f.kind === 'number' ? 'any' : null,
      placeholder: f.kind === 'secret' && current ? 'Saved. Leave empty to keep it' : f.placeholder,
      value: f.kind === 'secret' || current == null ? '' : String(current),
    });
    inputs[f.key] = input;
    if (f.kind !== 'secret') return field(label, `f-${f.key}`, input, f.help);
    const reveal = h('button', { type: 'button', class: 'chip', 'aria-pressed': 'false', 'aria-label': `Show ${f.label}`, onclick: () => {
      const showing = input.type === 'text';
      input.type = showing ? 'password' : 'text';
      reveal.textContent = showing ? 'Show' : 'Hide';
      reveal.setAttribute('aria-pressed', String(!showing));
    } }, 'Show');
    return field(label, `f-${f.key}`, h('div', { class: 'with-button' }, input, reveal), f.help);
  });
  const config = () => Object.fromEntries(kind.fields.map((f) => [f.key, f.kind === 'bool' ? inputs[f.key].checked : inputs[f.key].value]));
  const out = h('div', { role: 'status' });
  const test = h('button', { type: 'button', onclick: async () => {
    test.disabled = true;
    out.replaceChildren(h('p', { class: 'notice' }, 'Testing the connection…'));
    try {
      const r = await api('POST', '/integrations/test', { type: kind.id, id: existing?.id, config: config() });
      out.replaceChildren(h('p', { class: `notice ${r.ok ? 'ok' : 'bad'}` }, r.message));
    } catch (e) { out.replaceChildren(e.message === 'Cancelled.' ? '' : errorBox(e)); }
    test.disabled = false;
  } }, 'Test');
  const save = h('button', { type: 'button', class: 'primary', onclick: async () => {
    save.disabled = true;
    try {
      const body = { type: kind.id, name: name.value, config: config() };
      if (existing) await api('PUT', `/integrations/${existing.id}`, body);
      else await api('POST', '/integrations', body);
      dirty = false;
      toast('Saved');
      location.hash = '#/integrations';
    } catch (e) { if (e.message !== 'Cancelled.') out.replaceChildren(errorBox(e)); save.disabled = false; }
  } }, existing ? 'Save' : 'Add');
  leaveGuard = () => {
    if (!dirty || confirm('Leave without saving your changes?')) return true;
    ignoreHash = true;
    location.hash = here;
    return false;
  };
  window.onbeforeunload = () => (dirty ? true : undefined);
  const title = existing ? `Edit ${existing.name}` : kind.name;
  const form = h('div', { class: 'card', oninput: () => { dirty = true; } }, field('Name', 'f-name', name), fields);
  render(h('div', { class: 'narrow' },
    back(existing ? `#/integration-info/${existing.id}` : '#/integrations', existing ? existing.name : 'Integrations'),
    pageHead(title, existing ? kind.name : 'Step 2 of 2: connection details'),
    h('p', { class: 'muted' }, kind.description),
    form, out,
    h('div', { class: 'form-bar' }, test, h('span', { class: 'grow' }), linkButton('Cancel', '#/integrations'), save)));
}

// ------------------------------------------------------------- router

async function route() {
  leaveGuard = null;
  window.onbeforeunload = null;
  main.classList.remove('wide');
  document.title = 'oldpad';
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
