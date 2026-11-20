# oldpad

Give an old iPad a second life as a home dashboard on the wall.

oldpad is one small server binary plus a deliberately old-fashioned web page.
The server does the heavy lifting — talking to [Homey](https://homey.app) and,
later, other home automation systems — and the tablet only draws what it is
sent. It is built for devices newer dashboards have left behind: the tablet
page runs on **iOS 5 and later**, so an iPad 1, 2 or mini 1 is plenty.

> Status: early. Integrations: Homey Pro, Sonos,
> InfluxDB, MET Norway weather, and a demo house. See [docs/design.md](docs/design.md).

## What works today

- **Pair with your phone.** The tablet shows a QR code; you scan it, sign in
  with a passkey (Face ID / Touch ID — there are no passwords), name the
  screen, and it is paired. The tablet keeps a long-lived token that renews
  itself with use and that you can revoke from your phone.
- **A home screen** with the clock, indoor and outdoor readings (the last 24 hours' low
  and high, humidity, CO₂, wind), the forecast for the coming hours and
  tomorrow, the room's Sonos (now playing, play/pause, volume, favourites) and
  six tiles: lights, sockets, thermostats, sensors or Homey Flows.
- **Menu pages** for music, lights, climate, Flows and a longer forecast.
- **Live.** Changes made anywhere else show up within seconds.
- **Per screen.** Each screen has its own layout, chosen on your phone. Text
  follows the tablet's language (English and Norwegian so far).
- **Integrations by wizard.** Add Homey, Sonos, InfluxDB, weather or the demo
  house from your phone: pick a type, fill in the fields, test, save.
- **Day and night themes.**

## Run it

```bash
cargo build --release
OLDPAD_PUBLIC_URL=https://dash.example.com ./target/release/oldpad
```

On first start oldpad prints a **setup code**. Open `OLDPAD_PUBLIC_URL/admin`
on your phone, enter the code and create your passkey. Then open
`http://<server>:8080/` on the tablet and scan the code it shows.

| Variable | Default | |
| --- | --- | --- |
| `OLDPAD_PUBLIC_URL` | `http://localhost:8080` | The address your phone uses. Must be `https://` (passkeys require it) unless it is `localhost`. Put a TLS-terminating proxy in front. |
| `OLDPAD_BIND` | `0.0.0.0:8080` | Where to listen. Tablets connect here over plain HTTP. |
| `OLDPAD_DATA_DIR` | `data` | One JSON file with admins, screens and integrations. Keep it private. |
| `OLDPAD_DEVICE_TTL_DAYS` | `90` | A screen unused this long must pair again. |
| `OLDPAD_ADMIN_TTL_DAYS` | `30` | Phone sign-in lifetime. |
| `OLDPAD_LOG` | unset | Set to log every request and every event integrations receive (Homey's realtime messages included). Noisy; for troubleshooting. |

**On the iPad:** open the page in Safari, *Share → Add to Home Screen*, start
it from there (full screen), and lock it with *Settings → General →
Accessibility → Guided Access*.

## Why it is built this way

- **Old tablets get plain HTTP.** iOS 9 and older do not trust today's Let's
  Encrypt root, so the tablet talks plain HTTP on your LAN. It holds only a
  token scoped to its own tiles; the Homey API key never leaves the server.
- **The tablet page is ES5 and inline-block CSS.** No `fetch`, no flexbox, no
  frameworks. `cargo test` fails if modern JavaScript slips in.
- **Few dependencies, on purpose.** HTTP, WebSocket, CBOR and the passkey
  checks, the Sonos UPnP client and the Homey realtime client are written here
  and tested. Crates are used only for JSON (`serde_json`) and cryptography
  (`p256`, `sha2`, `getrandom`, and `rustls` for the one internet service, the
  weather), plus a QR encoder. The container image is about 1 MB with the web
  pages built in.

## Contributing

Adding a home automation system is one Rust module: see
[docs/integrations.md](docs/integrations.md). The admin wizard is generated
from the fields you declare.

## License

[MIT](LICENSE)
