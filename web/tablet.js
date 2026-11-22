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
    calMonth: 'Måned', calWeek: 'Uke', calWork: 'Arbeidsuke', calDay: 'Dag', calList: 'Liste', calComing: 'Kommende', monthsLong: ['januar', 'februar', 'mars', 'april', 'mai', 'juni', 'juli', 'august', 'september', 'oktober', 'november', 'desember'], pairPin: 'Tall å taste inn', failed: 'Det gikk ikke.', empty: 'Ingenting her ennå. Velg innhold for skjermen i admin på telefonen.', nothing: 'Ingenting spilles', theme: 'Utseende', themeAuto: 'Automatisk', themeDay: 'Lyst', themeNight: 'Mørkt', thisScreen: 'Denne skjermen', size: 'Klokke og temperatur', sizeNormal: 'Normal', sizeLarge: 'Store', sizeXl: 'Ekstra store', reload: 'Last inn på nytt', preview: 'Forhåndsvisning: her kan du se, men ikke styre.', next: 'Neste timer', days3: 'Neste dager', rooms: 'Rom', allRooms: 'Alle rom', favourites: 'Favoritter', search: 'Søk etter musikk og radio', searching: 'Søker…', noHits: 'Fant ingenting.', ships: 'Båter i nærheten', noShips: 'Ingen båter innen', radar: 'Radar', chart: 'Sjøkart', map: 'Kart', ago: 'for % min siden', price: 'Strømpris', priceNow: 'Nå', cheapest: 'Billigst', dearest: 'Dyrest', krKwh: 'kr/kWh', noPrices: 'Ingen priser ennå', nowDep: 'Nå', min: 'min', cancelled: 'Innstilt', platform: 'Plf.', noDeps: 'Ingen avganger', noEvents: 'Ingenting i kalenderen', allDay: 'Hele dagen', busy: 'Opptatt', warnings: 'Farevarsler', noWarnings: 'Ingen farevarsler', levels: { 2: 'Gult nivå', 3: 'Oransje nivå', 4: 'Rødt nivå' }, fromT: 'fra', untilT: 'til', more: 'til', source: 'Kilde: Meteorologisk institutt', kn: 'kn', shipKinds: { fishing: 'Fiske', tug: 'Slepebåt', sailing: 'Seilbåt', pleasure: 'Fritidsbåt', fast: 'Hurtigbåt', pilot: 'Los', sar: 'Redning', passenger: 'Passasjer', cargo: 'Last', tanker: 'Tankskip', other: 'Annet' }, yes: 'Ja', no: 'Nei', close: 'Lukk', kinds: { track: 'Sanger', playlist: 'Spillelister', album: 'Album', radio: 'Radio' }
  }, en: {
    days: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
    months: ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
    indoor: 'Indoor', outdoor: 'Outdoor', clock: 'Time', tomorrow: 'Tomorrow', on: 'On', off: 'Off', run: 'Run', unavailable: 'Unavailable',
    now: 'Now', today: 'Today', pair: 'Pair this screen', pairLead: 'Scan the code with your phone and sign in with your passkey.',
    or: 'or open', code: 'Code', waiting: 'Waiting for approval…', getting: 'Getting a code…', offline: 'Cannot reach the server. Retrying…',
    calMonth: 'Month', calWeek: 'Week', calWork: 'Work week', calDay: 'Day', calList: 'List', calComing: 'Coming up', monthsLong: ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'], pairPin: 'Number to type', failed: 'That did not work.', empty: 'Nothing here yet. Choose what this screen shows in the admin page on your phone.', nothing: 'Nothing playing', theme: 'Appearance', themeAuto: 'Automatic', themeDay: 'Light', themeNight: 'Dark', thisScreen: 'This screen', size: 'Clock and temperature', sizeNormal: 'Normal', sizeLarge: 'Large', sizeXl: 'Extra large', reload: 'Reload', preview: 'Preview: look, but no controls.', next: 'Next hours', days3: 'Coming days', rooms: 'Rooms', allRooms: 'All rooms', favourites: 'Favourites', search: 'Search music and radio', searching: 'Searching…', noHits: 'Nothing found.', ships: 'Ships nearby', noShips: 'No ships within', radar: 'Radar', chart: 'Chart', map: 'Map', ago: '% min ago', price: 'Electricity price', priceNow: 'Now', cheapest: 'Cheapest', dearest: 'Dearest', krKwh: 'kr/kWh', noPrices: 'No prices yet', nowDep: 'Now', min: 'min', cancelled: 'Cancelled', platform: 'Pl.', noDeps: 'No departures', noEvents: 'Nothing on the calendar', allDay: 'All day', busy: 'Busy', warnings: 'Weather warnings', noWarnings: 'No weather warnings', levels: { 2: 'Yellow level', 3: 'Orange level', 4: 'Red level' }, fromT: 'from', untilT: 'until', more: 'more', source: 'Source: MET Norway', kn: 'kn', shipKinds: { fishing: 'Fishing', tug: 'Tug', sailing: 'Sailing', pleasure: 'Pleasure', fast: 'High speed', pilot: 'Pilot', sar: 'Rescue', passenger: 'Passenger', cargo: 'Cargo', tanker: 'Tanker', other: 'Other' }, yes: 'Yes', no: 'No', close: 'Close', kinds: { track: 'Songs', playlist: 'Playlists', album: 'Albums', radio: 'Radio' }
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
    alert: 'M12 4 2.5 20h19zM12 10v5M12 17.5v.01',
    camera: 'M4 8h3.5L9 5.5h6L16.5 8H20a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1zM12 10a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z',
    calendar: 'M5 5h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM4 10h16M8 3v4M16 3v4',
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
  var FORECAST_MIN = 68, TILE_MIN = 130;
  var anchor = null;           // the speaker whose group the music page shows
  var query = '', results = null, searching = false, saved = {};
  var typing = false;          // don't redraw the search box under the keyboard
  var events = null;
  var pairTimer = null;
  var toastTimer = null;
  var linkTimer = null;       // set from the moment the connection is lost until it is back
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

  function linkShow() {
    $('link').textContent = T.offline;
    $('link').className = 'link';
  }

  // Short dropouts are normal on old Wi-Fi: say nothing unless it lasts.
  function linkLost() {
    if (linkTimer === null) { linkTimer = setTimeout(linkShow, 15000); }
  }

  function linkBack() {
    clearTimeout(linkTimer);
    linkTimer = null;
    $('link').className = 'link hidden';
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
    linkBack();
    startPairing();
  }

  function startPairing() {
    clearTimeout(pairTimer);
    show('pair');
    $('pair-title').textContent = T.pair;
    $('pair-lead').textContent = T.pairLead;
    $('pair-or').textContent = T.or;
    $('pair-code-label').textContent = T.code;
    $('pair-pin-label').textContent = T.pairPin;
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
      $('pair-pin').textContent = data.pin;
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
        linkLost();
        linkShow();
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
      layout = { tiles: [], more: [], panels: {}, pages: [], name: data.device ? data.device.name : '', favRows: data.device ? data.device.favRows : 1, imageToken: data.device ? data.device.imageToken : '' };
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
      // Fresh values, and a stream to keep them fresh (the two modes without one poll instead).
      if (PREVIEW || !window.EventSource || (events && events.readyState === 1)) { linkBack(); }
      if (!events) { connectEvents(); }
    });
  }

  function connectEvents() {
    if (PREVIEW) { setTimeout(loadState, 10000); return; } // no live stream for a preview: refresh instead
    if (events) { events.close(); events = null; }
    if (!window.EventSource) { setTimeout(loadState, 60000); return; } // no live updates: refresh instead
    request('POST', '/api/tablet/ticket', {}, {}, function (status, data) {
      if (events) { return; } // another attempt got there first
      if (status !== 200 || !data) { linkLost(); setTimeout(connectEvents, 5000); return; }
      var es = new EventSource('/api/events?ticket=' + encodeURIComponent(data.ticket));
      events = es;
      // Fetch only once subscribed, so a change made while the stream was down can't be missed.
      es.onopen = function () { loadState(); };
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
        linkLost();
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

  // Wind for a forecast hour: the arrow points where it blows, amber from "liten kuling".
  function windHtml(row) {
    if (!row || typeof row[3] !== 'number') { return '<span class="wind">&nbsp;</span>'; }
    return '<span class="wind' + (row[3] >= 10.8 ? ' strong' : '') + '">' +
      (typeof row[4] === 'number' ? '<i class="arr"' + rot(row[4] + 180) + '>↑</i>' : '') + Math.round(row[3]) + ' m/s</span>';
  }

  function slot(cls, hour, sym, text, row) {
    return '<div class="slot ' + cls + '"><span class="h">' + hour + '</span>' + icon(weather(sym).icon) + ' ' + text + windHtml(row) + '</div>';
  }

  function forecastCard() {
    var f = panel('weather'), s = series();
    if (!f || !s) { return ''; }
    var now = new Date().getTime() / 1000, html = '', n = 0, i, d, w = weather(val(f, 'weather_symbol'));
    html += '<div class="slot now"><span class="h">' + esc(w.text) + '</span>' + icon(w.icon) + windHtml(s[0]) + '</div>';
    for (i = 0; i < s.length && n < 6; i++) {
      d = new Date(s[i][0] * 1000);
      if (s[i][0] > now && d.getHours() % 2 === 0) {
        html += slot('', pad(d.getHours()), s[i][2], deg(s[i][1]), s[i]);
        n++;
      }
    }
    var t = tomorrow(s);
    if (t) { html += '<div class="slot tomorrow"><span class="h">' + T.tomorrow + '</span>' + icon(weather(t.symbol).icon) + ' ' + deg(t.lo) + '/' + deg(t.hi) + windHtml(t.wind) + '</div>'; }
    return '<div class="card forecast">' + html + '</div>';
  }

  function dayStats(s, offset) {
    var start = new Date(); start.setHours(0, 0, 0, 0); start.setDate(start.getDate() + offset);
    var from = start.getTime() / 1000, to = from + 86400, lo = null, hi = null, symbol = null, wind = null, i;
    for (i = 0; i < s.length; i++) {
      if (s[i][0] >= from && s[i][0] < to) {
        if (lo === null || s[i][1] < lo) { lo = s[i][1]; }
        if (hi === null || s[i][1] > hi) { hi = s[i][1]; }
        if (typeof s[i][3] === 'number' && (!wind || s[i][3] > wind[3])) { wind = s[i]; } // the day's strongest
        if (new Date(s[i][0] * 1000).getHours() === 12 || symbol === null) { symbol = s[i][2]; }
      }
    }
    return lo === null ? null : { lo: lo, hi: hi, symbol: symbol, wind: wind, date: start };
  }

  function tomorrow(s) { return dayStats(s, 1); }

  // An image the integration serves through oldpad, by its key.
  function imgSrc(e, key) { return '/api/image/' + esc(e.id.slice(0, e.id.indexOf(':'))) + '/' + esc(key) + imgAuth(); }
  function imgAuth() { return layout.imageToken ? '?t=' + esc(layout.imageToken) : ''; }

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
    if (c === 'departures') { return 'clock'; }
    if (c === 'calendar') { return 'calendar'; }
    if (c === 'camera') { return 'camera'; }
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
    } else if (cap(e, 'departures')) {
      var dep = nextDeps(e)[0];
      body = dep ? '<div class="value up">' + depWhen(dep) + '<div class="sub">' + esc(dep.line + ' ' + dep.to) + '</div></div>' : '<div class="value sub">' + T.noDeps + '</div>';
    } else if (cap(e, 'camera')) {
      body = camHtml(e.id);
    } else if (cap(e, 'events')) {
      var ev = calEvents([e])[0];
      body = ev ? '<div class="value up">' + calShort(ev) + '<div class="sub">' + esc(ev.x.t || T.busy) + '</div></div>' : '<div class="value sub">' + T.noEvents + '</div>';
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
    if (sheet === 'alerts') {
      return '<div class="sheet-bg" data-act="close"><div class="sheet wide" data-act="none"><span class="x" data-act="close">✕</span>' + alertsCard() + '</div></div>';
    }
    if (e && (e['class'] === 'price' || e['class'] === 'departures' || e['class'] === 'calendar' || e['class'] === 'camera')) {
      return '<div class="sheet-bg" data-act="close"><div class="sheet wide" data-act="none"><span class="x" data-act="close">✕</span>' + (e['class'] === 'price' ? priceCard(e) : e['class'] === 'calendar' ? calendarCard([e]) : e['class'] === 'camera' ? camCard(e) : departuresCard(e)) + '</div></div>';
    }
    if (!list.length) { return ''; }
    for (i = 0; i < list.length; i++) { html += controlHtml(e, list[i]); }
    return '<div class="sheet-bg" data-act="close"><div class="sheet" data-act="none">' +
      '<span class="x" data-act="close">✕</span><div class="sheet-h">' + esc(e.name) + '</div><div class="sub">' + esc(e.zone) + '</div>' + html + '</div></div>';
  }

  // ------------------------------------------------------------- departures

  function nextDeps(e) {
    var all = val(e, 'departures') || [], now = new Date().getTime() / 1000 - 30, out = [], i;
    for (i = 0; i < all.length; i++) { if (all[i].at > now) { out.push(all[i]); } }
    return out;
  }
  // "Now", "4 min", then the clock once it's a while off.
  function depWhen(d) {
    var m = Math.floor((d.at - new Date().getTime() / 1000) / 60);
    if (m < 1) { return T.nowDep; }
    if (m < 15) { return m + ' ' + T.min; }
    return pad(new Date(d.at * 1000).getHours()) + ':' + pad(new Date(d.at * 1000).getMinutes());
  }
  function hm(sec) { var d = new Date(sec * 1000); return pad(d.getHours()) + ':' + pad(d.getMinutes()); }

  function departuresCard(e) {
    var list = nextDeps(e), rows = '', i, d, late;
    for (i = 0; i < list.length; i++) {
      d = list[i];
      late = d.aimed && d.at - d.aimed >= 120;
      rows += '<div class="dep' + (d.gone ? ' gone' : '') + '"><span class="badge m-' + esc(d.mode) + '"' + (d.bg ? ' data-bg="' + esc(d.bg) + '" data-fg="' + esc(d.fg || '#fff') + '"' : '') + '>' + esc(d.line) + '</span>' +
        '<span class="dt">' + esc(d.to) + '<span class="sub">' + (d.quay ? T.platform + ' ' + esc(d.quay) : '') + (late ? ' <s>' + hm(d.aimed) + '</s>' : '') + '</span></span>' +
        '<span class="dw' + (d.rt ? ' rt' : '') + '">' + (d.gone ? T.cancelled : depWhen(d)) + '</span></div>';
    }
    return '<div class="card departures"><div class="sub">' + esc(e.name) + '</div>' + (rows || '<p class="muted">' + T.noDeps + '</p>') + '</div>';
  }

  // ------------------------------------------------------------- calendar

  // A day as its UTC midnight, so days compare and step the same across daylight saving.
  function dayOf(d) { return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 1000; }

  // What is still to come from these calendars, each under the day it shows on.
  // All-day events are UTC midnights; what already began shows under today.
  function calEvents(list) {
    var nowDate = new Date(), now = nowDate.getTime() / 1000, today = dayOf(nowDate), out = [], i, j, all, x, gone, limit;
    for (i = 0; i < list.length; i++) {
      all = val(list[i], 'events') || [];
      // The server sends months for the calendar page; the agenda keeps to its chosen days.
      limit = now + ((val(list[i], 'agenda') || {}).days || 14) * 86400;
      for (j = 0; j < all.length; j++) {
        x = all[j];
        if (x.at > limit) { continue; }
        gone = x.all ? x.end <= today : x.end > x.at ? x.end <= now : x.at < now;
        if (!gone) { out.push({ x: x, day: Math.max(x.all ? x.at : dayOf(new Date(x.at * 1000)), today) }); }
      }
    }
    out.sort(function (a, b) { return a.day - b.day || (b.x.all ? 1 : 0) - (a.x.all ? 1 : 0) || a.x.at - b.x.at; });
    return out;
  }
  function calDayName(day) {
    var today = dayOf(new Date()), d = new Date(day * 1000);
    if (day === today) { return T.today; }
    if (day === today + 86400) { return T.tomorrow; }
    return T.days[d.getUTCDay()] + ' ' + d.getUTCDate() + (NB ? '. ' : ' ') + T.months[d.getUTCMonth()];
  }
  function calShort(ev) {
    if (ev.day === dayOf(new Date())) { return ev.x.all ? T.today : hm(ev.x.at); }
    return ev.day === dayOf(new Date()) + 86400 ? T.tomorrow : T.days[new Date(ev.day * 1000).getUTCDay()];
  }
  function calendarCard(list) {
    var evs = calEvents(list), html = list.length === 1 ? '<div class="sub">' + esc(list[0].name) + '</div>' : '', day = null, i, ev;
    for (i = 0; i < evs.length && i < 40; i++) {
      ev = evs[i];
      if (ev.day !== day) { day = ev.day; html += '<div class="cal-day">' + esc(calDayName(day)) + '</div>'; }
      html += '<div class="cal"><span class="cw">' + (ev.x.all ? T.allDay : hm(ev.x.at) + (ev.x.end > ev.x.at ? '–' + hm(ev.x.end) : '')) + '</span>' +
        '<span class="ct">' + esc(ev.x.t || T.busy) + (ev.x.place ? '<span class="sub">' + esc(ev.x.place) + '</span>' : '') + '</span></div>';
    }
    return '<div class="card calendar">' + html + (evs.length ? '' : '<p class="muted">' + T.noEvents + '</p>') + '</div>';
  }

  // ------------------------------------------------------------- calendar page
  // Month, week, work week, day and list. Everything is placed in percent, so a view
  // fills the screen at any size. Days are UTC midnights in seconds, as in dayOf().

  var CAL_VIEWS = ['month', 'week', 'work', 'day', 'list'];
  var calView = load('oldpad.calview'), calFocus = null, calOpen = null, calHidden = {};
  if (CAL_VIEWS.indexOf(calView) < 0) { calView = 'month'; }

  function calToday() { return dayOf(new Date()); }
  function calDate() { return calFocus === null ? calToday() : calFocus; }
  function calWeekday(day) { return (new Date(day * 1000).getUTCDay() + 6) % 7; } // Monday is 0
  function calMonday(day) { return day - calWeekday(day) * 86400; }
  function calWeekNo(day) {
    var thursday = calMonday(day) + 3 * 86400, year = new Date(thursday * 1000).getUTCFullYear();
    return Math.floor((thursday - Date.UTC(year, 0, 1) / 1000) / 604800) + 1;
  }
  function calShortDay(day) { return T.days[new Date(day * 1000).getUTCDay()].slice(0, 3); }
  function calLongDate(day) {
    var d = new Date(day * 1000);
    return T.days[d.getUTCDay()] + ' ' + d.getUTCDate() + (NB ? '. ' : ' ') + T.monthsLong[d.getUTCMonth()];
  }
  function calTime(x) { return x.all ? T.allDay : hm(x.at) + (x.end > x.at ? '–' + hm(x.end) : ''); }
  function calColor(i) { return ' c' + (i % 6); }

  // Events by the days they cover, all-day first. A hidden calendar is left out.
  function calIndex(list) {
    var out = {}, i, j, all, x, first, last, day;
    for (i = 0; i < list.length; i++) {
      if (calHidden[list[i].id]) { continue; }
      all = val(list[i], 'events') || [];
      for (j = 0; j < all.length; j++) {
        x = all[j];
        first = x.all ? x.at : dayOf(new Date(x.at * 1000));
        last = x.all ? Math.max(x.at, x.end - 86400) : dayOf(new Date(Math.max(x.at, x.end - 1) * 1000));
        last = Math.min(last, first + 62 * 86400);
        for (day = first; day <= last; day += 86400) {
          if (!out[day]) { out[day] = []; }
          out[day].push({ x: x, c: i, key: i + ':' + j, name: list[i].name });
        }
      }
    }
    for (day in out) {
      if (out.hasOwnProperty(day)) {
        out[day].sort(function (a, b) { return (b.x.all ? 1 : 0) - (a.x.all ? 1 : 0) || a.x.at - b.x.at; });
      }
    }
    return out;
  }

  function calDays() {
    var day = calDate(), start = calMonday(day), n, out = [], i;
    if (calView === 'day') { return [day]; }
    n = calView === 'work' ? 5 : 7;
    // On a weekend, the work week that matters is the next one.
    if (calView === 'work' && calWeekday(day) > 4) { start += 7 * 86400; }
    for (i = 0; i < n; i++) { out.push(start + i * 86400); }
    return out;
  }

  function calTitle() {
    var day = calDate(), d = new Date(day * 1000), list, a, b;
    if (calView === 'month') { return T.monthsLong[d.getUTCMonth()] + ' ' + d.getUTCFullYear(); }
    if (calView === 'day') { return calLongDate(day); }
    if (calView === 'list') { return day === calToday() ? T.calComing : calLongDate(day); }
    list = calDays();
    a = new Date(list[0] * 1000);
    b = new Date(list[list.length - 1] * 1000);
    return T.calWeek + ' ' + calWeekNo(list[0]) + ' · ' + a.getUTCDate() + (a.getUTCMonth() === b.getUTCMonth() ? '' : '. ' + T.months[a.getUTCMonth()]) +
      '–' + b.getUTCDate() + (NB ? '. ' : ' ') + T.months[b.getUTCMonth()];
  }

  function calHead(list) {
    var names = [['month', T.calMonth], ['week', T.calWeek], ['work', T.calWork], ['day', T.calDay], ['list', T.calList]], seg = '', legend = '', i;
    for (i = 0; i < names.length; i++) {
      seg += '<span class="' + (calView === names[i][0] ? 'on' : '') + '" data-act="calview" data-value="' + names[i][0] + '">' + names[i][1] + '</span>';
    }
    if (list.length > 1) {
      for (i = 0; i < list.length; i++) {
        legend += '<span class="cal-leg' + calColor(i) + (calHidden[list[i].id] ? ' off' : '') + '" data-act="caltoggle" data-id="' + esc(list[i].id) + '"><i></i>' + esc(list[i].name) + '</span>';
      }
      legend = '<div class="cal-legend">' + legend + '</div>';
    }
    return '<div class="cal-top"><span class="cal-nav" data-act="calmove" data-dir="-1">‹</span><span class="cal-nav" data-act="calmove" data-dir="1">›</span>' +
      '<span class="cal-now' + (calDate() === calToday() ? ' on' : '') + '" data-act="caltoday">' + T.today + '</span>' +
      '<span class="cal-title">' + esc(calTitle()) + '</span><span class="cal-seg">' + seg + '</span></div>' + legend;
  }

  function calBox(left, top, width, height) {
    return ' data-l="' + left + '%" data-t="' + top + '%" data-width="' + width + '%" data-w="' + height + '%"';
  }

  function calMonthView(index) {
    var focus = new Date(calDate() * 1000), y = focus.getUTCFullYear(), m = focus.getUTCMonth(), first = Date.UTC(y, m, 1) / 1000;
    var start = calMonday(first), count = new Date(Date.UTC(y, m + 1, 0)).getUTCDate(), rows = Math.ceil((calWeekday(first) + count) / 7);
    var today = calToday(), wide = window.innerWidth > 900, html = '', head = '', weeks = '', r, c, day, list, room, cell, i, w = 100 / 7, h = 100 / rows;
    // How many events fit in a cell before it says "+2".
    room = Math.max(1, Math.floor(((window.innerHeight - 170) / rows - 30) / 21));
    for (c = 0; c < 7; c++) { head += '<span' + calBox(c * w, 0, w, 100) + '>' + calShortDay(start + c * 86400) + '</span>'; }
    for (r = 0; r < rows; r++) {
      weeks += '<span' + calBox(0, r * h, 100, h) + '>' + calWeekNo(start + r * 7 * 86400) + '</span>';
      for (c = 0; c < 7; c++) {
        day = start + (r * 7 + c) * 86400;
        list = index[day] || [];
        cell = '<span class="m-n">' + new Date(day * 1000).getUTCDate() + '</span>';
        for (i = 0; i < list.length && i < (list.length > room ? room - 1 : room); i++) {
          cell += '<span class="m-ev' + calColor(list[i].c) + (list[i].x.all ? ' all' : '') + '">' + (wide && !list[i].x.all ? '<b>' + hm(Math.max(list[i].x.at, day)) + '</b> ' : '') + esc(list[i].x.t || T.busy) + '</span>';
        }
        if (list.length > room) { cell += '<span class="m-more">+' + (list.length - Math.max(room - 1, 0)) + '</span>'; }
        html += '<div class="m-cell' + (new Date(day * 1000).getUTCMonth() !== m ? ' out' : '') + (day === today ? ' today' : '') + (c > 4 ? ' end' : '') + '" data-act="calday" data-day="' + day + '"' + calBox(c * w, r * h, w, h) + '>' + cell + '</div>';
      }
    }
    return '<div class="m-head">' + head + '</div><div class="m-weeks">' + weeks + '</div><div class="m-body">' + html + '</div>';
  }

  // Side by side when events overlap: each cluster shares the width.
  function calLanes(items) {
    var ends = [], cluster = [], until = -1, i, j, it;
    function close() {
      for (j = 0; j < cluster.length; j++) { cluster[j].lanes = ends.length; }
      cluster = [];
      ends = [];
    }
    for (i = 0; i < items.length; i++) {
      it = items[i];
      if (it.a >= until) { close(); until = it.b; }
      until = Math.max(until, it.b);
      for (j = 0; j < ends.length && ends[j] > it.a; j++) { /* the first free lane */ }
      ends[j] = it.b;
      it.lane = j;
      cluster.push(it);
    }
    close();
  }

  function calGrid(index, days) {
    var h0 = 7, h1 = 18, today = calToday(), nowDate = new Date(), nowMin = nowDate.getHours() * 60 + nowDate.getMinutes();
    var perDay = [], allRows = 0, head = '', cols = '', lines = '', w = 100 / days.length, i, j, list, timed, all, it, a, b, span, html, top, height, headH;
    function clip(x, day) {
      var s = dayOf(new Date(x.at * 1000)) < day ? 0 : new Date(x.at * 1000).getHours() * 60 + new Date(x.at * 1000).getMinutes();
      var e = dayOf(new Date(x.end * 1000)) > day ? 1440 : new Date(x.end * 1000).getHours() * 60 + new Date(x.end * 1000).getMinutes();
      return { a: s, b: Math.max(e, s + 30) };
    }
    for (i = 0; i < days.length; i++) {
      list = index[days[i]] || [];
      timed = [];
      all = [];
      for (j = 0; j < list.length; j++) {
        if (list[j].x.all) { all.push(list[j]); continue; }
        a = clip(list[j].x, days[i]);
        timed.push({ it: list[j], a: a.a, b: Math.min(a.b, 1440) });
        h0 = Math.min(h0, Math.floor(a.a / 60));
        h1 = Math.max(h1, Math.min(24, Math.ceil(a.b / 60)));
      }
      calLanes(timed);
      perDay.push({ timed: timed, all: all });
      allRows = Math.max(allRows, Math.min(all.length, 2));
    }
    span = (h1 - h0) * 60;
    headH = 42 + allRows * 25;
    for (i = 0; i < days.length; i++) {
      html = '<span class="tg-day' + (days[i] === today ? ' today' : '') + '" data-act="calday" data-day="' + days[i] + '"><b>' + new Date(days[i] * 1000).getUTCDate() + '</b> ' + calShortDay(days[i]) + '</span>';
      all = perDay[i].all;
      for (j = 0; j < all.length && j < 2; j++) {
        html += j === 1 && all.length > 2 ? '<span class="tg-all more" data-act="calday" data-day="' + days[i] + '">+' + (all.length - 1) + '</span>'
          : '<span class="tg-all' + calColor(all[j].c) + '" data-act="calev" data-ev="' + all[j].key + '">' + esc(all[j].x.t || T.busy) + '</span>';
      }
      head += '<div class="tg-col"' + calBox(i * w, 0, w, 100) + '>' + html + '</div>';
      html = '';
      for (j = 0; j < perDay[i].timed.length; j++) {
        it = perDay[i].timed[j];
        top = (it.a - h0 * 60) / span * 100;
        height = (it.b - it.a) / span * 100;
        html += '<div class="tg-ev' + calColor(it.it.c) + '" data-act="calev" data-ev="' + it.it.key + '"' + calBox(it.lane * 100 / it.lanes, top, 100 / it.lanes, height) + '><b>' + esc(it.it.x.t || T.busy) + '</b>' + (it.b - it.a >= 45 ? '<span>' + hm(it.it.x.at) + '</span>' : '') + '</div>';
      }
      if (days[i] === today && nowMin >= h0 * 60 && nowMin <= h1 * 60) { html += '<i class="tg-line" data-l="0" data-t="' + (nowMin - h0 * 60) / span * 100 + '%"></i>'; }
      cols += '<div class="tg-col' + (calWeekday(days[i]) > 4 ? ' end' : '') + '" data-day="' + days[i] + '"' + calBox(i * w, 0, w, 100) + '>' + html + '</div>';
    }
    for (i = h0; i < h1; i++) {
      lines += '<span class="tg-hour" data-l="0" data-t="' + (i - h0) * 60 / span * 100 + '%"><em>' + pad(i) + '</em></span>';
    }
    return '<div class="tg"><div class="tg-head" data-w="' + headH + 'px">' + head + '</div><div class="tg-main" data-l="0" data-t="' + headH + 'px">' + lines + '<div class="tg-cols">' + cols + '</div></div></div>';
  }

  function calRow(item) {
    return '<div class="cal cal-row" data-act="calev" data-ev="' + item.key + '"><i class="cal-dot' + calColor(item.c) + '"></i><span class="cw">' + calTime(item.x) + '</span>' +
      '<span class="ct">' + esc(item.x.t || T.busy) + (item.x.place ? '<span class="sub">' + esc(item.x.place) + '</span>' : '') + '</span></div>';
  }

  function calDayView(index) {
    var day = calDate(), list = index[day] || [], rows = '', i;
    for (i = 0; i < list.length; i++) { rows += calRow(list[i]); }
    return '<div class="dv-grid">' + calGrid(index, [day]) + '</div><div class="dv-list card">' + (rows || '<p class="muted">' + T.noEvents + '</p>') + '</div>';
  }

  function calListView(index) {
    var day = calDate(), seen = {}, html = '', n = 0, i, d, list, rows;
    for (d = day; d < day + 130 * 86400 && n < 80; d += 86400) {
      list = index[d] || [];
      rows = '';
      for (i = 0; i < list.length; i++) {
        if (!seen[list[i].key]) { seen[list[i].key] = true; rows += calRow(list[i]); n++; }
      }
      if (rows) { html += '<div class="cal-day" data-act="calday" data-day="' + d + '">' + esc(calDayName(d)) + '</div>' + rows; }
    }
    return '<div class="card calendar">' + (html || '<p class="muted">' + T.noEvents + '</p>') + '</div>';
  }

  function calDetail() {
    if (!calOpen) { return ''; }
    var x = calOpen.x, day = x.all ? x.at : dayOf(new Date(x.at * 1000));
    return '<div class="sheet-bg" data-act="calclose"><div class="sheet cal-sheet" data-act="none"><span class="x" data-act="calclose">✕</span>' +
      '<div class="sheet-h"><i class="cal-dot' + calColor(calOpen.c) + '"></i>' + esc(x.t || T.busy) + '</div>' +
      '<p>' + esc(calLongDate(day)) + '<br>' + calTime(x) + '</p>' +
      (x.place ? '<p>' + esc(x.place) + '</p>' : '') + '<p class="muted">' + esc(calOpen.name) + '</p></div></div>';
  }

  function calendarPage() {
    var list = byClass(['calendar']), index = calIndex(list), body;
    if (calView === 'list') { body = calListView(index); } else {
      body = '<div class="cal-fit' + (calView === 'day' ? ' split' : '') + '">' + (calView === 'month' ? calMonthView(index) : calView === 'day' ? calDayView(index) : calGrid(index, calDays())) + '</div>';
    }
    return calHead(list) + body + calDetail();
  }

  // The views fill what is left under the header.
  function fitCal() {
    var el = $('page').querySelector('.cal-fit');
    if (el) { el.style.height = Math.max(340, window.innerHeight - el.getBoundingClientRect().top - 12) + 'px'; }
  }

  // A step back or forward, within the months the server sends.
  function calMove(dir) {
    var day = calDate(), today = calToday(), low = today - 42 * 86400, high = today + 120 * 86400, d, next;
    if (calView === 'month') {
      d = new Date(day * 1000);
      next = Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + dir, 1) / 1000;
      if (next > high || next + 31 * 86400 < low) { return; }
    } else {
      next = Math.max(low, Math.min(high, day + dir * (calView === 'day' ? 1 : 7) * 86400));
    }
    calFocus = next;
    calOpen = null;
    render();
  }

  // ------------------------------------------------------------- cameras

  // The last picture of each camera on screen. Fetched with the screen's sign-in, one at a
  // time and only while shown: an old iPad has little memory.
  var camImages = {}, camBusy = false;

  function camInner(id) {
    var c = camImages[id], e = entities[id];
    if (!c) { return '<span class="cam-none">' + icon('camera') + '</span>'; }
    // The time, in red once it stops updating: a frozen picture must not pass for live.
    return '<img src="' + c.src + '" alt="' + esc(e ? e.name : '') + '"><span class="cam-t' + (new Date().getTime() - c.at > 30000 ? ' old' : '') + '">' + hm(c.at / 1000) + '</span>';
  }
  function camHtml(id) { return '<div class="cam" data-cam="' + esc(id) + '">' + camInner(id) + '</div>'; }
  function camCard(e) { return '<div class="card cam-card"><div class="sub">' + esc(e.name) + '</div>' + camHtml(e.id) + '</div>'; }
  function camPaint(id) {
    var els = $('page').querySelectorAll('[data-cam]'), i;
    for (i = 0; i < els.length; i++) { if (els[i].getAttribute('data-cam') === id) { els[i].innerHTML = camInner(id); } }
  }
  function camLoad(ids) {
    if (!ids.length) { camBusy = false; return; }
    var id = ids.shift();
    request('GET', '/api/tablet/camera?id=' + encodeURIComponent(id) + (PREVIEW ? '&preview=' + PREVIEW : ''), null, {}, function (status, data) {
      if (status === 200 && data && data.data) {
        camImages[id] = { src: 'data:' + data.type + ';base64,' + data.data, at: new Date().getTime() };
      } else if (status === 403) {
        delete camImages[id]; // taken off this screen
      }
      camPaint(id);
      camLoad(ids);
    });
  }
  function camTick(all) {
    var els = $('page').querySelectorAll('[data-cam]'), seen = {}, ids = [], i, id;
    for (i = 0; i < els.length; i++) {
      id = els[i].getAttribute('data-cam');
      if (!seen[id]) { seen[id] = true; if (all || !camImages[id]) { ids.push(id); } }
    }
    for (id in camImages) { if (camImages.hasOwnProperty(id) && !seen[id]) { delete camImages[id]; } }
    if (!camBusy && ids.length) { camBusy = true; camLoad(ids); }
  }

  // ------------------------------------------------------------- weather warnings

  // Warnings not yet over; `on` once they are in force.
  function activeAlerts() {
    var all = val(panel('weather'), 'alerts') || [], now = new Date().getTime() / 1000, out = [], i, a;
    for (i = 0; i < all.length; i++) {
      a = all[i];
      if (!a.until || a.until > now) { out.push({ a: a, on: !a.from || a.from <= now }); }
    }
    return out;
  }
  function alertTime(sec) {
    var d = new Date(sec * 1000), day = dayOf(d), name = calDayName(day);
    // "til i morgen", while a weekday keeps its capital in English.
    if (NB || day <= dayOf(new Date()) + 86400) { name = name.charAt(0).toLowerCase() + name.slice(1); }
    return name + ' ' + hm(sec);
  }
  function alertWhen(w) {
    if (!w.on) { return T.fromT + ' ' + alertTime(w.a.from); }
    return w.a.until ? T.untilT + ' ' + alertTime(w.a.until) : '';
  }
  function alertBanner() {
    var list = activeAlerts(), w = list[0];
    if (!w || page === 'settings') { return ''; }
    return '<div class="alert-bar l' + esc(w.a.level) + '" data-act="alerts">' + icon('alert') + '<b>' + esc(w.a.name) + '</b> ' +
      '<span>' + esc(T.levels[w.a.level]) + ' · ' + esc(alertWhen(w)) + ' · ' + esc(w.a.area) + '</span>' +
      (list.length > 1 ? '<em>+' + (list.length - 1) + ' ' + T.more + '</em>' : '') + '</div>';
  }
  function alertsCard() {
    var list = activeAlerts(), html = '', i, w;
    for (i = 0; i < list.length; i++) {
      w = list[i];
      html += '<div class="alert l' + esc(w.a.level) + '"><div class="alert-h">' + icon('alert') + ' ' + esc(w.a.name) + '</div>' +
        '<div class="sub">' + esc(T.levels[w.a.level]) + ' · ' + esc(alertWhen(w)) + ' · ' + esc(w.a.area) + '</div>' +
        (w.a.text ? '<p>' + esc(w.a.text) + '</p>' : '') + (w.a.advice ? '<p class="muted">' + esc(w.a.advice) + '</p>' : '') + '</div>';
    }
    return '<div class="card alerts"><div class="sheet-h">' + T.warnings + '</div>' + (html || '<p class="muted">' + T.noWarnings + '</p>') + '<p class="sub">' + T.source + '</p></div>';
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
    var els = root.querySelectorAll('[data-l],[data-rot],[data-w],[data-width],[data-bg]'), i, el, r;
    for (i = 0; i < els.length; i++) {
      el = els[i];
      if (el.getAttribute('data-w')) { el.style.height = el.getAttribute('data-w'); }
      if (el.getAttribute('data-width')) { el.style.width = el.getAttribute('data-width'); }
      if (el.getAttribute('data-l')) { el.style.left = el.getAttribute('data-l'); el.style.top = el.getAttribute('data-t'); }
      if (el.getAttribute('data-bg')) { el.style.background = el.getAttribute('data-bg'); el.style.color = el.getAttribute('data-fg') || '#fff'; }
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
      return '<div class="card ships">' + head + mapHtml(e, list, r, view === 'chart' ? 'sjokartraster' : 'grunnkart') + '</div>';
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
        html += '<img class="tile-img" alt="" src="/api/image/' + esc(iid) + '/' + layer + '-' + z + '-' + ty + '-' + tx + imgAuth() + '" data-l="' + Math.round(tx * 256 - left) + 'px" data-t="' + Math.round(ty * 256 - top) + 'px">';
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
    { id: 'ships', icon: 'boat', when: function () { return byClass(['ais']).length > 0; } },
    { id: 'departures', icon: 'clock', when: function () { return byClass(['departures']).length > 0; } },
    { id: 'calendar', icon: 'calendar', when: function () { return byClass(['calendar']).length > 0; } },
    { id: 'cameras', icon: 'camera', when: function () { return byClass(['camera']).length > 0; } }
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
        hours += slot('', pad(d.getHours()), s[i][2], deg(s[i][1]), s[i]);
        n++;
      }
    }
    for (i = 0; i < 4; i++) {
      st = dayStats(s, i);
      if (st) { days += slot('', i === 0 ? T.today : T.days[st.date.getDay()], st.symbol, deg(st.lo) + ' / ' + deg(st.hi), st.wind); }
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
        if (it['class'] === 'ais') { flush(); html += shipsCard(it); } else if (it['class'] === 'price') { flush(); html += priceCard(it); } else if (it['class'] === 'departures') { flush(); html += departuresCard(it); } else if (it['class'] === 'calendar') { flush(); html += calendarCard([it]); } else if (it['class'] === 'camera') { flush(); html += camCard(it); } else { run.push(it); }
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
    if (page === 'departures') { return byClass(['departures']).map(departuresCard).join(''); }
    if (page === 'calendar') { return calendarPage(); }
    if (page === 'cameras') { return '<div class="cams">' + byClass(['camera']).map(camCard).join('') + '</div>'; }
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
    $('page').innerHTML = alertBanner() + pageHtml() + sheetHtml();
    place($('page'));
    tick(); // fills in the clock, which has height in portrait: measure after it
    fitHome();
    fitCal();
    camTick(false);
  }

  // Keeps the home page on one screen when something adds height (a weather warning, more
  // favourites). The forecast gives up its spare room first, then the tiles, down to a tappable size.
  function fitHome() {
    var root = $('page'), forecast = root.querySelector('.forecast'), row = root.querySelector('.tiles');
    var inner = row ? row.querySelectorAll('.tile .in') : [], i, over, cut, rows, h;
    if (forecast) { forecast.style.height = ''; }
    if (row) { row.className = row.className.replace(/ fit\d/g, ''); }
    for (i = 0; i < inner.length; i++) { inner[i].style.height = ''; }
    if (page !== 'home') { return; }
    over = root.offsetHeight - window.innerHeight;
    if (over > 0 && forecast && window.innerWidth > 900) {
      cut = Math.min(over, Math.max(0, forecast.offsetHeight - FORECAST_MIN));
      forecast.style.height = (forecast.offsetHeight - cut) + 'px';
      over -= cut;
    }
    if (over <= 0 || !inner.length) { return; }
    rows = Math.max(1, Math.round(row.offsetHeight / (inner[0].offsetHeight + 12)));
    h = Math.max(TILE_MIN, inner[0].offsetHeight - Math.ceil(over / rows));
    row.className += h < 160 ? ' fit2' : h < 186 ? ' fit1' : '';
    for (i = 0; i < inner.length; i++) { inner[i].style.height = h + 'px'; }
  }

  window.addEventListener('resize', function () { if (page === 'calendar') { render(); } else { fitHome(); } }, false);

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
      calFocus = null;
      calOpen = null;
      render();
    } else if (act === 'calview') {
      calView = el.getAttribute('data-value');
      save('oldpad.calview', calView);
      calOpen = null;
      render();
    } else if (act === 'calmove') {
      calMove(Number(el.getAttribute('data-dir')));
    } else if (act === 'caltoday') {
      calFocus = null;
      render();
    } else if (act === 'calday') {
      calFocus = Number(el.getAttribute('data-day'));
      calView = 'day';
      save('oldpad.calview', calView);
      render();
    } else if (act === 'calev') {
      all = el.getAttribute('data-ev').split(':');
      c = byClass(['calendar'])[Number(all[0])];
      i = c ? (val(c, 'events') || [])[Number(all[1])] : null;
      if (i) { calOpen = { x: i, c: Number(all[0]), name: c.name }; render(); }
    } else if (act === 'calclose') {
      calOpen = null;
      render();
    } else if (act === 'caltoggle') {
      calHidden[id] = !calHidden[id];
      render();
    } else if (act === 'alerts') {
      sheet = 'alerts';
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
    } else if (act === 'tile' && e && (cap(e, 'prices') || cap(e, 'departures') || cap(e, 'events') || cap(e, 'camera'))) {
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
    // On the calendar, a sideways swipe turns the page.
    if (touchAt && page === 'calendar' && !calOpen && calView !== 'list' && Math.abs(t.clientX - touchAt.x) > 70 && Math.abs(t.clientY - touchAt.y) < 50) {
      calMove(t.clientX < touchAt.x ? 1 : -1);
      touchAt = null;
      return;
    }
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
  setInterval(function () { camTick(true); }, 10000);
  // Finished events, a new day and warnings starting or ending change the page without the server saying anything.
  var calShown = '';
  setInterval(function () {
    var list = [], k, now;
    for (k in entities) { if (entities.hasOwnProperty(k) && entities[k]['class'] === 'calendar') { list.push(entities[k]); } }
    now = JSON.stringify(calEvents(list)) + JSON.stringify(activeAlerts()) + dayOf(new Date());
    if (now !== calShown) { if (calShown) { render(); } calShown = now; }
  }, 60000);
  // Long-running kiosks leak; a nightly reload also picks up new versions.
  setTimeout(function () { window.location.reload(); }, 24 * 3600 * 1000);

  tick();
  token = PREVIEW ? null : load(TOKEN_KEY);
  if (token || PREVIEW) { loadState(); } else { startPairing(); }
}());
