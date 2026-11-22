# oldpad

Give an old iPad a second life as a home dashboard on the wall.

oldpad is one small server binary plus a deliberately old-fashioned web page.
The server does the heavy lifting — talking to [Homey](https://homey.app) or
[Home Assistant](https://www.home-assistant.io) — and the tablet only draws what it is
sent. It is built for devices newer dashboards have left behind: the tablet
page is written for **iOS 5 and later**, so an iPad 1, 2 or mini 1 is plenty.
In daily use on an iPad 2 with iOS 9.3.5; older versions are untested.

> Status: early. Integrations: Homey Pro, Home Assistant, Sonos, InfluxDB,
> Netatmo and Legrand (weather, thermostats, lights, sockets, cameras), MET Norway
> weather and warnings, iCal and CalDAV calendars, Entur departures, electricity
> prices, ships from your own AIS receiver, and a demo house. See
> [docs/design.md](docs/design.md).

## What works today

- **Pair with your phone.** The tablet shows a QR code; you scan it, sign in
  with a passkey (Face ID / Touch ID — there are no passwords), name the
  screen, and it is paired. The tablet keeps a long-lived token that renews
  itself with use and that you can revoke from your phone.
- **A home screen** with the clock, indoor and outdoor readings (the last 24 hours' low
  and high, humidity, CO₂, wind), the forecast for the coming hours and
  tomorrow, the room's Sonos (now playing, play/pause, volume, favourites) and
  six tiles: lights, sockets, thermostats, sensors or Homey Flows.
- **Menu pages** for music, lights, climate, Flows, a longer forecast, graphs,
  ships, departures, the calendar and cameras. A page shows when the screen has
  something for it, and you can add pages of your own.
- **Graphs.** The last 24 hours of temperature, humidity, CO₂ and wind for the
  screen's indoor, outdoor and wind readings, with the electricity price for the
  hours ahead. InfluxDB's history is used where it is the source. For anything
  else oldpad records a reading every 15 minutes, so those graphs fill up from
  the day you add them.
- **Doorbell pop-up.** Pick a doorbell or motion sensor from Homey or Home
  Assistant and optionally a camera per screen. When it goes off, the screen shows
  the picture for a minute, even at night. It is silent: old iPads do not let a
  web page play sound by itself.
- **Live.** Changes made anywhere else show up within seconds. After a dropped
  connection the screen fetches what it missed, and a banner says so if the
  server stays out of reach for 15 seconds. If an integration stops answering
  for two minutes, the screens showing it name it, as its values may be old.
- **Per screen.** Each screen has its own layout, chosen on your phone. Text
  follows the tablet's language (English and Norwegian so far).
- **Integrations by wizard.** Add any integration from your phone: pick a type,
  fill in the fields, test, save.
- **Day and night themes**, by hours you choose or by sunrise and sunset at the
  MET forecast's location, midnight sun and polar night included.
- **Night clock (optional, off by default).** Turn it on in a screen's settings
  (the cog). During night hours, after 5 minutes without a touch, the page gives
  way to a dim clock on black until the screen is touched or day starts. Orange
  and red weather warnings in force show under it. A web
  page cannot change the backlight, so set the iPad's brightness yourself.

## Run it

The container image is built for `linux/amd64` and `linux/arm64`, so it also runs
on a Raspberry Pi with a 64-bit OS:

```bash
docker run -d --name oldpad --restart unless-stopped -p 8080:8080 -v oldpad-data:/data -e OLDPAD_PUBLIC_URL=https://dash.example.com ghcr.io/bisand/oldpad:main
```

It runs as user 65534. A new named volume gets the right owner by itself. A host
folder, or a volume created by an older image, must be given to that user once:

```bash
docker run --rm -v oldpad-data:/data alpine chown -R 65534:65534 /data
```

Or build it yourself:

```bash
cargo build --release
```

```bash
OLDPAD_PUBLIC_URL=https://dash.example.com ./target/release/oldpad
```

On first start oldpad prints a **setup code** (`docker logs oldpad` in a container). Open `OLDPAD_PUBLIC_URL/admin`
on your phone, enter the code and create your passkey. Then open
`OLDPAD_PUBLIC_URL` on the tablet and scan the code it shows.

| Variable | Default | |
| --- | --- | --- |
| `OLDPAD_PUBLIC_URL` | `http://localhost:8080` | The address your phone uses. Must be `https://` (passkeys require it) unless it is `localhost`. Put a TLS-terminating proxy in front. |
| `OLDPAD_BIND` | `0.0.0.0:8080` | Where to listen, in plain HTTP: put an HTTPS proxy in front. |
| `OLDPAD_ADMIN_BIND` | unset | Optional separate port for the admin page, for setups that keep tablets on a private network. |
| `OLDPAD_DATA_DIR` | `data` | One JSON file with admins, screens and integrations, and one with the last day of readings. Keep it private. |
| `OLDPAD_DEVICE_TTL_DAYS` | `90` | A screen unused this long must pair again. |
| `OLDPAD_ADMIN_TTL_DAYS` | `30` | Phone sign-in lifetime. |
| `OLDPAD_LAN_IP` | unset | Any address on the home network, so integrations can scan it for devices (Sonos) where multicast discovery does not reach. In Kubernetes, set it to the node's address (`status.hostIP`). |
| `OLDPAD_LOG` | unset | Set to log every request and every event integrations receive (Homey's realtime messages included). Noisy; for troubleshooting. |

**On the iPad:** open the page in Safari, *Share → Add to Home Screen*, start
it from there (full screen), and lock it with *Settings → General →
Accessibility → Guided Access*.

## Exposing it to the internet

Admin and tablets share one HTTPS address, for example `https://dash.example.com`:

- Put a TLS-terminating proxy in front, and permanently redirect all HTTP to HTTPS.
- Old iPads do not trust Let's Encrypt's root. Install **ISRG Root X1** on each one before enabling the redirect, and compare its fingerprint with the one Let's Encrypt publishes.
- The proxy must buffer requests (except `/api/events`), rate-limit by client address, and set HSTS. Allow bodies up to 4 MB on `/api/admin/restore`, as nginx stops at 1 MB by default.
- Pairing needs the number shown on the screen. Backups, new admins, approving a screen, removing access and moving a stored key to a new address ask for a fresh passkey check.

## Why it is built this way

- **Old tablets need one extra root certificate.** iOS 9 and older do not trust
  today's Let's Encrypt root, so ISRG Root X1 is installed once. A tablet holds only
  a token scoped to its own tiles; the Homey API key never leaves the server.
- **The tablet page is ES5 and inline-block CSS.** No `fetch`, no flexbox, no
  frameworks. `cargo test` fails if modern JavaScript slips in.
- **Few dependencies, on purpose.** HTTP, WebSocket, CBOR and the passkey
  checks, the Sonos UPnP client and the Homey realtime client are written here
  and tested. Crates are used only for JSON (`serde_json`) and cryptography
  (`p256`, `sha2`, `getrandom`, and `rustls` for the integrations that reach the
  internet), plus a QR encoder. The container image is about 1.5 MB with the web
  pages built in.

## Contributing

Adding a home automation system is one Rust module: see
[docs/integrations.md](docs/integrations.md). The admin wizard is generated
from the fields you declare.

## License

[MIT](LICENSE)
