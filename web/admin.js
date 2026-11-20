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
  main.replaceChildren(...nodes);
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
  render(
    h('h1', {}, 'Welcome to oldpad'),
    h('p', {}, 'Create the first admin. You will sign in with a passkey (Face ID, Touch ID or your device PIN) — there is no password.'),
    h('label', { for: 'name' }, 'Your name'), name,
    h('label', { for: 'code' }, 'Setup code'), code,
    h('p', { class: 'help' }, 'Printed in the server log when oldpad starts for the first time.'),
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

async function homeView() {
  const [devices, integrations] = await Promise.all([api('GET', '/devices'), api('GET', '/integrations')]);
  render(
    h('h2', {}, 'Screens'),
    devices.length
      ? devices.map((d) => h('div', { class: 'card' },
          h('div', { class: 'row' },
            h('div', {}, h('strong', {}, d.name), h('div', { class: 'small muted' }, `Seen ${ago(d.lastSeen)} · ${d.tiles.length} tiles`)),
            h('div', { class: 'row' },
              h('button', { onclick: () => (location.hash = `#/device/${d.id}`) }, 'Tiles'),
              h('button', { class: 'danger', onclick: async () => {
                if (!confirm(`Remove ${d.name}? It will have to be paired again.`)) return;
                await api('DELETE', `/devices/${d.id}`); route();
              } }, 'Remove')))))
      : h('p', { class: 'muted' }, 'No screens yet. Open this server\'s address on a tablet and scan its code.'),
    h('h2', {}, 'Integrations'),
    integrations.map((i) => h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('div', {}, h('strong', {}, i.name), h('div', { class: 'small muted' }, i.typeName, ' · ',
          h('span', { class: i.status === 'running' ? 'ok' : i.status.startsWith('error') ? 'bad' : '' }, i.status))),
        h('div', { class: 'row' },
          h('button', { onclick: () => (location.hash = `#/integration/${i.id}`) }, 'Edit'),
          h('button', { class: 'danger', onclick: async () => {
            if (!confirm(`Remove ${i.name}? Its tiles will show as unavailable.`)) return;
            await api('DELETE', `/integrations/${i.id}`); route();
          } }, 'Remove'))))),
    h('div', { class: 'actions' }, h('button', { class: 'primary', onclick: () => (location.hash = '#/integration/new') }, 'Add integration')),
    h('h2', {}, 'Account'),
    h('div', { class: 'actions' },
      h('button', { onclick: async () => {
        try { await register({}); alert('Passkey added.'); } catch (e) { alert(e.message); }
      } }, 'Add another passkey'),
      h('button', { onclick: async () => { await api('POST', '/logout', {}); route(); } }, 'Sign out')),
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
  const chosen = [...device.tiles];
  const list = h('div');
  const draw = () => {
    let zone = null;
    list.replaceChildren(...entities.flatMap((e) => {
      const i = chosen.indexOf(e.id);
      const row = h('div', { class: i >= 0 ? 'pick sel' : 'pick', onclick: () => {
        i >= 0 ? chosen.splice(i, 1) : chosen.push(e.id);
        draw();
      } }, h('span', { class: 'n' }, i >= 0 ? String(i + 1) : ''), h('span', {}, e.name, h('span', { class: 'small muted' }, ` · ${e.class}`)));
      const header = e.zone !== zone ? h('div', { class: 'zone' }, (zone = e.zone) || 'No room') : null;
      return [header, row];
    }));
  };
  draw();
  const out = h('div');
  render(
    h('h1', {}, device.name),
    h('p', { class: 'muted' }, 'Tap in the order the tiles should appear. Six fit on an iPad screen.'),
    entities.length ? list : h('p', { class: 'notice' }, 'Nothing to show yet. Add an integration first.'),
    h('div', { class: 'actions' },
      h('button', { class: 'primary', onclick: async () => {
        try { await api('PUT', `/devices/${id}`, { tiles: chosen }); location.hash = '#/'; } catch (e) { out.replaceChildren(errorBox(e)); }
      } }, 'Save'),
      h('button', { onclick: () => (location.hash = '#/') }, 'Cancel')),
    out,
  );
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
      location.hash = '#/';
    } catch (e) { out.replaceChildren(errorBox(e)); save.disabled = false; }
  } }, 'Save');
  render(
    h('h1', {}, existing ? `Edit ${existing.name}` : kind.name),
    existing ? null : h('p', { class: 'muted' }, 'Step 2 of 2 — connection details.'),
    h('p', {}, kind.description),
    h('label', { for: 'f-name' }, 'Name'), name,
    fields,
    h('div', { class: 'actions' }, test, save, h('button', { onclick: () => (location.hash = '#/') }, 'Cancel')),
    out,
  );
}

// ------------------------------------------------------------- router

async function route() {
  try {
    const status = await api('GET', '/status');
    document.getElementById('who').textContent = status.admin ? status.admin.name : '';
    if (status.setupNeeded) return setupView();
    if (!status.admin) return loginView();
    const [, view, arg] = (location.hash || '#/').split('/');
    if (view === 'pair' && arg) return await pairView(arg);
    if (view === 'device' && arg) return await deviceView(arg);
    if (view === 'integration' && arg) return await integrationView(arg);
    return await homeView();
  } catch (e) {
    render(errorBox(e));
  }
}

window.addEventListener('hashchange', route);
route();
