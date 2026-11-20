# Lights flows for Homey (zoneLights)

Buttons that switch all lights in some zones at once, and make sure they did. They live
in Homey, so they work from Homey's app and automations too, and oldpad shows each as a
tile that is **On while any of its lights is on** (it reads the zones from the Flow).

## Pieces in Homey

1. **HomeyScript `zoneLights`**: the logic, below. One copy, used by every lights Flow.
2. **One Advanced Flow per group**, e.g. "Lys: Andre etasje": a *Start* card ("This Flow is
   started") wired to a HomeyScript *code* card (`runCodeWithArg_v2`) whose code starts
   `zoneLights` and returns at once, with the zones as its argument:

   ```js
   // Starts the shared script "zoneLights" (all the logic lives there) and returns at once:
   // with 433 repeats and retries it can take half a minute, longer than a Flow card may wait.
   Homey.flow.runFlowCardAction({
     uri: 'homey:app:com.athom.homeyscript', id: 'homey:app:com.athom.homeyscript:runWithArg',
     args: { script: { id: '<zoneLights script id>', name: 'zoneLights' }, argument: args[0] },
   }).catch(() => {});
   await wait(1000); // long enough for Homey to have the request
   return true;
   ```

   Argument: `toggle: Stue, Spisestue` (or `on:` / `off:`). Zone names as Homey shows them,
   any case; sub-zones are included. New group: duplicate a Flow and change the argument.

Notes from setting this up over the Web API:
- The saved-script card (`runWithArg`) is not allowed in Advanced Flows; the code card is.
- The start card of an Advanced Flow is `{"type": "start"}`. With the `programmatic_trigger`
  card instead, Homey does not mark the Flow startable.
- Create Flows with the fields at the top level of the body, not under `advancedflow`.

## The script

Which lights: class *light*, or a plug Homey was told holds a light; not Device Groups
(📎), not unavailable ones (they keep a stale state that would spoil *toggle*).
oldpad's `House` in `src/integrations/homey.rs` picks the same lights for the tile state.

```js
// zoneLights: turn all lights in some zones on or off, and make sure they heard it.
// Argument: "<toggle|on|off>: Zone, Zone, …"   (sub-zones are included)
//           prefix "dry " to only list what would happen, e.g. "dry toggle: Stue"
// toggle: off if any light is on, otherwise on.
// Lights that confirm (Zigbee, Z-Wave, Hue, Wi-Fi) are re-sent until they report the new state.
// 433 MHz lights can't confirm, so they always get the command several times.
// Made for oldpad; called from Advanced Flows "Lys: …".

const RF433 = ['se.nexa', 'se.luxorparts', 'eu.elro.flamingo', 'se.telldus', 'com.everflourish', 'nl.klikaanklikuit', 'com.kaku'];
const RF_REPEATS = 3;       // sends per 433 light
const RF_GAP = 250;         // ms between 433 sends (each send itself takes ~0.7 s): one radio, so one at a time
const TRIES = 3;            // attempts for lights that confirm
const SETTLE = 3000;        // ms to wait before checking they did

const arg = String(args[0] || '').trim();
const dry = /^dry\s+/i.test(arg);
const m = arg.replace(/^dry\s+/i, '').match(/^(toggle|on|off)\s*:\s*(.+)$/i);
if (!m) throw new Error('Argument: "toggle|on|off: Zone, Zone"');
const action = m[1].toLowerCase();
// Zone names may hold non-breaking spaces; compare loosely.
const norm = (s) => String(s).normalize('NFKC').replace(/\s+/g, ' ').trim().toLowerCase();
const wanted = m[2].split(',').map(norm).filter(Boolean);

const zones = Object.values(await Homey.zones.getZones());
const roots = zones.filter((z) => wanted.includes(norm(z.name)));
const missing = wanted.filter((w) => !roots.some((z) => norm(z.name) === w));
if (missing.length) throw new Error(`No zone named: ${missing.join(', ')}`);
// The chosen zones and everything under them.
const inside = new Set(roots.map((z) => z.id));
for (let grew = true; grew;) {
  grew = false;
  for (const z of zones) if (z.parent && inside.has(z.parent) && !inside.has(z.id)) { inside.add(z.id); grew = true; }
}

const app = (d) => (d.driverId || d.driverUri || '').replace(/^homey:app:/, '').split(':')[0];
const all = Object.values(await Homey.devices.getDevices()).filter((d) =>
  inside.has(d.zone) && (d.class === 'light' || d.virtualClass === 'light') && d.capabilities.includes('onoff')
  && app(d) !== 'com.swttt.devicegroups');
// An unavailable device keeps its last state, which would spoil the toggle: leave it out.
const gone = all.filter((d) => d.available === false);
const lights = all.filter((d) => d.available !== false);
const goneNote = gone.length ? `; unavailable: ${gone.map((d) => d.name).join(', ')}` : '';
if (!lights.length) return `No lights in ${roots.map((z) => z.name).join(', ')}`;

const isOn = (d) => d.capabilitiesObj && d.capabilitiesObj.onoff && d.capabilitiesObj.onoff.value === true;
const target = action === 'on' ? true : action === 'off' ? false : !lights.some(isOn);
const rf = lights.filter((d) => RF433.includes(app(d)));
const confirming = lights.filter((d) => !rf.includes(d));
const word = target ? 'on' : 'off';

if (dry) {
  return `Would turn ${word}: ${confirming.map((d) => d.name).join(', ') || '–'}; `
    + `433 MHz ×${RF_REPEATS}: ${rf.map((d) => d.name).join(', ') || '–'}` + goneNote;
}

const send = (d) => d.setCapabilityValue('onoff', target).then(() => true, (e) => { console.log(`${d.name}: ${e.message}`); return false; });

// 433: round-robin, so each light's repeats are spread out in time.
const rfDone = (async () => {
  for (let round = 0; round < RF_REPEATS; round++) {
    for (const d of rf) { await send(d); await wait(RF_GAP); }
  }
})();

// The rest: send, wait, re-read, re-send to those that didn't take it.
let left = confirming;
for (let attempt = 1; attempt <= TRIES && left.length; attempt++) {
  for (const d of left) { send(d); await wait(150); } // a little apart, not to flood the mesh
  await wait(SETTLE);
  const now = await Promise.all(left.map((d) => Homey.devices.getDevice({ id: d.id }).catch(() => d)));
  left = now.filter((d) => isOn(d) !== target);
  if (left.length) console.log(`attempt ${attempt}: not yet ${word}: ${left.map((d) => d.name).join(', ')}`);
}
await rfDone;

const summary = `${lights.length} lights ${word}` + (left.length ? `; no answer from ${left.map((d) => d.name).join(', ')}` : '') + goneNote;
if (left.length) {
  // A timeline note, so a lamp that keeps missing gets noticed.
  await Homey.flow.runFlowCardAction({ uri: 'homey:manager:notifications', id: 'homey:manager:notifications:create_notification', args: { text: `Lights: ${summary}` } })
    .catch((e) => console.log(`timeline: ${e.message}`));
}
console.log(summary);
return summary;
```
