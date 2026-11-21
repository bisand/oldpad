# Writing an integration

An integration connects oldpad to a home automation system, such as Homey Pro
or Home Assistant. `src/integrations/demo.rs` is the smallest complete example.

## The shape of one

A module with `pub fn kind() -> Kind`, added to `kinds()` in
`src/integrations/mod.rs`:

```rust
Kind {
    id: "acme",                      // stored in config, never change it
    name: "Acme Hub",                // shown in the wizard
    description: "One sentence on what it connects.",
    fields: vec![Field { key: "address", label: "Hub address", kind: FieldKind::Url,
                         required: true, placeholder: "http://192.168.1.60",
                         help: "Where to find it." }],
    test,   // fn(&Map) -> Result<String, String>: the wizard's "Test connection"
    start,  // fn(&Map, Sink) -> Result<Box<dyn Instance>, String>
}
```

**The wizard is generated from `fields`.** Nothing in the admin UI needs to
change for a new integration. Field kinds: `Text`, `Url`, `Secret` (never sent
back to the browser; leaving it empty on edit keeps the stored value),
`Number`, `Bool`. Values are validated against the kinds before `test` or
`start` sees them.

**`start` returns an `Instance`**:

```rust
trait Instance: Send + Sync {
    fn entities(&self) -> Vec<Entity>;                     // current state, cheap
    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String>;
    fn stop(&self);                                        // end background threads
}
```

An instance may also implement `search(entity, query)`, returning `[{id, title, sub, kind}]` whose ids work with `play_favourite` and `add_favourite`. The tablet's music page shows a search box for it. Sonos searches the open radio-browser.info directory, and Spotify once connected.

**Accounts.** An instance can offer accounts to sign in to (`accounts()` returns `[{id, name, connected}]`); the admin page shows a Connect button for each. `connect(account)` returns the URL where the admin signs in and finishes in the background. Keep what it earns (a token) with `Event::Remember { key, value }`: the key must start with `_`. Such keys are stored with the integration's config, handed to `start` on the next start, kept when the admin edits the fields, and never sent to the browser. Sonos connects Spotify this way, through Sonos's own music-service sign-in (`src/integrations/smapi.rs`).

`start` may block (it runs on its own thread) and should fail with a clear
message the admin can act on. Push live changes through the `Sink`:
`Event::Changed { entity, capability, value }` for one value,
`Event::Reloaded` when entities were added, removed or renamed.

## Entities and capabilities

An `Entity` is anything a tile can show: a light, a thermostat, a sensor, a
scene. Capabilities use Homey's names so every integration maps onto one
vocabulary the tablet understands:

| Capability | Value | Tablet shows |
| --- | --- | --- |
| `onoff` | bool | tap to toggle |
| `dim` | 0.0–1.0 | percentage on a light that is on |
| `target_temperature` | number, `min`/`max` | − / + in 0.5° steps |
| `measure_temperature` | number | read-only reading |
| `button` | write-only | tap to run (a Flow, a scene) |
| `temperature_min`, `temperature_max` | number | the last 24 hours' range on the climate panel |
| `measure_humidity`, `measure_co2`, `measure_wind_strength` (km/h) | number | climate panel |
| `speaker_playing`, `volume_set` (0–1) | bool, number | music card controls |
| `speaker_track`, `speaker_artist` | string | music card text |
| `speaker_art` | image key for `image()` | cover or station logo on the music card, fetched through `/api/image/<integration>/<key>` |
| `favourites` / `play_favourite` | `[{id,title}]` / write-only id | favourite buttons |
| `add_favourite` | write-only id of a search result | ☆ on a search result saves it as a favourite |
| `speaker_group` | id of the room this one plays with (itself when alone or leading) | music page: tick rooms in or out of a group. Set it to another room's id to join that room's group, or to the room's own id to leave. A screen may only group rooms it controls. |
| `events` | `[{t, at, end, all, place}]`, epoch seconds; `all` means UTC midnights; `t` null for a private event | calendar agenda |
| `alerts` | `[{id, level (2–4), name, area, from, until, text, advice}]` on a `weather` entity | warning banner on every page, details on tap |
| `weather_symbol`, `forecast` | MET symbol code, `[[epoch, °C, symbol], …]` | forecast strip and weather page |

Entity `class` decides which menu page shows it: `light`/`socket` → lights,
`thermostat`/`heater`/`sensor` → climate, `button` → Flows, `speaker` → music
panel, `weather` → forecast panel, `calendar` → calendar page.

Mark a capability `settable: false` if it is read-only; the server refuses
writes to it before your `set` is called. A tablet can only control entities
an admin chose as its tiles.

## Rules

- **No new crates without a reason written in the PR.** oldpad hand-writes
  protocols (HTTP, WebSocket, CBOR) to keep the dependency tree small and
  reviewable. Cryptography is the exception: never hand-roll it.
- Use `crate::client::request` and `crate::client::WebSocket` for LAN devices.
- Never log secrets. Errors returned from `test`/`start` are shown to the admin.
- Add unit tests for your mapping from the system's data to `Entity`.
