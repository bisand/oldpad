/* oldpad tablet page.
 *
 * Written for the oldest browsers we support (iOS 5+ Safari): ES5 only, no
 * fetch, no Promise, no arrow functions, no let/const, no template strings.
 * `cargo test` runs a check that rejects modern syntax in this file.
 */
(function () {
  'use strict';

  var TOKEN_KEY = 'oldpad.token';
  var DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
  var MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

  var token = null;
  var tiles = [];
  var events = null;
  var pairTimer = null;
  var toastTimer = null;

  function $(id) { return document.getElementById(id); }

  function show(id) {
    $('pair').className = id === 'pair' ? 'screen' : 'screen hidden';
    $('home').className = id === 'home' ? 'screen' : 'screen hidden';
  }

  // Private browsing on old iOS throws on localStorage writes.
  function load(key) { try { return window.localStorage.getItem(key); } catch (e) { return null; } }
  function save(key, value) {
    try {
      if (value === null) { window.localStorage.removeItem(key); } else { window.localStorage.setItem(key, value); }
    } catch (e) { /* ignore */ }
  }

  function esc(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

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

  function unpair() {
    token = null;
    save(TOKEN_KEY, null);
    if (events) { events.close(); events = null; }
    startPairing();
  }

  function startPairing() {
    clearTimeout(pairTimer);
    show('pair');
    $('pair-status').textContent = 'Getting a code…';
    request('POST', '/api/pair', {}, {}, function (status, data) {
      if (status !== 200 || !data) {
        $('pair-status').textContent = (data && data.error) || 'Cannot reach the server. Retrying…';
        pairTimer = setTimeout(startPairing, 10000);
        return;
      }
      $('qr').innerHTML = data.qr;
      $('pair-url').textContent = data.url.replace(/#.*/, '');
      $('pair-code').textContent = data.code;
      $('pair-status').textContent = 'Waiting for approval…';
      pollPairing(data.id, data.secret);
    });
  }

  function pollPairing(id, secret) {
    pairTimer = setTimeout(function () {
      request('GET', '/api/pair/' + encodeURIComponent(id), null, { 'X-Pair-Secret': secret }, function (status, data) {
        if (data && data.status === 'approved') {
          token = data.token;
          save(TOKEN_KEY, token);
          loadHome();
        } else if (data && data.status === 'pending') {
          pollPairing(id, secret);
        } else if (status === 0) {
          pollPairing(id, secret); // network hiccup: keep the same code
        } else {
          startPairing(); // expired: show a fresh code
        }
      });
    }, 2000);
  }

  // ------------------------------------------------------------- home

  function loadHome() {
    request('GET', '/api/tablet', null, {}, function (status, data) {
      if (status !== 200 || !data) {
        toast('Cannot reach the server. Retrying…');
        setTimeout(loadHome, 10000);
        return;
      }
      $('device-name').textContent = data.device.name;
      tiles = data.tiles;
      render();
      show('home');
      connectEvents();
    });
  }

  function connectEvents() {
    if (events) { events.close(); events = null; }
    if (!window.EventSource) { setTimeout(loadHome, 60000); return; } // no live updates: refresh instead
    request('POST', '/api/tablet/ticket', {}, {}, function (status, data) {
      if (status !== 200 || !data) { setTimeout(connectEvents, 5000); return; }
      var es = new EventSource('/api/events?ticket=' + encodeURIComponent(data.ticket));
      events = es;
      es.addEventListener('entity', function (e) {
        var msg = JSON.parse(e.data);
        var tile = find(msg.id);
        if (tile && tile.capabilities && tile.capabilities[msg.capability]) {
          tile.capabilities[msg.capability].value = msg.value;
          render();
        }
      }, false);
      es.addEventListener('reload', function () { loadHome(); }, false);
      es.addEventListener('revoked', function () { unpair(); }, false);
      // The ticket is single use, so let the browser's own retry fail and get a new one.
      es.onerror = function () {
        es.close();
        if (events === es) { events = null; setTimeout(connectEvents, 5000); }
      };
    });
  }

  function find(id) {
    var i;
    for (i = 0; i < tiles.length; i++) { if (tiles[i].id === id) { return tiles[i]; } }
    return null;
  }

  function cap(tile, name) {
    return tile.capabilities && tile.capabilities[name] ? tile.capabilities[name] : null;
  }

  function fmt(n, digits) {
    if (typeof n !== 'number') { return '–'; }
    return n.toFixed(digits);
  }

  function tileHtml(tile, index) {
    var cls = 'tile';
    var body = '';
    if (tile.missing) {
      return '<div class="tile missing"><div class="in"><p class="label">Unavailable</p><p class="zone">' + esc(tile.id) + '</p></div></div>';
    }
    var onoff = cap(tile, 'onoff');
    var dim = cap(tile, 'dim');
    var target = cap(tile, 'target_temperature');
    var measure = cap(tile, 'measure_temperature');
    if (onoff) {
      if (onoff.value) { cls += ' on'; }
      body = '<div class="value">' + (onoff.value ? (dim ? Math.round(dim.value * 100) + '%' : 'On') : 'Off') + '</div>';
    } else if (target) {
      body = '<div class="value">' + fmt(target.value, 1) + '°' +
        (measure ? '<div class="sub">Now ' + fmt(measure.value, 1) + '°</div>' : '') + '</div>' +
        '<div class="steps"><span class="step" data-step="-0.5">−</span><span class="step" data-step="0.5">+</span></div>';
    } else if (cap(tile, 'button')) {
      body = '<div class="value">Run</div>';
    } else if (measure) {
      body = '<div class="value">' + fmt(measure.value, 1) + '°</div>';
    }
    return '<div class="' + cls + '" data-index="' + index + '"><div class="in">' +
      '<p class="label">' + esc(tile.name) + '</p><p class="zone">' + esc(tile.zone) + '</p>' + body + '</div></div>';
  }

  function render() {
    var html = '';
    var i;
    for (i = 0; i < tiles.length; i++) { html += tileHtml(tiles[i], i); }
    $('tiles').innerHTML = html;
    $('empty').className = tiles.length ? 'empty hidden' : 'empty';
  }

  function set(tile, capability, value, previous) {
    request('POST', '/api/tablet/set', { id: tile.id, capability: capability, value: value }, {}, function (status, data) {
      if (status !== 200) {
        if (previous !== undefined && tile.capabilities[capability]) {
          tile.capabilities[capability].value = previous;
          render();
        }
        toast((data && data.error) || 'That did not work.');
      }
    });
  }

  function closest(el, cls) {
    while (el && el !== document) {
      if ((' ' + el.className + ' ').indexOf(' ' + cls + ' ') >= 0) { return el; }
      el = el.parentNode;
    }
    return null;
  }

  function onTap(e) {
    var el = closest(e.target, 'tile');
    if (!el || el.getAttribute('data-index') === null) { return; }
    var tile = tiles[+el.getAttribute('data-index')];
    var step = closest(e.target, 'step');
    var c;
    if (step && (c = cap(tile, 'target_temperature'))) {
      var prev = c.value;
      var next = Math.round((prev + parseFloat(step.getAttribute('data-step'))) * 2) / 2;
      if (typeof c.min === 'number') { next = Math.max(c.min, next); }
      if (typeof c.max === 'number') { next = Math.min(c.max, next); }
      c.value = next;
      render();
      set(tile, 'target_temperature', next, prev);
    } else if ((c = cap(tile, 'onoff')) && c.settable) {
      c.value = !c.value;
      render();
      set(tile, 'onoff', c.value, !c.value);
    } else if (cap(tile, 'button')) {
      el.className += ' pressed';
      setTimeout(function () { render(); }, 400);
      set(tile, 'button', true);
    }
  }

  // ------------------------------------------------------------- clock & theme

  function pad(n) { return n < 10 ? '0' + n : String(n); }

  function tick() {
    var d = new Date();
    $('time').textContent = pad(d.getHours()) + ':' + pad(d.getMinutes());
    $('date').textContent = DAYS[d.getDay()] + ' ' + d.getDate() + ' ' + MONTHS[d.getMonth()];
    // shortcut: fixed 07–21 day; switch to sunrise/sunset once the weather integration lands.
    var h = d.getHours();
    document.body.className = h >= 7 && h < 21 ? 'day' : 'night';
  }

  document.addEventListener('click', onTap, false);
  tick();
  setInterval(tick, 10000);
  // Long-running kiosks leak; a nightly reload also picks up new versions.
  setTimeout(function () { window.location.reload(); }, 24 * 3600 * 1000);

  token = load(TOKEN_KEY);
  if (token) { loadHome(); } else { startPairing(); }
}());
