// Presses controls and follows live changes without loading a new page, changing only
// what differs, so the e-ink panel need not redraw the whole screen. Without this file the forms still work.
(function () {
  if (!window.XMLHttpRequest || !document.implementation || !document.implementation.createHTMLDocument) { return; }
  var busy = false, timer = null, presses = 0;

  function sync(now, next) {
    var i, a;
    if (now.nodeType !== next.nodeType || now.nodeName !== next.nodeName) {
      now.parentNode.replaceChild(document.importNode(next, true), now);
    } else if (now.nodeType === 3) {
      if (now.nodeValue !== next.nodeValue) { now.nodeValue = next.nodeValue; }
    } else if (now.nodeType === 1) {
      for (i = now.attributes.length - 1; i >= 0; i--) {
        if (!next.hasAttribute(now.attributes[i].name)) { now.removeAttribute(now.attributes[i].name); }
      }
      for (i = 0; i < next.attributes.length; i++) {
        a = next.attributes[i];
        if (now.getAttribute(a.name) !== a.value) { now.setAttribute(a.name, a.value); }
      }
      if (now.childNodes.length !== next.childNodes.length) {
        now.innerHTML = next.innerHTML;
      } else {
        for (i = 0; i < now.childNodes.length; i++) { sync(now.childNodes[i], next.childNodes[i]); }
      }
    }
  }

  function show(html) {
    var body = /<body[^>]*>([\s\S]*)<\/body>/.exec(html), doc = document.implementation.createHTMLDocument('');
    if (!body) { return false; }
    doc.body.innerHTML = body[1];
    // Signed out: the answer is the pairing page.
    if (!doc.body.querySelector('.head')) { return false; }
    sync(document.body, doc.body);
    // A warning has said its piece once the page is current again.
    if (document.body.querySelector('.notice')) { later(5000); }
    return true;
  }

  function address() {
    return window.location.pathname + window.location.search.replace(/([?&])notice=[^&]*&?/, '$1').replace(/[?&]$/, '');
  }

  function later(ms) {
    if (!timer) { timer = setTimeout(function () { timer = null; refresh(); }, ms); }
  }

  function refresh() {
    var xhr, started = presses, field = document.activeElement;
    // Not under a finger or a half-typed search. Search results would ask the music service each time.
    if (busy || (field && field.tagName === 'INPUT')) { later(1500); return; }
    if (/[?&]q=[^&]/.test(window.location.search)) { return; }
    xhr = new XMLHttpRequest();
    xhr.open('GET', address(), true);
    xhr.onreadystatechange = function () {
      var shown = false;
      if (xhr.readyState !== 4 || xhr.status !== 200 || started !== presses) { return; }
      try { shown = show(xhr.responseText); } catch (e) { shown = false; }
      if (!shown) { window.location.reload(); }
    };
    xhr.send();
  }

  document.addEventListener('submit', function (ev) {
    var form = ev.target, xhr, fields = [], i, f;
    // Search is a plain link to its results.
    if ((form.getAttribute('method') || '').toLowerCase() !== 'post') { return; }
    xhr = new XMLHttpRequest();
    ev.preventDefault();
    if (busy) { return; }
    busy = true;
    presses++;
    for (i = 0; i < form.elements.length; i++) {
      f = form.elements[i];
      if (f.name) { fields.push(encodeURIComponent(f.name) + '=' + encodeURIComponent(f.value)); }
    }
    xhr.open('POST', form.getAttribute('action'), true);
    xhr.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded');
    xhr.onreadystatechange = function () {
      var shown = false;
      if (xhr.readyState !== 4) { return; }
      busy = false;
      try { shown = xhr.status === 200 && show(xhr.responseText); } catch (e) { shown = false; }
      // Reloading is a plain GET, so it can never press the control a second time.
      // With no answer at all, stay on this page: a reload would show the browser's error.
      if (!shown && xhr.status !== 0) { window.location.reload(); }
    };
    xhr.send(fields.join('&'));
  }, false);

  // The same stream the tablet listens to. The timer also covers a stream that went quiet.
  if (window.EventSource) {
    var events = new EventSource('/kindle/events');
    events.onopen = function () { later(100); };
    events.addEventListener('entity', function () { later(1000); }, false);
    events.addEventListener('reload', function () { later(100); }, false);
    events.addEventListener('refresh', function () { window.location.reload(); }, false);
    events.addEventListener('revoked', function () { window.location.reload(); }, false);
  }
  setInterval(function () { later(0); }, window.EventSource ? 60000 : 15000);
}());
