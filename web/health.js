// Tells the admin's Screens page how this screen is doing. Must load before the page's own script.
(function () {
  if (!window.XMLHttpRequest || !window.JSON) { return; }
  var drops = 0, errors = 0, error = '', ms = null, version = null, send = null;

  window.addEventListener('error', function (e) {
    errors++;
    error = String(e.message || 'Script error').slice(0, 200);
  }, false);

  function report() {
    var started = new Date().getTime(), sentDrops = drops, sentErrors = errors;
    if (!send) { return; }
    send({ ms: ms, drops: sentDrops, errors: sentErrors, error: error, version: version, width: window.innerWidth, height: window.innerHeight }, function (status, data) {
      if (status !== 200) { return; }
      ms = new Date().getTime() - started;
      drops -= sentDrops;
      errors -= sentErrors;
      if (!errors) { error = ''; }
      if (data && data.version) { version = data.version; }
    });
  }

  window.oldpadHealth = {
    drop: function () { drops++; },
    start: function (sender) {
      send = sender;
      setTimeout(report, 3000);
      // The second report carries what the first one measured.
      setTimeout(report, 6000);
      setInterval(report, 60000);
    }
  };
}());
