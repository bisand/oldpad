// Presses a control without loading a new page, then changes only what differs,
// so the e-ink panel need not redraw the whole screen. Without this file the forms still work.
(function () {
  if (!window.XMLHttpRequest || !document.implementation || !document.implementation.createHTMLDocument) { return; }
  var busy = false;

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
    return true;
  }

  document.addEventListener('submit', function (ev) {
    var form = ev.target, xhr, fields = [], i, f;
    // Search is a plain link to its results.
    if ((form.getAttribute('method') || '').toLowerCase() !== 'post') { return; }
    xhr = new XMLHttpRequest();
    ev.preventDefault();
    if (busy) { return; }
    busy = true;
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
      if (!shown) { window.location.reload(); }
    };
    xhr.send(fields.join('&'));
  }, false);
}());
