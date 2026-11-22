# Design

Decisions made so far, with the reason for each. Open questions are at the end.

## Target device

First target: iPad 2, iOS 9.3.5, Safari 9, 512 MB RAM, 1024×768. The page is
written to run on iOS 5 and later. This sets the rules for the tablet page:

- **ES5 only**, `XMLHttpRequest` instead of `fetch`, no framework. Enforced by
  `tests/es5.rs`.
- **Inline-block layout**: no flexbox (iOS 5 has only the 2009 syntax), no grid.
- **HTTPS with one added root.** iOS 9 does not trust ISRG Root X1, so it is
  installed once on each old iPad. Tablets and phones share one HTTPS address.
- Few repaints, no heavy animation.
- Installed with *Add to Home Screen* (`apple-mobile-web-app-capable`) and
  locked with Guided Access.

## Server

Rust, standard library first. One thread per connection (capped at 128), one
request per connection, `Content-Length` bodies only, 16 KB head and 64 KB body
limits (4 MB for a signed-in admin restoring a backup). SSE streams hold their thread for their lifetime, which is fine for a
household of screens. State is one JSON file, rewritten atomically.

**Packaging.** A static musl binary on an empty base image, published to
`ghcr.io/bisand/oldpad` for amd64 and arm64. Each architecture builds on a native
runner and must start and answer `/healthz` before it is pushed; public tags move
only once both have passed. A push to `main` updates `main` and `sha-<commit>`. A
published GitHub release adds its semver tags, and the floating ones (`1.2`, `1`,
`latest`) move only when it is the newest release they cover. A released version
is never overwritten. The server checks GitHub's latest release every six hours
and only reports the result. A build from `main` says so rather than claiming to
be out of date, since it may be ahead of every release.

**Dependency policy.** Protocols we can write and test ourselves are written
here: HTTP server and client, WebSocket client, CBOR, base64, the WebAuthn
parsing. Crates are allowed for JSON (`serde`, `serde_json`) and cryptography
(`p256`, `sha2`, `getrandom`, and `rustls` for integrations that reach the
internet), which is never hand-rolled, plus `qrcodegen` (zero dependencies). No async runtime, no web framework.

**Security model.**
- Admins sign in with passkeys only (ES256, user verification required). The
  first passkey needs a setup code printed in the server log.
- Admin session: random token in an `HttpOnly; SameSite=Strict` cookie,
  `Secure` when the public URL is https. Only its SHA-256 is stored.
- Every non-GET request needs an `X-Oldpad: 1` header (forces a CORS preflight
  cross-site, which is never answered), and admin writes with a foreign
  `Origin` are refused.
- Screens: random bearer token, stored hashed, sliding 90-day expiry, revocable.
  A screen can read and control only its own tiles. Event streams use a
  one-time 60-second ticket so the token never appears in a URL.
- Pairing: 8-character code (no look-alike characters) plus a number shown on
  the screen, 5-minute expiry, at most 10 pending (the oldest gives way), token
  handed to the polling tablet exactly once.
- Integration secrets are never returned to the browser.

## Architecture

```
 phone (HTTPS) ──┐
                 ├──> oldpad server ──> home: Homey, Home Assistant, Sonos, InfluxDB, AIS
 tablet (HTTPS) ─┘        │        ──> internet: MET, Netatmo, Entur, prices, calendars
       ^                  │
       └── SSE ───────────┘
```

The server keeps one live connection to Homey, polls Sonos every 4 seconds, and
forwards changes to the tablets that show them over **Server-Sent Events**
(supported since Safari 5). The tablet does not poll for state: each time its
stream connects it fetches the current state once, so nothing changed during a
dropout is missed, and a banner appears if it stays disconnected for 15 seconds.
An integration that stops answering for two minutes is named in a bar on the
screens that show it, because its last values stay up.
Readings for the graphs come from InfluxDB where that is the source. For other
climate panels the server samples every 15 minutes and keeps a day in
`history.json`, separate from the state and from backups. An integration that is
down is skipped, so an outage shows as a gap rather than a flat line.
Only camera pictures and the graphs' history are fetched on a timer, while shown.

## Integrations

Compiled-in modules behind one trait; the admin wizard is generated from the
fields each declares. See [integrations.md](integrations.md).

| Source | How | Notes |
| --- | --- | --- |
| Homey Pro (2023) | Own HTTP + socket.io v2 client over WebSocket, see [homey-protocol.md](homey-protocol.md) | API key scopes: `homey.device.readonly`, `homey.device.control`, `homey.zone.readonly`, `homey.flow.readonly`, `homey.flow.start`. Control, but no admin (rename, delete, settings, users, apps). |
| Sonos | Direct on the LAN, not through Homey. Speakers are found by SSDP multicast like the Sonos app does; where multicast cannot reach (a Kubernetes pod) one known speaker address is enough, as every speaker reports the whole household's current addresses and oldpad re-reads them when a room stops answering. | Favourites with artwork, volume, play/pause, now playing. Homey's Sonos support is too thin for this. |
| Indoor / outdoor | InfluxDB 1.x query | Configurable measurement, field and tag filter per reading. The last 24 hours' min/max alongside the current value, and 15-minute averages for the graphs page, shared between screens for 5 minutes. |
| Forecast | MET Norway Locationforecast | Next hours and tomorrow. Sunrise and sunset are calculated from the coordinates, with no extra request, and can drive the day/night theme. |

## Pairing and tokens

1. An unpaired tablet asks for a pairing session and shows a QR code with a
   one-time code (5 minutes, single use).
2. The tablet waits on an SSE stream.
3. The phone scans, signs in with a **passkey** (WebAuthn, Face ID / Touch ID),
   enters the number shown on the screen and names it ("Kitchen").
4. The tablet receives its token and reloads into the dashboard.

The device token is one random opaque value, not an access/refresh pair. The
server stores only its hash, with name, role and last-seen time. It expires
after 90 days unused and every use pushes that forward, so a tablet in daily
use never expires. Revocation is server-side, from the phone. A refresh-token
dance only pays off when you cannot revoke on the server, and here you can.

## Configuration

Stored as JSON on the server, edited from the phone, pushed live to tablets.
A screen has six home tiles, fixed panels (indoor, outdoor, wind, forecast,
music, doorbell and its camera), further entities for the menu pages, and up to
12 pages of its own, made of entities and the home cards (`clock`, `climate`,
`forecast`, `music`). A screen can share another screen's layout.

## Home screen (iPad, landscape)

- Left rail: Home, Music, Lights, Climate, Flows, Weather, Graphs, Ships,
  Departures, Calendar, Cameras, then the screen's own pages. Pages with nothing
  to show are hidden.
- Top: clock and date; indoor and outdoor temperature with the last 24 hours' min/max,
  humidity and CO₂ (amber above 1000 ppm).
- Forecast strip: now, the next hours, tomorrow.
- Sonos card for a **fixed room** per device: now playing, play/pause, volume,
  favourites.
- **Six** Homey tiles.
- Theme switches by set hours or at sunrise/sunset; night is dark with dim text.
- Optional night clock per screen. It follows the set hours rather than the sun,
  so polar night cannot hide the dashboard all day. Orange and red weather
  warnings in force show under the clock.
- Doorbell pop-up above everything, including the night clock.

## Kindle in its own browser (no jailbreak)

Open `/kindle` in the Kindle's Experimental Browser. It is plain HTML and works without JavaScript, then reloading every five minutes. With JavaScript it listens to the tablet's event stream at `/kindle/events`, sends presses in the background and changes only what differs, to spare the panel a full redraw. Search results are not refreshed live, because each refresh would ask the music service.

- Pairing uses the normal code and PIN, with a QR code for the address. The screen then keeps an `HttpOnly` cookie scoped to `/kindle`.
- Each press is a form with a token derived from the screen, and a fingerprint of what the page showed. A stale page presses nothing and asks for another tap.
- It shows the same controls as the e-ink picture below, four across and as many rows as fit below the info boxes, each with its live value. Locks, alarms and unclassified devices stay off it.
- Info boxes above the buttons show indoor, outdoor, wind, the forecast and the screen's sensors, at most eight.
- Music follows the tablet's page: what the group plays, rooms to pick and group, favourites and search. Volume moves in 5 % steps, and it shows the first 24 favourites.
- A menu lists Home, Music, Lights, Climate, Scenes, Weather and the screen's own pages, leaving out empty ones. It sits at the bottom in portrait and on the left in landscape.
- Icons come from one grayscale sheet, `web/kindle-icons.png`, drawn from the tablet's icon paths. Its columns follow `ICONS` in `src/kindle.rs`.
- The Kindle still goes to sleep by itself, so this is a remote you wake, not an always-on display.

## E-ink screen (Kindle Paperwhite 3, portrait)

A reader shows a picture drawn by the server and reports where it was tapped. It pairs like any screen and sends its token as `Authorization: Bearer`.

- `GET /api/tablet/kindle.png?page=0&time=14:05` returns a 1072×1448 8-bit grayscale PNG with `X-Oldpad-Layout` and `X-Oldpad-Page`. `time` is the reader's own clock, because the server may be in another zone.
- `POST /api/tablet/kindle/tap` with `X-Oldpad: 1` and `{"x", "y", "page", "layout"}` presses what is at that point and returns `{"ok": true, "page": n}`. Fetch the picture for that page afterwards.
- A `409` means the picture is out of date and nothing was pressed: fetch it again.
- Buttons are the screen's own lights, sockets, thermostats and scenes, eight to a page. Locks, alarms and unclassified devices are left off.

## Open questions

- UPnP event subscriptions for Sonos instead of 4-second polling.
- Doorbell triggers from Netatmo. Homey and Home Assistant expose the `alarm_*`
  capabilities it needs today.
- More languages than English and Norwegian.
- An example Kubernetes manifest.
- Out-of-process integrations (a program speaking JSON over stdio), so
  integrations can be written in any language without a rebuild.
