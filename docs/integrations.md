# Writing an integration

An integration connects oldpad to a home automation system. Homey Pro is the
first; `src/integrations/demo.rs` is the smallest complete example.

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
