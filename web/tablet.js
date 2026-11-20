/* oldpad tablet page.
 *
 * Written for the oldest browsers we support (iOS 5+ Safari): ES5 only, no
 * fetch, no Promise, no arrow functions, no let/const, no template strings.
 * `cargo test` runs a check that rejects modern syntax in this file.
 */
(function () {
  'use strict';

  var TOKEN_KEY = 'oldpad.token';
  var THEME_KEY = 'oldpad.theme';
  var SIZE_KEY = 'oldpad.size';
  var SHIPVIEW_KEY = 'oldpad.shipview'; // radar, chart (nautical) or map     // how big the clock and temperatures are: normal, large, xl
  // ?preview=<screen id>: an admin looking at a screen from the admin page. Read only.
  var PREVIEW = (/[?&]preview=([A-Za-z0-9]+)/.exec(window.location.search) || [])[1] || null;

  // ------------------------------------------------------------- language

  var LANG = (navigator.language || 'en').toLowerCase();
  var NB = LANG.indexOf('nb') === 0 || LANG.indexOf('no') === 0 || LANG.indexOf('nn') === 0;
  var TEXTS = { nb: {
    days: ['Søndag', 'Mandag', 'Tirsdag', 'Onsdag', 'Torsdag', 'Fredag', 'Lørdag'],
    months: ['jan', 'feb', 'mar', 'apr', 'mai', 'jun', 'jul', 'aug', 'sep', 'okt', 'nov', 'des'],
    indoor: 'Inne', outdoor: 'Ute', clock: 'Klokken', tomorrow: 'I morgen', on: 'På', off: 'Av', run: 'Kjør', unavailable: 'Utilgjengelig',
    now: 'Nå', today: 'I dag', pair: 'Koble til denne skjermen', pairLead: 'Skann koden med telefonen og logg inn med passnøkkel.',
    or: 'eller åpne', code: 'Kode', waiting: 'Venter på godkjenning…', getting: 'Henter kode…', offline: 'Får ikke kontakt med serveren. Prøver igjen…',
    failed: 'Det gikk ikke.', empty: 'Ingenting her ennå. Velg innhold for skjermen i admin på telefonen.', nothing: 'Ingenting spilles', theme: 'Utseende', themeAuto: 'Automatisk', themeDay: 'Lyst', themeNight: 'Mørkt', thisScreen: 'Denne skjermen', size: 'Klokke og temperatur', sizeNormal: 'Normal', sizeLarge: 'Store', sizeXl: 'Ekstra store', reload: 'Last inn på nytt', preview: 'Forhåndsvisning: her kan du se, men ikke styre.', next: 'Neste timer', days3: 'Neste dager', rooms: 'Rom', allRooms: 'Alle rom', favourites: 'Favoritter', search: 'Søk etter musikk og radio', searching: 'Søker…', noHits: 'Fant ingenting.', ships: 'Båter i nærheten', noShips: 'Ingen båter innen', radar: 'Radar', chart: 'Sjøkart', map: 'Kart', ago: 'for % min siden', price: 'Strømpris', priceNow: 'Nå', cheapest: 'Billigst', dearest: 'Dyrest', krKwh: 'kr/kWh', noPrices: 'Ingen priser ennå', kn: 'kn', shipKinds: { fishing: 'Fiske', tug: 'Slepebåt', sailing: 'Seilbåt', pleasure: 'Fritidsbåt', fast: 'Hurtigbåt', pilot: 'Los', sar: 'Redning', passenger: 'Passasjer', cargo: 'Last', tanker: 'Tankskip', other: 'Annet' }, yes: 'Ja', no: 'Nei', close: 'Lukk', kinds: { track: 'Sanger', playlist: 'Spillelister', album: 'Album', radio: 'Radio' }
  }, en: {
    days: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
    months: ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
    indoor: 'Indoor', outdoor: 'Outdoor', clock: 'Time', tomorrow: 'Tomorrow', on: 'On', off: 'Off', run: 'Run', unavailable: 'Unavailable',
    now: 'Now', today: 'Today', pair: 'Pair this screen', pairLead: 'Scan the code with your phone and sign in with your passkey.',
    or: 'or open', code: 'Code', waiting: 'Waiting for approval…', getting: 'Getting a code…', offline: 'Cannot reach the server. Retrying…',
    failed: 'That did not work.', empty: 'Nothing here yet. Choose what this screen shows in the admin page on your phone.', nothing: 'Nothing playing', theme: 'Appearance', themeAuto: 'Automatic', themeDay: 'Light', themeNight: 'Dark', thisScreen: 'This screen', size: 'Clock and temperature', sizeNormal: 'Normal', sizeLarge: 'Large', sizeXl: 'Extra large', reload: 'Reload', preview: 'Preview: look, but no controls.', next: 'Next hours', days3: 'Coming days', rooms: 'Rooms', allRooms: 'All rooms', favourites: 'Favourites', search: 'Search music and radio', searching: 'Searching…', noHits: 'Nothing found.', ships: 'Ships nearby', noShips: 'No ships within', radar: 'Radar', chart: 'Chart', map: 'Map', ago: '% min ago', price: 'Electricity price', priceNow: 'Now', cheapest: 'Cheapest', dearest: 'Dearest', krKwh: 'kr/kWh', noPrices: 'No prices yet', kn: 'kn', shipKinds: { fishing: 'Fishing', tug: 'Tug', sailing: 'Sailing', pleasure: 'Pleasure', fast: 'High speed', pilot: 'Pilot', sar: 'Rescue', passenger: 'Passenger', cargo: 'Cargo', tanker: 'Tanker', other: 'Other' }, yes: 'Yes', no: 'No', close: 'Close', kinds: { track: 'Songs', playlist: 'Playlists', album: 'Albums', radio: 'Radio' }
  } };
  var T = TEXTS[NB ? 'nb' : 'en'];
  // Household settings from the server; these defaults hold until the first load.
  var SETTINGS = { day_start: 7, day_end: 21, theme: 'auto', language: 'auto' };
  var DECIMAL = (function () { try { return (1.5).toLocaleString().charAt(1) === ',' || NB ? ',' : '.'; } catch (e) { return '.'; } }());

  // MET Norway symbol codes → icon and words.
  var WEATHER = {
    clearsky: ['sun', 'Klart', 'Clear'], fair: ['partly', 'Lettskyet', 'Fair'], partlycloudy: ['partly', 'Delvis skyet', 'Partly cloudy'],
    cloudy: ['cloud', 'Skyet', 'Cloudy'], fog: ['fog', 'Tåke', 'Fog'],
    lightrain: ['rain', 'Lett regn', 'Light rain'], rain: ['rain', 'Regn', 'Rain'], heavyrain: ['rain', 'Kraftig regn', 'Heavy rain'],
    lightrainshowers: ['rain', 'Lette regnbyger', 'Light showers'], rainshowers: ['rain', 'Regnbyger', 'Showers'], heavyrainshowers: ['rain', 'Kraftige regnbyger', 'Heavy showers'],
    lightsleet: ['sleet', 'Lett sludd', 'Light sleet'], sleet: ['sleet', 'Sludd', 'Sleet'], heavysleet: ['sleet', 'Kraftig sludd', 'Heavy sleet'],
    lightsleetshowers: ['sleet', 'Sluddbyger', 'Sleet showers'], sleetshowers: ['sleet', 'Sluddbyger', 'Sleet showers'], heavysleetshowers: ['sleet', 'Kraftige sluddbyger', 'Heavy sleet showers'],
    lightsnow: ['snow', 'Lett snø', 'Light snow'], snow: ['snow', 'Snø', 'Snow'], heavysnow: ['snow', 'Kraftig snø', 'Heavy snow'],
    lightsnowshowers: ['snow', 'Snøbyger', 'Snow showers'], snowshowers: ['snow', 'Snøbyger', 'Snow showers'], heavysnowshowers: ['snow', 'Kraftige snøbyger', 'Heavy snow showers']
  };

  function weather(symbol) {
    var s = String(symbol || '');
    var night = s.indexOf('_night') > 0;
    var base = s.replace(/_(day|night|polartwilight)$/, '');
    if (base.indexOf('thunder') >= 0) { return { icon: 'thunder', text: NB ? 'Torden' : 'Thunder' }; }
    var w = WEATHER[base] || ['cloud', '', ''];
    var icon = w[0];
    if (night && icon === 'sun') { icon = 'moon'; }
    if (night && icon === 'partly') { icon = 'moonpartly'; }
    return { icon: icon, text: NB ? w[1] : w[2] };
  }

  // ------------------------------------------------------------- icons

  var P = {
    home: 'M3 11l9-8 9 8M5 10v10h14V10',
    music: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z',
    bulb: 'M9 18h6M10 21h4M12 3a6 6 0 0 0-4 10.5c.6.6 1 1.5 1 2.5h6c0-1 .4-1.9 1-2.5A6 6 0 0 0 12 3z',
    thermo: 'M14 14.8V4a2 2 0 0 0-4 0v10.8a4 4 0 1 0 4 0z',
    bolt: 'M13 2L4 14h7l-1 8 9-12h-7z',
    cloud: 'M7 18h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 11.5 3.3 3.3 0 0 0 7 18z',
    sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
    moon: 'M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z',
    partly: 'M8 3v1.5M3.5 8H2M4.8 4.8l1 1M12.2 4.8l-1 1M5.3 11A4 4 0 1 1 12 7M9 20h9a3.5 3.5 0 0 0 .4-7A5 5 0 0 0 9 14a3 3 0 0 0 0 6z',
    moonpartly: 'M10.5 3.5A5 5 0 1 0 5 10a4 4 0 0 1 5.5-6.5zM9 20h9a3.5 3.5 0 0 0 .4-7A5 5 0 0 0 9 14a3 3 0 0 0 0 6z',
    rain: 'M7 15h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 8.5 3.3 3.3 0 0 0 7 15zM8 18l-1 3M12 18l-1 3M16 18l-1 3',
    sleet: 'M7 15h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 8.5 3.3 3.3 0 0 0 7 15zM8 18l-1 3M12 19h.01M16 18l-1 3',
    snow: 'M7 15h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 8.5 3.3 3.3 0 0 0 7 15zM8 19h.01M12 21h.01M16 19h.01M10 18h.01M14 18h.01',
    fog: 'M4 9h16M3 13h18M5 17h14',
    thunder: 'M7 15h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 8.5 3.3 3.3 0 0 0 7 15zM13 15l-2 4h3l-2 4',
    chart: 'M3 20h18M5 16l4-5 4 3 6-8',
    radio: 'M4 9h16v11H4zM8 9l9-5M15 15a2 2 0 1 0 0-.01M7 13h3M7 16h3',
    play: 'M8 5v14l11-7z',
    pause: 'M7 5h3v14H7zM14 5h3v14h-3z',
    flame: 'M12 22a6 6 0 0 0 6-6c0-4-3-6-4-10-2 2-3 4-3 6-1-1-2-2-2-4-2 2-3 5-3 8a6 6 0 0 0 6 6z',
    plug: 'M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0zM12 17v5',
    door: 'M5 21V4a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v17M3 21h18M15 12h.01',
    clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2',
    dots: 'M5 12h.01M12 12h.01M19 12h.01',
    boat: 'M3 16h18l-2.5 4h-13zM12 3v11M12 4l6 9h-6',
    cog: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM10.3 3h3.4l.5 2.4 1.7 1 2.3-.8 1.7 2.9-1.8 1.6v2l1.8 1.6-1.7 2.9-2.3-.8-1.7 1-.5 2.4h-3.4l-.5-2.4-1.7-1-2.3.8-1.7-2.9 1.8-1.6v-2L4.1 8.5l1.7-2.9 2.3.8 1.7-1z'
  };

  function icon(name, filled) {
    var d = P[name] || P.cloud;
    return '<svg class="ic" viewBox="0 0 24 24"><path d="' + d + '" fill="' + (filled ? 'currentColor' : 'none') +
      '" stroke="currentColor" stroke-width="' + (filled ? 0 : 1.8) + '" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  }

  // ------------------------------------------------------------- state

  var token = null;
  var entities = {};          // id → entity
  var layout = { tiles: [], more: [], panels: {}, pages: [] };
  var page = 'home';
  var anchor = null;           // the speaker whose group the music page shows
  var query = '', results = null, searching = false, saved = {};
  var typing = false;          // don't redraw the search box under the keyboard
  var events = null;
  var pairTimer = null;
  var toastTimer = null;
  var mapZoom = 0;              // + and − on the ships map, on top of the zoom that fits the radius
  var sheet = null;            // the entity whose controls are open
  var holdUntil = 0;          // don't redraw under a finger on the volume slider
  var redrawPending = false;

  function $(id) { return document.getElementById(id); }

  function load(key) { try { return window.localStorage.getItem(key); } catch (e) { return null; } }
  function save(key, value) {
    try {
      if (value === null) { window.localStorage.removeItem(key); } else { window.localStorage.setItem(key, value); }
    } catch (e) { /* private browsing on old iOS */ }
  }

  function esc(s) {
    return String(s === null || s === undefined ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function num(n, digits) {
    if (typeof n !== 'number') { return '–'; }
    return n.toFixed(digits).replace('.', DECIMAL);
  }

  function pad(n) { return n < 10 ? '0' + n : String(n); }

  function toast(msg) {
    var el = $('toast');
    el.textContent = msg;
    el.className = 'toast';
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { el.className = 'toast hidden'; }, 4000);
  }

  function request(method, url, body, headers, done) {
    var x = new XMLHttpRequest();
    x.open(method, url, true);
    x.setRequestHeader('X-Oldpad', '1');
    if (token) { x.setRequestHeader('Authorization', 'Bearer ' + token); }
    var k;
    for (k in headers) { if (headers.hasOwnProperty(k)) { x.setRequestHeader(k, headers[k]); } }
    if (body !== null) { x.setRequestHeader('Content-Type', 'application/json'); }
    x.onreadystatechange = function () {
      if (x.readyState !== 4) { return; }
      var data = null;
      try { data = JSON.parse(x.responseText); } catch (e) { /* not JSON */ }
      if (x.status === 401 && token) { unpair(); return; }
      done(x.status, data);
    };
    x.send(body === null ? null : JSON.stringify(body));
  }

  // ------------------------------------------------------------- pairing

  function show(which) {
    $('pair').className = which === 'pair' ? 'pair' : 'pair hidden';
    $('app').className = which === 'app' ? '' : 'hidden';
  }

  function unpair() {
    token = null;
    save(TOKEN_KEY, null);
    if (events) { events.close(); events = null; }
    startPairing();
  }

  function startPairing() {
    clearTimeout(pairTimer);
    show('pair');
    $('pair-title').textContent = T.pair;
    $('pair-lead').textContent = T.pairLead;
    $('pair-or').textContent = T.or;
    $('pair-code-label').textContent = T.code;
    $('pair-status').textContent = T.getting;
    request('POST', '/api/pair', {}, {}, function (status, data) {
      if (status !== 200 || !data) {
        $('pair-status').textContent = (data && data.error) || T.offline;
        pairTimer = setTimeout(startPairing, 10000);
        return;
      }
      $('qr').innerHTML = data.qr;
      $('pair-url').textContent = data.url.replace(/#.*/, '');
      $('pair-code').textContent = data.code;
      $('pair-status').textContent = T.waiting;
      pollPairing(data.id, data.secret);
    });
  }

  function pollPairing(id, secret) {
    pairTimer = setTimeout(function () {
      request('GET', '/api/pair/' + encodeURIComponent(id), null, { 'X-Pair-Secret': secret }, function (status, data) {
        if (data && data.status === 'approved') {
          token = data.token;
          save(TOKEN_KEY, token);
          loadState();
        } else if ((data && data.status === 'pending') || status === 0) {
          pollPairing(id, secret);
        } else {
          startPairing();
        }
      });
    }, 2000);
  }

  // ------------------------------------------------------------- data

  function keep(e) {
    if (e && e.id) { entities[e.id] = e; return e.id; }
    return null;
  }

  function loadState() {
    request('GET', '/api/tablet' + (PREVIEW ? '?preview=' + PREVIEW : ''), null, {}, function (status, data) {
      if (status !== 200 || !data) {
        toast(T.offline);
        setTimeout(loadState, 10000);
        return;
      }
      entities = {};
      if (data.settings) {
        SETTINGS = data.settings;
        if (SETTINGS.language !== 'auto' && TEXTS[SETTINGS.language]) {
          NB = SETTINGS.language === 'nb';
          T = TEXTS[SETTINGS.language];
          DECIMAL = NB ? ',' : '.';
        }
      }
      var i, k;
      layout = { tiles: [], more: [], panels: {}, pages: [], name: data.device ? data.device.name : '', favRows: data.device ? data.device.favRows : 1 };
      for (i = 0; i < data.tiles.length; i++) { layout.tiles.push(keep(data.tiles[i])); }
      for (i = 0; i < data.more.length; i++) { layout.more.push(keep(data.more[i])); }
      for (k in data.panels) { if (data.panels.hasOwnProperty(k)) { layout.panels[k] = keep(data.panels[k]); } }
      // Own pages: entities are kept like any other; cards stay as {card: name}.
      for (i = 0; i < (data.pages || []).length; i++) {
        var pg = data.pages[i], items = [], j;
        for (j = 0; j < pg.items.length; j++) { items.push(pg.items[j].card ? pg.items[j] : (keep(pg.items[j]) || { missing: true })); }
        layout.pages.push({ id: 'own-' + pg.id, name: pg.name, icon: pg.icon, items: items });
      }
      show('app');
      render();
      connectEvents();
    });
  }

  function connectEvents() {
    if (PREVIEW) { setTimeout(loadState, 10000); return; } // no live stream for a preview: refresh instead
    if (events) { events.close(); events = null; }
    if (!window.EventSource) { setTimeout(loadState, 60000); return; } // no live updates: refresh instead
    request('POST', '/api/tablet/ticket', {}, {}, function (status, data) {
      if (status !== 200 || !data) { setTimeout(connectEvents, 5000); return; }
      var es = new EventSource('/api/events?ticket=' + encodeURIComponent(data.ticket));
      events = es;
      es.addEventListener('entity', function (e) {
        var msg = JSON.parse(e.data);
        var ent = entities[msg.id];
        if (ent && ent.capabilities && ent.capabilities[msg.capability]) {
          ent.capabilities[msg.capability].value = msg.value;
          render();
        }
      }, false);
      es.addEventListener('reload', function () { loadState(); }, false);
      es.addEventListener('revoked', function () { unpair(); }, false);
      es.addEventListener('refresh', function () { window.location.reload(); }, false);
      // The ticket is single use, so let the browser's own retry fail and get a new one.
      es.onerror = function () {
        es.close();
        if (events === es) { events = null; setTimeout(connectEvents, 5000); }
      };
    });
  }

  function cap(e, name) { return e && e.capabilities && e.capabilities[name] ? e.capabilities[name] : null; }
  function val(e, name) { var c = cap(e, name); return c ? c.value : null; }
  function favRows() { return typeof layout.favRows === 'number' ? layout.favRows : 1; }
  function panel(name) { return layout.panels[name] ? entities[layout.panels[name]] : null; }

  function pool() {
    var seen = {}, out = [], lists = [layout.tiles, layout.more], i, j, id;
    for (i = 0; i < layout.pages.length; i++) { lists.push(pageIds(layout.pages[i])); }
    for (i = 0; i < lists.length; i++) {
      for (j = 0; j < lists[i].length; j++) {
        id = lists[i][j];
        if (id && !seen[id] && entities[id]) { seen[id] = true; out.push(entities[id]); }
      }
    }
    return out;
  }

  function pageIds(pg) {
    var ids = [], i;
    for (i = 0; i < pg.items.length; i++) { if (typeof pg.items[i] === 'string') { ids.push(pg.items[i]); } }
    return ids;
  }

  function byClass(classes) {
    var all = pool(), out = [], i;
    for (i = 0; i < all.length; i++) { if (classes.indexOf(all[i]['class']) >= 0) { out.push(all[i]); } }
    return out;
  }

  // Speakers: the panel's first, then the others this screen may control.
  function speakers() {
    var sp = panel('speaker'), out = sp ? [sp] : [], all = byClass(['speaker']), i;
    for (i = 0; i < all.length; i++) { if (!sp || all[i].id !== sp.id) { out.push(all[i]); } }
    return out;
  }
  function bare(id) { return id.slice(id.indexOf(':') + 1); }
  // speaker_group holds the leading room's id within the same integration.
  function leadOf(e) { var g = val(e, 'speaker_group'); return g ? e.id.slice(0, e.id.indexOf(':') + 1) + g : e.id; }
  // What a group plays comes from its leader; a member reports the same when the leader is not on this screen.
  function leader(e) { return entities[leadOf(e)] || e; }
  function groupOf(e) {
    var all = speakers(), out = [], i;
    for (i = 0; i < all.length; i++) { if (leadOf(all[i]) === leadOf(e)) { out.push(all[i]); } }
    return out.length ? out : [e];
  }
  function groupName(rooms) {
    if (rooms.length === 1) { return rooms[0].name; }
    if (rooms.length === 2) { return rooms[0].name + ' + ' + rooms[1].name; }
    return rooms[0].name + ' + ' + (rooms.length - 1);
  }
  function anchorRoom() { return (anchor && entities[anchor]) || speakers()[0] || null; }

  // ------------------------------------------------------------- pieces

  function clockCard() {
    return '<div class="col w30"><div class="card clock"><div class="label-s">' + icon('clock') + ' ' + T.clock + '</div><div class="time" id="time"></div><span class="date" id="date"></span></div></div>';
  }

  // A slight tint by temperature: blue when cold, amber and red when warm.
  // Rooms are judged by comfort, outdoors by the weather.
  function tempCls(t, indoor) {
    if (typeof t !== 'number') { return ''; }
    if (indoor) { return t < 18 ? 't-cool' : t >= 26 ? 't-hot' : t >= 24 ? 't-warm' : ''; }
    return t < -5 ? 't-cold' : t < 5 ? 't-cool' : t >= 25 ? 't-hot' : t >= 18 ? 't-warm' : '';
  }
  function deg(t) { return '<span class="' + tempCls(t, false) + '">' + Math.round(t) + '°</span>'; }

  function climateHalf(e, label, iconName, wind, divider) {
    if (!e) { return ''; }
    var lo = val(e, 'temperature_min'), hi = val(e, 'temperature_max'), hum = val(e, 'measure_humidity'), co2 = val(e, 'measure_co2');
    var bits = [];
    if (typeof lo === 'number' && typeof hi === 'number') { bits.push(num(lo, 1) + '° / ' + num(hi, 1) + '°'); }
    if (typeof hum === 'number') { bits.push(Math.round(hum) + ' %'); }
    if (typeof co2 === 'number') { bits.push('<span class="' + (co2 < 1000 ? 'ok' : co2 < 1400 ? 'warn' : 'bad') + '">CO₂ ' + Math.round(co2) + '</span>'); }
    var w = wind ? val(wind, 'measure_wind_strength') : null;
    if (typeof w === 'number') { bits.push(Math.round(w / 3.6) + ' m/s'); }
    return '<div class="half' + (divider ? ' divider' : '') + '"><div class="label-s">' + icon(iconName) + ' ' + label + '</div>' +
      '<div class="big ' + tempCls(val(e, 'measure_temperature'), iconName === 'home') + '">' + num(val(e, 'measure_temperature'), 1) + '°</div><div class="sub">' + bits.join(' · ') + '</div></div>';
  }

  function climateCard(width) {
    var inn = panel('indoor'), out = panel('outdoor');
    if (!inn && !out) { return ''; }
    var wind = cap(out, 'measure_wind_strength') ? out : panel('wind');
    return '<div class="col ' + width + '"><div class="card climate">' +
      climateHalf(inn, T.indoor, 'home', null, false) + climateHalf(out, T.outdoor, 'cloud', wind, !!inn) + '</div></div>';
  }

  function series() {
    var f = panel('weather'), s = val(f, 'forecast');
    return s && s.length ? s : null;
  }

  function slot(cls, hour, sym, text) {
    return '<div class="slot ' + cls + '"><span class="h">' + hour + '</span>' + icon(weather(sym).icon) + ' ' + text + '</div>';
  }

  function forecastCard() {
    var f = panel('weather'), s = series();
    if (!f || !s) { return ''; }
    var now = new Date().getTime() / 1000, html = '', n = 0, i, d, w = weather(val(f, 'weather_symbol'));
    html += '<div class="slot now">' + icon(w.icon) + '<span class="sub">' + esc(w.text) + '</span></div>';
    for (i = 0; i < s.length && n < 6; i++) {
      d = new Date(s[i][0] * 1000);
      if (s[i][0] > now && d.getHours() % 2 === 0) {
        html += slot('', pad(d.getHours()), s[i][2], deg(s[i][1]));
        n++;
      }
    }
    var t = tomorrow(s);
    if (t) { html += '<div class="slot tomorrow"><span class="h">' + T.tomorrow + '</span>' + icon(weather(t.symbol).icon) + ' ' + deg(t.lo) + '/' + deg(t.hi) + '</div>'; }
    return '<div class="card forecast">' + html + '</div>';
  }

  function dayStats(s, offset) {
    var start = new Date(); start.setHours(0, 0, 0, 0); start.setDate(start.getDate() + offset);
    var from = start.getTime() / 1000, to = from + 86400, lo = null, hi = null, symbol = null, i;
    for (i = 0; i < s.length; i++) {
      if (s[i][0] >= from && s[i][0] < to) {
        if (lo === null || s[i][1] < lo) { lo = s[i][1]; }
        if (hi === null || s[i][1] > hi) { hi = s[i][1]; }
        if (new Date(s[i][0] * 1000).getHours() === 12 || symbol === null) { symbol = s[i][2]; }
      }
    }
    return lo === null ? null : { lo: lo, hi: hi, symbol: symbol, date: start };
  }

  function tomorrow(s) { return dayStats(s, 1); }

  // An image the integration serves through oldpad, by its key.
  function imgSrc(e, key) { return '/api/image/' + esc(e.id.slice(0, e.id.indexOf(':'))) + '/' + esc(key); }

  // The favourites in the admin's order; the front page shows only those marked for it.
  function favsHtml(lead, title, max, frontOnly) {
    var favs = val(lead, 'favourites') || [], html = '', i, n = 0, f;
    for (i = 0; i < favs.length && n < max; i++) {
      f = favs[i];
      if (frontOnly && f.front === false) { continue; }
      n++;
      html += '<span class="fav pic' + (f.title === title ? ' on' : '') + '" data-act="fav" data-id="' + esc(lead.id) + '" data-fav="' + esc(f.id) + '">' +
        (f.art ? '<img src="' + imgSrc(lead, f.art) + '" alt="">' : '<span class="noimg">' + esc(f.title.charAt(0)) + '</span>') + esc(f.title) + '</span>';
    }
    return html;
  }

  // A room's own volume, or with `name` 'group_volume' (on the leader) the whole group's.
  function volHtml(e, name) {
    name = name || 'volume_set';
    var vol = Math.round((val(e, name) || 0) * 100);
    return '<input type="range" min="0" max="100" step="1" value="' + vol + '" data-id="' + esc(e.id) + '" data-cap="' + name + '"><span>' + vol + '</span>';
  }

  // One room's card: what its group plays, with the room's own volume unless left out.
  // `rows` of five favourites under it; fewer when there are fewer favourites.
  function musicCard(room, rows, withVol) {
    if (!room) { return ''; }
    var lead = leader(room), favs = val(lead, 'favourites') || [], front = 0, i;
    for (i = 0; i < favs.length; i++) { if (favs[i].front !== false) { front++; } }
    rows = Math.min(rows, Math.ceil(front / 5));
    var playing = val(lead, 'speaker_playing') === true;
    var title = val(lead, 'speaker_track') || T.nothing, sub = val(lead, 'speaker_artist') || '';
    var art = val(lead, 'speaker_art');
    // A group gets the group's volume, which keeps each room's share like the Sonos app does.
    var grouped = (leadOf(room) !== room.id || groupOf(room).length > 1) && cap(lead, 'group_volume');
    var artHtml = art ? '<img src="' + imgSrc(lead, art) + '" alt="">' : icon('radio');
    return '<div class="card music' + (rows ? (rows > 1 ? ' r' + rows : '') : ' short') + (withVol ? ' sv' : '') + '"><div class="art">' + artHtml + '</div>' +
      '<div class="now-playing"><div class="sub">' + esc(groupName(groupOf(room))) + '</div><div class="np-title">' + esc(title) + '</div><div class="np-sub sub">' + esc(sub) + '</div></div>' +
      '<span class="play" data-act="play" data-id="' + esc(lead.id) + '">' + icon(playing ? 'pause' : 'play', true) + '</span>' +
      (withVol ? '<div class="vol">' + (grouped ? volHtml(lead, 'group_volume') : volHtml(room)) + '</div>' : '') +
      (rows ? '<div class="favs">' + favsHtml(lead, title, rows * 5, true) + '</div>' : '') + '</div>';
  }

  function tileIcon(e) {
    var c = e['class'];
    if (c === 'light') { return 'bulb'; }
    if (c === 'socket') { return 'plug'; }
    if (c === 'thermostat' || c === 'heater') { return 'flame'; }
    if (c === 'button') { return 'moon'; }
    if (c === 'sensor') { return 'thermo'; }
    if (c === 'speaker') { return 'music'; }
    if (c === 'garagedoor' || c === 'door' || c === 'lock') { return 'door'; }
    if (c === 'ais') { return 'boat'; }
    if (c === 'price') { return 'bolt'; }
    return 'bolt';
  }

  function tileHtml(e) {
    if (!e || e.missing) {
      return '<div class="tile missing"><div class="in"><p class="label">' + T.unavailable + '</p></div></div>';
    }
    var cls = 'tile', body = '';
    var onoff = cap(e, 'onoff'), dim = cap(e, 'dim'), target = cap(e, 'target_temperature'), measure = cap(e, 'measure_temperature');
    // A thermostat may have an on/off switch too; its setpoint matters more (the switch is in its sheet).
    if (target) {
      cls += ' thermo';
      body = '<div class="value up">' + num(target.value, 1) + '°<div class="sub">' + (onoff && onoff.value === false ? T.off + ' · ' : '') +
        (measure ? T.now + ' ' + num(measure.value, 1) + '°' : '') + '</div></div>' +
        '<div class="steps"><span class="step" data-act="step" data-step="-' + (target.step || 0.5) + '" data-id="' + esc(e.id) + '">−</span><span class="step" data-act="step" data-step="' + (target.step || 0.5) + '" data-id="' + esc(e.id) + '">+</span></div>';
    } else if (onoff) {
      if (onoff.value) { cls += ' on'; }
      body = '<div class="value">' + (onoff.value ? (dim ? Math.round(dim.value * 100) + ' %' : T.on) : T.off) + '</div>';
    } else if (cap(e, 'speaker_playing')) {
      if (val(leader(e), 'speaker_playing')) { cls += ' on'; }
      body = '<div class="value sub">' + esc(val(leader(e), 'speaker_playing') ? val(leader(e), 'speaker_track') || T.on : T.off) + '</div>';
    } else if (cap(e, 'button')) {
      body = '<div class="value sub">' + T.run + '</div>';
    } else if (measure) {
      body = '<div class="value">' + num(measure.value, 1) + '°</div>';
    } else if (cap(e, 'prices')) {
      var pn = val(e, 'price');
      body = '<div class="value ' + priceLevel(e, pn) + '">' + (typeof pn === 'number' ? num(pn, 2) : '–') + '<span class="unit"> kr</span></div>';
    } else if (cap(e, 'ships')) {
      var near = val(e, 'ships') || [];
      body = '<div class="value up">' + near.length + '<div class="sub">' + (near.length ? esc(shipName(near[0])) + ' · ' + km(near[0].km) : '') + '</div></div>';
    }
    if (controls(e).length > 1) { body += '<span class="more" data-act="more" data-id="' + esc(e.id) + '">' + icon('dots') + '</span>'; }
    return '<div class="' + cls + '" data-act="tile" data-id="' + esc(e.id) + '"><div class="in">' + icon(tileIcon(e)) +
      '<p class="label">' + esc(e.name) + '</p><p class="zone">' + esc(e.zone) + '</p>' + body + '</div></div>';
  }

  // ------------------------------------------------------------- controls sheet

  // Capabilities the sheet shows, controls first. Speakers and Flows have their own UI.
  var SHEET_FIRST = ['onoff', 'dim', 'target_temperature'];
  function controls(e) {
    var out = [], id, c, i;
    if (!e || !e.capabilities || e['class'] === 'speaker' || e['class'] === 'button') { return out; }
    for (id in e.capabilities) {
      if (!e.capabilities.hasOwnProperty(id)) { continue; }
      c = e.capabilities[id];
      // Write-only maintenance actions (reset meter, identify) don't belong on a wall screen.
      if (c.value === null || typeof c.value === 'object') { continue; }
      out.push(id);
    }
    out.sort(function (a, b) {
      var ia = SHEET_FIRST.indexOf(a), ib = SHEET_FIRST.indexOf(b);
      var ra = ia >= 0 ? ia : e.capabilities[a].settable ? 10 : 20, rb = ib >= 0 ? ib : e.capabilities[b].settable ? 10 : 20;
      return ra - rb || (a < b ? -1 : 1);
    });
    return out;
  }

  function capTitle(id, c) {
    if (c.title) { return c.title; }
    if (id === 'onoff') { return T.on; }
    var s = id.replace(/^(measure|meter|alarm)_/, '').replace(/[._]/g, ' ');
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  // 0..1 scales show as percent.
  function isPct(c) { return c.min === 0 && c.max === 1; }
  function fmt(c, v) {
    if (typeof v === 'boolean') { return v ? T.yes : T.no; }
    if (typeof v !== 'number') { return esc(String(v)); }
    if (isPct(c)) { return Math.round(v * 100) + ' %'; }
    var digits = c.step && c.step < 1 ? (c.step < 0.1 ? 2 : 1) : (Math.round(v) === v && (c.units || '').charAt(0) !== '°' ? 0 : 1);
    return num(v, digits) + (c.units ? (c.units.charAt(0) === '°' ? '' : ' ') + esc(c.units) : '');
  }

  function controlHtml(e, id) {
    var c = e.capabilities[id], d = ' data-id="' + esc(e.id) + '" data-cap="' + esc(id) + '"', ctl = '', i, v, scale, step;
    if (c.settable && c.values && c.values.length) {
      for (i = 0; i < c.values.length; i++) {
        v = c.values[i];
        ctl += '<span class="fav' + (v.id === c.value ? ' on' : '') + '" data-act="enum"' + d + ' data-value="' + esc(v.id) + '">' + esc(v.title || v.id) + '</span>';
      }
      return '<div class="ctl"><div class="ctl-t">' + esc(capTitle(id, c)) + '</div><div class="choices">' + ctl + '</div></div>';
    }
    if (c.settable && typeof c.value === 'boolean') {
      ctl = '<span class="switch' + (c.value ? ' on' : '') + '" data-act="flip"' + d + '><span></span></span>';
    } else if (c.settable && typeof c.value === 'number' && id === 'target_temperature') {
      ctl = '<span class="step" data-act="step" data-step="-' + (c.step || 0.5) + '"' + d + '>−</span><span class="ctl-v">' + fmt(c, c.value) + '</span>' +
        '<span class="step" data-act="step" data-step="' + (c.step || 0.5) + '"' + d + '>+</span>';
    } else if (c.settable && typeof c.value === 'number' && typeof c.min === 'number' && typeof c.max === 'number') {
      scale = isPct(c) ? 100 : 1;
      step = (c.step || (c.max - c.min) / 100) * scale;
      return '<div class="ctl"><div class="ctl-t">' + esc(capTitle(id, c)) + '</div><div class="ctl-r"><input type="range" min="' + c.min * scale + '" max="' + c.max * scale +
        '" step="' + step + '" value="' + c.value * scale + '" data-scale="' + scale + '"' + d + '><span>' + fmt(c, c.value) + '</span></div></div>';
    } else {
      ctl = '<span class="ctl-v' + (id.indexOf('alarm_') === 0 && c.value === true ? ' alarm' : '') + '">' + fmt(c, c.value) + '</span>';
    }
    return '<div class="ctl line"><div class="ctl-t">' + esc(capTitle(id, c)) + '</div><div class="ctl-x">' + ctl + '</div></div>';
  }

  function sheetHtml() {
    var e = sheet && entities[sheet], list = controls(e), html = '', i;
    if (e && e['class'] === 'price') { return '<div class="sheet-bg" data-act="close"><div class="sheet wide" data-act="none"><span class="x" data-act="close">✕</span>' + priceCard(e) + '</div></div>'; }
    if (!list.length) { return ''; }
    for (i = 0; i < list.length; i++) { html += controlHtml(e, list[i]); }
    return '<div class="sheet-bg" data-act="close"><div class="sheet" data-act="none">' +
      '<span class="x" data-act="close">✕</span><div class="sheet-h">' + esc(e.name) + '</div><div class="sub">' + esc(e.zone) + '</div>' + html + '</div></div>';
  }

  // ------------------------------------------------------------- electricity price

  // The hours from now on: [start, end, kr], start in seconds.
  function pricesAhead(e) {
    var all = val(e, 'prices') || [], now = new Date().getTime() / 1000, out = [], i;
    for (i = 0; i < all.length; i++) { if (all[i][1] > now) { out.push(all[i]); } }
    return out;
  }
  // Cheap or dear: where the price sits between the cheapest and dearest hour ahead.
  function priceRange(e) {
    var ahead = pricesAhead(e).slice(0, 36), lo = null, hi = null, i;
    for (i = 0; i < ahead.length; i++) {
      if (lo === null || ahead[i][2] < lo) { lo = ahead[i][2]; }
      if (hi === null || ahead[i][2] > hi) { hi = ahead[i][2]; }
    }
    return { lo: lo, hi: hi };
  }
  function priceLevel(e, p) {
    var r = priceRange(e), f;
    if (typeof p !== 'number' || r.lo === null || r.hi - r.lo < 0.05) { return 'p-mid'; }
    f = (p - r.lo) / (r.hi - r.lo);
    return f < 0.34 ? 'p-low' : f > 0.66 ? 'p-high' : 'p-mid';
  }
  function hh(sec) { return pad(new Date(sec * 1000).getHours()); }

  function priceCard(e) {
    var ahead = pricesAhead(e).slice(0, 36), max = 0, min = null, lo = null, hi = null, bars = '', i, p, h;
    if (!ahead.length) { return '<div class="card price"><div class="sub">' + T.price + '</div><p class="muted">' + T.noPrices + '</p></div>'; }
    for (i = 0; i < ahead.length; i++) {
      p = ahead[i][2];
      if (p > max) { max = p; hi = ahead[i]; }
      if (min === null || p < min) { min = p; lo = ahead[i]; }
    }
    // Bars start a little below the cheapest hour, so the differences show; the numbers say the rest.
    var floor = Math.max(0, min - (max - min) - 0.05);
    for (i = 0; i < ahead.length; i++) {
      p = ahead[i][2];
      h = new Date(ahead[i][0] * 1000).getHours();
      bars += '<span class="bar' + (i === 0 ? ' now' : '') + '"><i class="' + priceLevel(e, p) + '" data-w="' + Math.max(4, Math.round((p - floor) / (max - floor) * 100)) + '%"></i>' +
        (h % 3 === 0 ? '<em>' + pad(h) + '</em>' : '') + '</span>';
    }
    return '<div class="card price"><div class="sub">' + T.price + '</div>' +
      '<div class="price-top"><span class="big ' + priceLevel(e, ahead[0][2]) + '">' + num(ahead[0][2], 2) + '</span> <span class="sub">' + T.krKwh + '</span>' +
      '<span class="price-mm"><span>' + T.cheapest + ' <b>' + num(lo[2], 2) + '</b> ' + hh(lo[0]) + '–' + hh(lo[1]) + '</span>' +
      '<span>' + T.dearest + ' <b>' + num(hi[2], 2) + '</b> ' + hh(hi[0]) + '–' + hh(hi[1]) + '</span></span></div>' +
      '<div class="bars">' + bars + '</div></div>';
  }

  // ------------------------------------------------------------- ships

  function shipKind(code) {
    if (code === 30) { return 'fishing'; }
    if (code === 31 || code === 32 || code === 52) { return 'tug'; }
    if (code === 36) { return 'sailing'; }
    if (code === 37) { return 'pleasure'; }
    if (code >= 40 && code < 50) { return 'fast'; }
    if (code === 50) { return 'pilot'; }
    if (code === 51) { return 'sar'; }
    if (code >= 60 && code < 70) { return 'passenger'; }
    if (code >= 70 && code < 80) { return 'cargo'; }
    if (code >= 80 && code < 90) { return 'tanker'; }
    return 'other';
  }
  function shipName(s) { return s.name || 'MMSI ' + s.mmsi; }
  function km(d) { return (d < 1 ? Math.round(d * 1000) + ' m' : num(d, d < 10 ? 1 : 0) + ' km'); }
  // The page's CSP allows no inline styles: positions ride on data attributes and place() sets them.
  function rot(deg) { return ' data-rot="' + Math.round(deg) + '"'; }
  function place(root) {
    var els = root.querySelectorAll('[data-l],[data-rot],[data-w]'), i, el, r;
    for (i = 0; i < els.length; i++) {
      el = els[i];
      if (el.getAttribute('data-w')) { el.style.height = el.getAttribute('data-w'); }
      if (el.getAttribute('data-l')) { el.style.left = el.getAttribute('data-l'); el.style.top = el.getAttribute('data-t'); }
      r = el.getAttribute('data-rot');
      if (r) { el.style.webkitTransform = el.style.transform = 'rotate(' + r + 'deg)'; }
    }
  }

  // A radar with home in the middle and north up, and the nearest ships listed beside it.
  function shipsCard(e) {
    var list = val(e, 'ships') || [], r = val(e, 'radius') || 20, dots = '', rows = '', i, s, x, y, dir, moving, kind;
    for (i = list.length - 1; i >= 0; i--) { // nearest drawn last, on top
      s = list[i];
      x = 50 + 50 * Math.sin(s.brg * Math.PI / 180) * s.km / r;
      y = 50 - 50 * Math.cos(s.brg * Math.PI / 180) * s.km / r;
      dir = typeof s.cog === 'number' ? s.cog : s.hdg;
      moving = s.kn >= 0.5 && typeof dir === 'number';
      kind = shipKind(s.type);
      dots += '<span class="ship k-' + kind + (moving ? '' : ' still') + (s.age >= 300 ? ' old' : '') + '" data-l="' + x.toFixed(1) + '%" data-t="' + y.toFixed(1) + '%">' +
        '<i' + (moving ? rot(dir) : '') + '></i>' + (i < 4 ? '<b>' + esc(shipName(s)) + '</b>' : '') + '</span>';
    }
    for (i = 0; i < list.length && i < 12; i++) {
      s = list[i];
      kind = shipKind(s.type);
      moving = s.kn >= 0.5 && typeof s.cog === 'number';
      rows += '<div class="sh' + (s.age >= 300 ? ' old' : '') + '"><span class="sw k-' + kind + '"></span><span class="sn">' + esc(shipName(s)) +
        '<span class="sub">' + T.shipKinds[kind] + (s.dest ? ' → ' + esc(s.dest) : '') + '</span></span>' +
        '<span class="sd">' + km(s.km) + '<span class="sub">' + (s.age >= 300 ? T.ago.replace('%', Math.round(s.age / 60)) : moving ? num(s.kn, 0) + ' ' + T.kn + ' <i class="arr"' + rot(s.cog) + '>↑</i>' : '—') + '</span></span></div>';
    }
    var view = load(SHIPVIEW_KEY) || 'radar';
    var head = '<div class="ships-head"><span class="sub">' + T.ships + '</span>' + choices('shipview', view, [['radar', T.radar], ['chart', T.chart], ['map', T.map]]) + '</div>';
    if (view !== 'radar' && val(e, 'home')) {
      return '<div class="card ships">' + head + mapHtml(e, list, r, view === 'chart' ? 'sjokartraster' : 'topo') + '</div>';
    }
    return '<div class="card ships">' + head + '<div class="ships-in">' +
      '<div class="radar"><span class="ring r1"></span><span class="ring r2"></span><span class="ring r3"></span>' +
      '<span class="rl">' + km(r / 3) + '</span><span class="rl l2">' + km(2 * r / 3) + '</span><span class="rl l3">' + km(r) + '</span>' +
      '<span class="n">N</span><span class="home"></span>' + dots + '</div>' +
      '<div class="ship-list">' + (rows || '<p class="muted">' + T.noShips + ' ' + km(r) + '</p>') + '</div></div></div>';
  }

  // Web Mercator pixels of a place at zoom z.
  function merc(lat, lon, z) {
    var n = 256 * Math.pow(2, z), s = Math.sin(lat * Math.PI / 180);
    return { x: (lon + 180) / 360 * n, y: (0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI)) * n };
  }

  // Kartverket's tiles around home (through oldpad: the page may only load from it), ships on top.
  function mapHtml(e, list, r, layer) {
    var i, home = val(e, 'home'), w = Math.max(300, $('page').clientWidth - 36), h = Math.max(300, Math.min(620, window.innerHeight - 150));
    // Fit home and every ship in view (zoom 0 pixels), then + and − on top of that.
    var x0 = merc(home[0], home[1], 0), b = { l: x0.x, r: x0.x, t: x0.y, b: x0.y }, q, fit;
    for (i = 0; i < list.length; i++) {
      if (typeof list[i].lat !== 'number') { continue; }
      q = merc(list[i].lat, list[i].lon, 0);
      b.l = Math.min(b.l, q.x); b.r = Math.max(b.r, q.x); b.t = Math.min(b.t, q.y); b.b = Math.max(b.b, q.y);
    }
    fit = b.r - b.l > 0 || b.b - b.t > 0 ? Math.floor(Math.log(Math.min((w - 120) / Math.max(b.r - b.l, 1e-9), (h - 80) / Math.max(b.b - b.t, 1e-9))) / Math.LN2) : 13;
    var z = Math.max(7, Math.min(15, Math.min(fit, 14) + mapZoom)), scale = Math.pow(2, z);
    var left = (b.l + b.r) / 2 * scale - w / 2, top = (b.t + b.b) / 2 * scale - h / 2;
    var iid = e.id.slice(0, e.id.indexOf(':')), html = '', tx, ty, s, p, dir, moving, kind, c;
    for (ty = Math.floor(top / 256); ty <= Math.floor((top + h) / 256); ty++) {
      for (tx = Math.floor(left / 256); tx <= Math.floor((left + w) / 256); tx++) {
        html += '<img class="tile-img" alt="" src="/api/image/' + esc(iid) + '/' + layer + '-' + z + '-' + ty + '-' + tx + '" data-l="' + Math.round(tx * 256 - left) + 'px" data-t="' + Math.round(ty * 256 - top) + 'px">';
      }
    }
    c = merc(home[0], home[1], z);
    html += '<span class="home" data-l="' + Math.round(c.x - left) + 'px" data-t="' + Math.round(c.y - top) + 'px"></span>';
    for (i = list.length - 1; i >= 0; i--) {
      s = list[i];
      if (typeof s.lat !== 'number') { continue; }
      p = merc(s.lat, s.lon, z);
      dir = typeof s.cog === 'number' ? s.cog : s.hdg;
      moving = s.kn >= 0.5 && typeof dir === 'number';
      kind = shipKind(s.type);
      html += '<span class="ship k-' + kind + (moving ? '' : ' still') + (s.age >= 300 ? ' old' : '') + '" data-l="' + Math.round(p.x - left) + 'px" data-t="' + Math.round(p.y - top) + 'px"><i' + (moving ? rot(dir) : '') + '></i>' +
        '<b>' + esc(shipName(s)) + (moving ? ' · ' + num(s.kn, 0) + ' ' + T.kn : '') + '</b></span>';
    }
    return '<div class="map ' + layer + '" data-w="' + h + 'px">' + html +
      '<span class="zoom"><span class="step" data-act="zoom" data-step="1">+</span><span class="step" data-act="zoom" data-step="-1">−</span></span>' +
      '<span class="credit">© Kartverket</span></div>';
  }

  function tilesHtml(list, big) {
    var html = '', i;
    for (i = 0; i < list.length; i++) { html += tileHtml(list[i]); }
    return list.length ? '<div class="tiles' + (big ? ' big' : '') + '">' + html + '</div>' : '';
  }

  // ------------------------------------------------------------- pages

  var PAGES = [
    { id: 'home', icon: 'home' },
    { id: 'music', icon: 'music', when: function () { return speakers().length > 0; } },
    { id: 'lights', icon: 'bulb', when: function () { return byClass(['light', 'socket']).length > 0; } },
    { id: 'climate', icon: 'thermo', when: function () { return !!(panel('indoor') || panel('outdoor')) || byClass(['thermostat', 'heater', 'sensor']).length > 0; } },
    { id: 'flows', icon: 'bolt', when: function () { return byClass(['button']).length > 0; } },
    { id: 'weather', icon: 'cloud', when: function () { return !!series(); } },
    { id: 'ships', icon: 'boat', when: function () { return byClass(['ais']).length > 0; } }
  ];

  function homePage() {
    var tiles = [], i;
    for (i = 0; i < layout.tiles.length && i < 6; i++) { tiles.push(entities[layout.tiles[i]] || { missing: true }); }
    var html = '<div class="row">' + clockCard() + climateCard('w70') + '</div>' + forecastCard() + musicCard(panel('speaker'), favRows(), true) + tilesHtml(tiles, false);
    return layout.tiles.length || panel('indoor') || panel('weather') || panel('speaker') ? html : html + '<p class="empty muted">' + T.empty + '</p>';
  }

  // Pick a group by tapping a room, tick rooms in or out, choose what plays.
  function musicPage() {
    var room = anchorRoom(), all = speakers(), lead, rows = '', i, e, inside, other;
    if (!room) { return ''; }
    lead = leadOf(room);
    for (i = 0; i < all.length; i++) {
      e = all[i];
      inside = leadOf(e) === lead;
      other = leader(e);
      rows += '<div class="rm' + (inside ? ' in' : '') + '">' +
        '<span class="chk" data-act="join" data-id="' + esc(e.id) + '">' + (inside ? '✓' : '') + '</span>' +
        '<span class="nm" data-act="pick" data-id="' + esc(e.id) + '">' + esc(e.name) +
        (inside ? '' : '<span class="sub">' + esc(val(other, 'speaker_playing') ? val(other, 'speaker_track') || '' : T.nothing) + '</span>') + '</span>' +
        (inside ? '<span class="rv">' + volHtml(e) + '</span>' : '') + '</div>';
    }
    return musicCard(room, 0, leadOf(room) !== room.id || groupOf(room).length > 1) +
      '<div class="row"><div class="col w50"><div class="card rooms"><span class="label-s">' + T.rooms + '</span>' +
      (all.length > 1 ? '<span class="all" data-act="all">' + T.allRooms + '</span>' : '') + rows + '</div></div>' +
      '<div class="col w50"><div class="card">' + searchHtml(leader(room)) + '</div></div></div>';
  }

  // The search box, then either the results or the favourites.
  function searchHtml(lead) {
    var html = '<form class="search" id="search"><input type="search" id="q" placeholder="' + T.search + '" value="' + esc(query) + '" autocomplete="off" autocorrect="off">' +
      (results || searching ? '<span class="clear" data-act="clear">×</span>' : '') + '</form>', i, r, favs, known;
    if (searching) { return html + '<p class="muted">' + T.searching + '</p>'; }
    if (!results) {
      return html + '<span class="label-s">' + T.favourites + '</span><div class="favgrid">' + favsHtml(lead, val(lead, 'speaker_track'), 100) + '</div>';
    }
    if (!results.length) { return html + '<p class="muted">' + T.noHits + '</p>'; }
    favs = val(lead, 'favourites') || [];
    for (i = 0; i < results.length; i++) {
      r = results[i];
      if (i === 0 || results[i - 1].kind !== r.kind) { html += '<div class="label-s kind">' + esc(T.kinds[r.kind] || r.kind) + '</div>'; }
      known = saved[r.id] || favs.some(function (f) { return f.title === r.title; });
      html += '<div class="hit' + (r.art ? ' pic' : '') + '">' + (r.art ? '<img src="' + imgSrc(lead, r.art) + '" alt="">' : '') + '<span class="hit-t" data-act="fav" data-id="' + esc(lead.id) + '" data-fav="' + esc(r.id) + '">' + esc(r.title) +
        '<span class="sub">' + esc(r.sub || '') + '</span></span>' +
        '<span class="star' + (known ? ' on' : '') + '" data-act="save" data-id="' + esc(lead.id) + '" data-fav="' + esc(r.id) + '">' + (known ? '★' : '☆') + '</span></div>';
    }
    return html;
  }

  function runSearch() {
    var lead = leader(anchorRoom() || {}), q = $('q') ? $('q').value : query;
    query = q.replace(/^\s+|\s+$/g, '');
    if (!query || !lead.id) { return; }
    searching = true;
    typing = false;
    if ($('q')) { $('q').blur(); }
    render();
    request('POST', '/api/tablet/search', { id: lead.id, query: query }, {}, function (status, data) {
      searching = false;
      results = status === 200 && data ? data.results : [];
      if (status !== 200) { toast((data && data.error) || T.failed); }
      render();
    });
  }

  // This screen's own choices, kept on the screen itself.
  function myTheme() { return load(THEME_KEY) || 'auto'; }

  function choices(act, current, opts) {
    var html = '', i;
    for (i = 0; i < opts.length; i++) {
      html += '<span class="fav' + (current === opts[i][0] ? ' on' : '') + '" data-act="' + act + '" data-value="' + opts[i][0] + '">' + opts[i][1] + '</span>';
    }
    return '<div class="choices">' + html + '</div>';
  }

  function settingsPage() {
    return '<div class="card"><span class="label-s">' + T.theme + '</span>' +
      choices('theme', myTheme(), [['auto', T.themeAuto], ['day', T.themeDay], ['night', T.themeNight]]) + '</div>' +
      '<div class="card"><span class="label-s">' + T.size + '</span>' +
      choices('size', load(SIZE_KEY) || 'normal', [['normal', T.sizeNormal], ['large', T.sizeLarge], ['xl', T.sizeXl]]) + '</div>' +
      '<div class="card"><span class="label-s">' + T.thisScreen + '</span><p class="big-s">' + esc(layout.name || '') + '</p>' +
      '<span class="fav" data-act="reload">' + T.reload + '</span></div>';
  }

  function weatherPage() {
    var s = series(), now = new Date().getTime() / 1000, hours = '', days = '', n = 0, i, d, st;
    for (i = 0; i < s.length && n < 12; i++) {
      if (s[i][0] > now) {
        d = new Date(s[i][0] * 1000);
        hours += slot('', pad(d.getHours()), s[i][2], deg(s[i][1]));
        n++;
      }
    }
    for (i = 0; i < 4; i++) {
      st = dayStats(s, i);
      if (st) { days += slot('', i === 0 ? T.today : T.days[st.date.getDay()], st.symbol, deg(st.lo) + ' / ' + deg(st.hi)); }
    }
    return forecastCard() + '<div class="card"><div class="label-s">' + T.next + '</div><div class="hours">' + hours + '</div></div>' +
      '<div class="card"><div class="label-s">' + T.days3 + '</div><div class="days">' + days + '</div></div>';
  }

  // An own page: cards full width, runs of entities as big tiles, in the admin's order.
  function ownPage(pg) {
    var html = '', run = [], i, it;
    function flush() { html += tilesHtml(run, true); run = []; }
    for (i = 0; i < pg.items.length; i++) {
      it = pg.items[i];
      if (typeof it === 'object' && it.card) {
        flush();
        if (it.card === 'clock') { html += '<div class="row">' + clockCard().replace('w30', 'w100') + '</div>'; }
        if (it.card === 'climate') { html += '<div class="row">' + climateCard('w100') + '</div>'; }
        if (it.card === 'forecast') { html += forecastCard(); }
        if (it.card === 'music') { html += musicCard(panel('speaker') || speakers()[0], favRows(), true); }
      } else {
        it = typeof it === 'string' ? entities[it] || { missing: true } : it;
        if (it['class'] === 'ais') { flush(); html += shipsCard(it); } else if (it['class'] === 'price') { flush(); html += priceCard(it); } else { run.push(it); }
      }
    }
    flush();
    return html || '<p class="empty muted">' + T.empty + '</p>';
  }

  function pageHtml() {
    var i;
    for (i = 0; i < layout.pages.length; i++) { if (layout.pages[i].id === page) { return ownPage(layout.pages[i]); } }
    if (page === 'music') { return musicPage(); }
    if (page === 'lights') { return tilesHtml(byClass(['light', 'socket']), true); }
    if (page === 'climate') { return '<div class="row">' + climateCard('w100') + '</div>' + tilesHtml(byClass(['thermostat', 'heater', 'sensor']), true); }
    if (page === 'flows') { return tilesHtml(byClass(['button']), true); }
    if (page === 'weather') { return weatherPage(); }
    if (page === 'ships') { return byClass(['ais']).map(shipsCard).join(''); }
    if (page === 'settings') { return settingsPage(); }
    return homePage();
  }

  function render() {
    if (typing) { return; }
    if (new Date().getTime() < holdUntil) {
      if (!redrawPending) { redrawPending = true; setTimeout(function () { redrawPending = false; render(); }, 600); }
      return;
    }
    var rail = '', i, p;
    for (i = 0; i < PAGES.length; i++) {
      p = PAGES[i];
      if (!p.when || p.when()) { rail += '<a data-act="page" data-page="' + p.id + '"' + (p.id === page ? ' class="on"' : '') + '>' + icon(p.icon) + '</a>'; }
    }
    for (i = 0; i < layout.pages.length; i++) {
      p = layout.pages[i];
      rail += '<a data-act="page" data-page="' + esc(p.id) + '" class="own' + (p.id === page ? ' on' : '') + '">' + icon(p.icon) + '<span class="rail-name">' + esc(p.name) + '</span></a>';
    }
    rail += '<a data-act="page" data-page="settings" class="cog' + (page === 'settings' ? ' on' : '') + '">' + icon('cog') + '</a>';
    $('rail').innerHTML = rail;
    $('page').innerHTML = pageHtml() + sheetHtml();
    place($('page'));
    tick();
  }

  // ------------------------------------------------------------- actions

  function set(e, capability, value, previous) {
    if (PREVIEW) { toast(T.preview); if (previous !== undefined && cap(e, capability)) { cap(e, capability).value = previous; render(); } return; }
    request('POST', '/api/tablet/set', { id: e.id, capability: capability, value: value }, {}, function (status, data) {
      if (status !== 200) {
        if (previous !== undefined && cap(e, capability)) { cap(e, capability).value = previous; render(); }
        toast((data && data.error) || T.failed);
      }
    });
  }

  function actionOf(el) {
    while (el && el !== document) {
      if (el.getAttribute && el.getAttribute('data-act')) { return el; }
      el = el.parentNode;
    }
    return null;
  }

  function onTap(ev) {
    var el = actionOf(ev.target);
    if (!el) { return; }
    var act = el.getAttribute('data-act'), id = el.getAttribute('data-id'), e = id ? entities[id] : null, c, prev, next, i, all;
    if (act === 'page') {
      page = el.getAttribute('data-page');
      sheet = null;
      render();
    } else if (act === 'more') {
      sheet = id;
      render();
    } else if (act === 'close') {
      sheet = null;
      render();
    } else if (act === 'flip' && e && (c = cap(e, el.getAttribute('data-cap')))) {
      c.value = !c.value;
      render();
      set(e, el.getAttribute('data-cap'), c.value, !c.value);
    } else if (act === 'enum' && e && (c = cap(e, el.getAttribute('data-cap')))) {
      prev = c.value;
      c.value = el.getAttribute('data-value');
      render();
      set(e, el.getAttribute('data-cap'), c.value, prev);
    } else if (act === 'step' && e && (c = cap(e, el.getAttribute('data-cap') || 'target_temperature'))) {
      prev = c.value;
      next = Math.round((prev + parseFloat(el.getAttribute('data-step'))) * 100) / 100;
      if (typeof c.min === 'number') { next = Math.max(c.min, next); }
      if (typeof c.max === 'number') { next = Math.min(c.max, next); }
      c.value = next;
      render();
      set(e, el.getAttribute('data-cap') || 'target_temperature', next, prev);
    } else if (act === 'tile' && e && cap(e, 'prices')) {
      sheet = id;
      render();
    } else if (act === 'tile' && e && cap(e, 'ships')) {
      page = 'ships';
      render();
    } else if (act === 'tile' && e && cap(e, 'target_temperature')) {
      sheet = id; // a tap shouldn't switch the heat off; the sheet has the switch
      render();
    } else if (act === 'tile' && e && (c = cap(e, 'onoff')) && c.settable) {
      c.value = !c.value;
      render();
      set(e, 'onoff', c.value, !c.value);
    } else if (act === 'tile' && e && cap(e, 'speaker_playing')) {
      e = leader(e);
      c = cap(e, 'speaker_playing');
      c.value = !c.value;
      render();
      set(e, 'speaker_playing', c.value, !c.value);
    } else if (act === 'tile' && e && cap(e, 'button')) {
      el.className += ' pressed';
      setTimeout(render, 400);
      set(e, 'button', true);
    } else if (act === 'pick' && e) {
      anchor = e.id;
      render();
    } else if (act === 'join' && e) {
      join(e);
    } else if (act === 'all') {
      for (i = 0, all = speakers(); i < all.length; i++) {
        if (leadOf(all[i]) !== leadOf(anchorRoom())) { join(all[i]); }
      }
    } else if (act === 'play' && e && (c = cap(e, 'speaker_playing'))) {
      prev = c.value;
      c.value = !prev;
      render();
      set(e, 'speaker_playing', c.value, prev);
    } else if (act === 'shipview') {
      save(SHIPVIEW_KEY, el.getAttribute('data-value'));
      mapZoom = 0;
      render();
    } else if (act === 'zoom') {
      mapZoom = Math.max(-3, Math.min(4, mapZoom + parseInt(el.getAttribute('data-step'), 10)));
      render();
    } else if (act === 'theme') {
      save(THEME_KEY, el.getAttribute('data-value'));
      render();
    } else if (act === 'size') {
      save(SIZE_KEY, el.getAttribute('data-value'));
      render();
    } else if (act === 'reload') {
      window.location.reload();
    } else if (act === 'clear') {
      results = null;
      query = '';
      render();
    } else if (act === 'save' && e && !saved[el.getAttribute('data-fav')]) {
      saved[el.getAttribute('data-fav')] = true;
      render();
      set(e, 'add_favourite', el.getAttribute('data-fav'));
    } else if (act === 'fav' && e) {
      el.className += ' on';
      set(e, 'play_favourite', el.getAttribute('data-fav'));
    }
  }

  // Tick a room into the shown group, or take it out to play on its own.
  function join(e) {
    var room = anchorRoom(), c = cap(e, 'speaker_group'), rest, i, prev;
    if (!room || !c) { return; }
    prev = c.value;
    if (leadOf(e) === leadOf(room)) {
      rest = groupOf(room);
      if (rest.length < 2) { return; } // a group needs at least one room
      if (e.id === room.id) {
        for (i = 0; i < rest.length; i++) { if (rest[i].id !== e.id) { anchor = rest[i].id; break; } }
      }
      if (leadOf(e) === e.id) {
        // The leader leaves: show the others together under a new one until the speakers say who it is.
        for (i = 0; i < rest.length; i++) { if (rest[i].id !== e.id) { cap(rest[i], 'speaker_group').value = bare(anchor === e.id ? rest[i].id : anchor); } }
      }
      c.value = bare(e.id);
      render();
      set(e, 'speaker_group', e.id, prev);
    } else {
      c.value = val(room, 'speaker_group') || bare(room.id);
      render();
      set(e, 'speaker_group', room.id, prev);
    }
  }

  // Volume, and any number the controls sheet offers; sent when the finger lets go.
  function onSlide(ev) {
    var t = ev.target, id = t && t.getAttribute && t.getAttribute('data-id'), e = id ? entities[id] : null, c, v, prev;
    var name = t && t.getAttribute && t.getAttribute('data-cap') || 'volume_set', scale = parseFloat(t && t.getAttribute && t.getAttribute('data-scale')) || 100;
    if (!e || t.type !== 'range' || !(c = cap(e, name))) { return; }
    holdUntil = new Date().getTime() + 1500;
    v = parseFloat(t.value) / scale;
    if (t.nextSibling) { t.nextSibling.textContent = t.getAttribute('data-scale') ? fmt(c, v).replace('&amp;', '&') : t.value; }
    if (ev.type === 'change') {
      prev = c.value;
      c.value = v;
      set(e, name, v, prev);
    }
  }

  // ------------------------------------------------------------- clock & theme

  function tick() {
    var d = new Date(), el = $('time');
    if (el) {
      el.textContent = pad(d.getHours()) + ':' + pad(d.getMinutes());
      $('date').textContent = T.days[d.getDay()] + ' ' + d.getDate() + (NB ? '. ' : ' ') + T.months[d.getMonth()];
    }
    // The admin's hours; a window may wrap past midnight (e.g. 22 to 6).
    var h = d.getHours(), a = SETTINGS.day_start, b = SETTINGS.day_end;
    var inWindow = a <= b ? h >= a && h < b : h >= a || h < b;
    // This screen's own choice wins over the household's.
    var theme = load(THEME_KEY) && load(THEME_KEY) !== 'auto' ? load(THEME_KEY) : SETTINGS.theme;
    var day = theme === 'day' || (theme === 'auto' && inWindow);
    var size = load(SIZE_KEY);
    document.body.className = (day ? 'day' : 'night') + (size === 'large' || size === 'xl' ? ' size-' + size : '');
  }

  document.addEventListener('click', onTap, false);
  // Old iOS sends no click for taps on plain elements, and waits 300 ms when it does:
  // treat a touch that ends where it began as the tap. A moved finger is a scroll.
  var touchAt = null;
  document.addEventListener('touchstart', function (ev) {
    var t = ev.touches[0];
    touchAt = ev.touches.length === 1 ? { x: t.clientX, y: t.clientY } : null;
  }, false);
  document.addEventListener('touchend', function (ev) {
    var t = ev.changedTouches[0];
    if (!touchAt || !actionOf(ev.target) || ev.target.tagName === 'INPUT' || Math.abs(t.clientX - touchAt.x) > 10 || Math.abs(t.clientY - touchAt.y) > 10) { return; }
    touchAt = null;
    ev.preventDefault(); // or the browser sends its own click after this
    onTap(ev);
  }, false);
  document.addEventListener('submit', function (ev) { ev.preventDefault(); runSearch(); }, false);
  document.addEventListener('focus', function (ev) { if (ev.target && ev.target.id === 'q') { typing = true; } }, true);
  document.addEventListener('blur', function (ev) {
    if (ev.target && ev.target.id === 'q') { typing = false; query = ev.target.value; setTimeout(render, 0); }
  }, true);
  document.addEventListener('input', onSlide, false);
  document.addEventListener('change', onSlide, false);
  setInterval(tick, 10000);
  // The forecast strip moves with the clock; redraw it hourly.
  setInterval(render, 3600 * 1000);
  // Long-running kiosks leak; a nightly reload also picks up new versions.
  setTimeout(function () { window.location.reload(); }, 24 * 3600 * 1000);

  tick();
  token = PREVIEW ? null : load(TOKEN_KEY);
  if (token || PREVIEW) { loadState(); } else { startPairing(); }
}());
