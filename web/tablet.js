/* oldpad tablet page.
 *
 * Written for the oldest browsers we support (iOS 5+ Safari): ES5 only, no
 * fetch, no Promise, no arrow functions, no let/const, no template strings.
 * `cargo test` runs a check that rejects modern syntax in this file.
 */
(function () {
  'use strict';

  var TOKEN_KEY = 'oldpad.token';
  // ?preview=<screen id>: an admin looking at a screen from the admin page. Read only.
  var PREVIEW = (/[?&]preview=([A-Za-z0-9]+)/.exec(window.location.search) || [])[1] || null;

  // ------------------------------------------------------------- language

  var LANG = (navigator.language || 'en').toLowerCase();
  var NB = LANG.indexOf('nb') === 0 || LANG.indexOf('no') === 0 || LANG.indexOf('nn') === 0;
  var T = NB ? {
    days: ['Søndag', 'Mandag', 'Tirsdag', 'Onsdag', 'Torsdag', 'Fredag', 'Lørdag'],
    months: ['jan', 'feb', 'mar', 'apr', 'mai', 'jun', 'jul', 'aug', 'sep', 'okt', 'nov', 'des'],
    indoor: 'Inne', outdoor: 'Ute', tomorrow: 'I morgen', on: 'På', off: 'Av', run: 'Kjør', unavailable: 'Utilgjengelig',
    now: 'Nå', today: 'I dag', pair: 'Koble til denne skjermen', pairLead: 'Skann koden med telefonen og logg inn med passnøkkel.',
    or: 'eller åpne', code: 'Kode', waiting: 'Venter på godkjenning…', getting: 'Henter kode…', offline: 'Får ikke kontakt med serveren. Prøver igjen…',
    failed: 'Det gikk ikke.', empty: 'Ingenting her ennå. Velg innhold for skjermen i admin på telefonen.', nothing: 'Ingenting spilles', preview: 'Forhåndsvisning: her kan du se, men ikke styre.', next: 'Neste timer', days3: 'Neste dager', rooms: 'Rom', allRooms: 'Alle rom', favourites: 'Favoritter', search: 'Søk etter musikk og radio', searching: 'Søker…', noHits: 'Fant ingenting.', kinds: { track: 'Sanger', playlist: 'Spillelister', album: 'Album', radio: 'Radio' }
  } : {
    days: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
    months: ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
    indoor: 'Indoor', outdoor: 'Outdoor', tomorrow: 'Tomorrow', on: 'On', off: 'Off', run: 'Run', unavailable: 'Unavailable',
    now: 'Now', today: 'Today', pair: 'Pair this screen', pairLead: 'Scan the code with your phone and sign in with your passkey.',
    or: 'or open', code: 'Code', waiting: 'Waiting for approval…', getting: 'Getting a code…', offline: 'Cannot reach the server. Retrying…',
    failed: 'That did not work.', empty: 'Nothing here yet. Choose what this screen shows in the admin page on your phone.', nothing: 'Nothing playing', preview: 'Preview: look, but no controls.', next: 'Next hours', days3: 'Coming days', rooms: 'Rooms', allRooms: 'All rooms', favourites: 'Favourites', search: 'Search music and radio', searching: 'Searching…', noHits: 'Nothing found.', kinds: { track: 'Songs', playlist: 'Playlists', album: 'Albums', radio: 'Radio' }
  };
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
    door: 'M5 21V4a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v17M3 21h18M15 12h.01'
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
      var i, k;
      layout = { tiles: [], more: [], panels: {}, pages: [] };
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
    return '<div class="col w30"><div class="card clock"><div class="time" id="time"></div><span class="date" id="date"></span></div></div>';
  }

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
      '<div class="big">' + num(val(e, 'measure_temperature'), 1) + '°</div><div class="sub">' + bits.join(' · ') + '</div></div>';
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
        html += slot('', pad(d.getHours()), s[i][2], Math.round(s[i][1]) + '°');
        n++;
      }
    }
    var t = tomorrow(s);
    if (t) { html += '<div class="slot tomorrow"><span class="h">' + T.tomorrow + '</span>' + icon(weather(t.symbol).icon) + ' ' + Math.round(t.lo) + '°/' + Math.round(t.hi) + '°</div>'; }
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

  function favsHtml(lead, title, max) {
    var favs = val(lead, 'favourites') || [], html = '', i;
    for (i = 0; i < favs.length && i < max; i++) {
      html += '<span class="fav' + (favs[i].title === title ? ' on' : '') + '" data-act="fav" data-id="' + esc(lead.id) + '" data-fav="' + esc(favs[i].id) + '">' + esc(favs[i].title) + '</span>';
    }
    return html;
  }

  function volHtml(e) {
    var vol = Math.round((val(e, 'volume_set') || 0) * 100);
    return '<input type="range" min="0" max="100" step="1" value="' + vol + '" data-id="' + esc(e.id) + '"><span>' + vol + '</span>';
  }

  // One room's card: what its group plays, with the room's own volume unless left out.
  function musicCard(room, favCount, withVol) {
    if (!room) { return ''; }
    var lead = leader(room), playing = val(lead, 'speaker_playing') === true;
    var title = val(lead, 'speaker_track') || T.nothing, sub = val(lead, 'speaker_artist') || '';
    return '<div class="card music' + (favCount ? '' : ' short') + '"><div class="art">' + icon('radio') + '</div>' +
      '<div class="now-playing"><div class="sub">' + esc(groupName(groupOf(room))) + '</div><div class="np-title">' + esc(title) + '</div><div class="np-sub sub">' + esc(sub) + '</div></div>' +
      '<span class="play" data-act="play" data-id="' + esc(lead.id) + '">' + icon(playing ? 'pause' : 'play', true) + '</span>' +
      (withVol ? '<div class="vol">' + volHtml(room) + '</div>' : '') +
      (favCount ? '<div class="favs">' + favsHtml(lead, title, favCount) + '</div>' : '') + '</div>';
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
    return 'bolt';
  }

  function tileHtml(e) {
    if (!e || e.missing) {
      return '<div class="tile missing"><div class="in"><p class="label">' + T.unavailable + '</p></div></div>';
    }
    var cls = 'tile', body = '';
    var onoff = cap(e, 'onoff'), dim = cap(e, 'dim'), target = cap(e, 'target_temperature'), measure = cap(e, 'measure_temperature');
    if (onoff) {
      if (onoff.value) { cls += ' on'; }
      body = '<div class="value">' + (onoff.value ? (dim ? Math.round(dim.value * 100) + ' %' : T.on) : T.off) + '</div>';
    } else if (target) {
      body = '<div class="value up">' + num(target.value, 1) + '°' + (measure ? '<div class="sub">' + T.now + ' ' + num(measure.value, 1) + '°</div>' : '') + '</div>' +
        '<div class="steps"><span class="step" data-act="step" data-step="-0.5" data-id="' + esc(e.id) + '">−</span><span class="step" data-act="step" data-step="0.5" data-id="' + esc(e.id) + '">+</span></div>';
    } else if (cap(e, 'speaker_playing')) {
      if (val(leader(e), 'speaker_playing')) { cls += ' on'; }
      body = '<div class="value sub">' + esc(val(leader(e), 'speaker_playing') ? val(leader(e), 'speaker_track') || T.on : T.off) + '</div>';
    } else if (cap(e, 'button')) {
      body = '<div class="value sub">' + T.run + '</div>';
    } else if (measure) {
      body = '<div class="value">' + num(measure.value, 1) + '°</div>';
    }
    return '<div class="' + cls + '" data-act="tile" data-id="' + esc(e.id) + '"><div class="in">' + icon(tileIcon(e)) +
      '<p class="label">' + esc(e.name) + '</p><p class="zone">' + esc(e.zone) + '</p>' + body + '</div></div>';
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
    { id: 'weather', icon: 'cloud', when: function () { return !!series(); } }
  ];

  function homePage() {
    var tiles = [], i;
    for (i = 0; i < layout.tiles.length && i < 6; i++) { tiles.push(entities[layout.tiles[i]] || { missing: true }); }
    var html = '<div class="row">' + clockCard() + climateCard('w70') + '</div>' + forecastCard() + musicCard(panel('speaker'), 5, true) + tilesHtml(tiles, false);
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
    return musicCard(room, 0, false) +
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
      html += '<div class="hit"><span class="hit-t" data-act="fav" data-id="' + esc(lead.id) + '" data-fav="' + esc(r.id) + '">' + esc(r.title) +
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

  function weatherPage() {
    var s = series(), now = new Date().getTime() / 1000, hours = '', days = '', n = 0, i, d, st;
    for (i = 0; i < s.length && n < 12; i++) {
      if (s[i][0] > now) {
        d = new Date(s[i][0] * 1000);
        hours += slot('', pad(d.getHours()), s[i][2], Math.round(s[i][1]) + '°');
        n++;
      }
    }
    for (i = 0; i < 4; i++) {
      st = dayStats(s, i);
      if (st) { days += slot('', i === 0 ? T.today : T.days[st.date.getDay()], st.symbol, Math.round(st.lo) + '° / ' + Math.round(st.hi) + '°'); }
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
        if (it.card === 'music') { html += musicCard(panel('speaker') || speakers()[0], 5, true); }
      } else {
        run.push(typeof it === 'string' ? entities[it] || { missing: true } : it);
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
    $('rail').innerHTML = rail;
    $('page').innerHTML = pageHtml();
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
      render();
    } else if (act === 'step' && e && (c = cap(e, 'target_temperature'))) {
      prev = c.value;
      next = Math.round((prev + parseFloat(el.getAttribute('data-step'))) * 2) / 2;
      if (typeof c.min === 'number') { next = Math.max(c.min, next); }
      if (typeof c.max === 'number') { next = Math.min(c.max, next); }
      c.value = next;
      render();
      set(e, 'target_temperature', next, prev);
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

  function onSlide(ev) {
    var t = ev.target, id = t && t.getAttribute && t.getAttribute('data-id'), e = id ? entities[id] : null, c, v, prev;
    if (!e || t.type !== 'range') { return; }
    holdUntil = new Date().getTime() + 1500;
    if (t.nextSibling) { t.nextSibling.textContent = t.value; }
    if (ev.type === 'change' && (c = cap(e, 'volume_set'))) {
      v = parseInt(t.value, 10) / 100;
      prev = c.value;
      c.value = v;
      set(e, 'volume_set', v, prev);
    }
  }

  // ------------------------------------------------------------- clock & theme

  function tick() {
    var d = new Date(), el = $('time');
    if (el) {
      el.textContent = pad(d.getHours()) + ':' + pad(d.getMinutes());
      $('date').textContent = T.days[d.getDay()] + ' ' + d.getDate() + (NB ? '. ' : ' ') + T.months[d.getMonth()];
    }
    // shortcut: fixed 07–21 day; switch to sunrise/sunset from the forecast integration later.
    var h = d.getHours();
    document.body.className = h >= 7 && h < 21 ? 'day' : 'night';
  }

  document.addEventListener('click', onTap, false);
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
