# oldpad

Give an old iPad a second life as a home dashboard on the wall.

oldpad is a small self-hosted server plus a deliberately old-fashioned web page.
The server does the heavy lifting — talking to [Homey](https://homey.app),
Sonos, InfluxDB and the weather service — and the tablet only draws what it is
sent. It is built for devices that newer dashboards have left behind: the first
target is an **iPad 2 on iOS 9.3.5 (Safari 9, 512 MB RAM)**.

> Status: design phase. Nothing runs yet. See [docs/design.md](docs/design.md).

## What it will do

- **Home screen** — clock, indoor and outdoor temperature from InfluxDB, a short
  forecast from [MET Norway](https://api.met.no), Sonos now playing with volume
  and favourites for the room the tablet hangs in, and six Homey tiles.
- **Side menu** — music, lights, climate, Homey Flows, weather, graphs.
- **Pair with your phone** — the tablet shows a QR code, you scan it and approve
  with a passkey (Face ID / Touch ID), and the tablet gets a long-lived device
  token you can revoke at any time.
- **Configure from your phone** — pages, tiles, devices and Sonos room are edited
  on the phone and pushed live to the tablet.
- **Day and night themes** — switched at sunrise and sunset.

## Why the tablet is so dumb

Old Safari has no `fetch`, no modern JavaScript syntax, no CSS grid, and does
not trust today's Let's Encrypt root. So the tablet page is plain ES5 and
flexbox, receives live updates with Server-Sent Events, and is served over
plain HTTP on the LAN. Secrets (the Homey API key) never leave the server; the
tablet only holds a dashboard-scoped device token.

## License

[MIT](LICENSE)
