# Design

Decisions made so far, with the reason for each. Open questions are at the end.

## Target device

First target: iPad 2, iOS 9.3.5, Safari 9, 512 MB RAM, 1024×768. The page is
written to run on iOS 5 and later. This sets the rules for the tablet page:

- **ES5 only**, `XMLHttpRequest` instead of `fetch`, no framework. Enforced by
  `tests/es5.rs`.
- **Inline-block layout**: no flexbox (iOS 5 has only the 2009 syntax), no grid.
- **Plain HTTP on the LAN.** iOS 9 does not trust ISRG Root X1, so Let's
  Encrypt certificates fail. The phone uses the normal HTTPS address.
- Few repaints, no heavy animation.
- Installed with *Add to Home Screen* (`apple-mobile-web-app-capable`) and
  locked with Guided Access.

## Server

Rust, standard library first. One thread per connection (capped at 128), one
request per connection, `Content-Length` bodies only, 16 KB head and 64 KB body
limits. SSE streams hold their thread for their lifetime, which is fine for a
household of screens. State is one JSON file, rewritten atomically.

**Dependency policy.** Protocols we can write and test ourselves are written
here: HTTP server and client, WebSocket client, CBOR, base64, the WebAuthn
parsing. Crates are allowed for JSON (`serde`, `serde_json`) and cryptography
(`p256`, `sha2`, `getrandom`), which is never hand-rolled, plus `qrcodegen`
(zero dependencies). No async runtime, no web framework.

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
- Pairing: 8-character code (no look-alike characters), 5-minute expiry, at
  most 10 pending, token handed to the polling tablet exactly once.
- Integration secrets are never returned to the browser.

## Architecture

```
 phone (HTTPS) ─┐
                ├──> oldpad server ──> Homey Pro (local Web API, API key)
 tablet (HTTP) ─┘        │        ──> Sonos speakers (UPnP, LAN)
       ^                 │        ──> InfluxDB 1.x (indoor/outdoor readings)
       └── SSE ──────────┘        ──> api.met.no (forecast, sunrise/sunset)
```

The server keeps one live connection to Homey and subscribes to Sonos events,
and forwards changes to every tablet over **Server-Sent Events** (supported
since Safari 5). The tablet never polls.

## Integrations

Compiled-in modules behind one trait; the admin wizard is generated from the
fields each declares. See [integrations.md](integrations.md).

| Source | How | Notes |
| --- | --- | --- |
| Homey Pro (2023) | Own HTTP + socket.io v2 client over WebSocket, see [homey-protocol.md](homey-protocol.md) | API key scopes: `homey.device.readonly`, `homey.device.control`, `homey.zone.readonly`, `homey.flow.readonly`, `homey.flow.start`. Control, but no admin (rename, delete, settings, users, apps). |
| Sonos | Direct on the LAN, not through Homey | Favourites with artwork, volume, play/pause, now playing. Homey's Sonos support is too thin for this. |
| Indoor / outdoor | InfluxDB 1.x query | Configurable measurement, field and tag filter per reading. Today's min/max alongside the current value. |
| Forecast | MET Norway Locationforecast + Sunrise | Next hours and tomorrow. Sunrise/sunset drives the day/night theme. |

## Pairing and tokens

1. An unpaired tablet asks for a pairing session and shows a QR code with a
   one-time code (5 minutes, single use).
2. The tablet waits on an SSE stream.
3. The phone scans, signs in with a **passkey** (WebAuthn, Face ID / Touch ID),
   names the device ("Kitchen") and picks a role (`kiosk` or `admin`).
4. The tablet receives its token and reloads into the dashboard.

The device token is one random opaque value, not an access/refresh pair. The
server stores only its hash, with name, role and last-seen time. It expires
after 90 days unused and every use pushes that forward, so a tablet in daily
use never expires. Revocation is server-side, from the phone. A refresh-token
dance only pays off when you cannot revoke on the server, and here you can.

## Configuration

Stored as JSON on the server, edited from the phone, pushed live to tablets.
A dashboard has pages, a page has tiles. Tile types:

`clock` · `weather` · `climate` (in/out) · `sonos-room` · `homey-device` ·
`homey-zone` · `homey-flow` · `image` (e.g. a rendered Grafana PNG) · `link`

## Home screen (iPad, landscape)

- Left rail: Home, Music, Lights, Climate, Flows, Weather, Graphs.
- Top: clock and date; indoor and outdoor temperature with today's min/max,
  humidity and CO₂ (amber above 1000 ppm).
- Forecast strip: now, the next hours, tomorrow.
- Sonos card for a **fixed room** per device: now playing, play/pause, volume,
  favourites.
- **Six** Homey tiles.
- Theme switches automatically at sunrise/sunset; night is dark with dim text.

## Open questions

- Graphs page (the kiosk's rendered Grafana PNGs) — needs an image proxy, as the CSP allows only same-origin images.
- Sonos album art (same reason), and UPnP event subscriptions instead of 4-second polling.
- More languages than English and Norwegian.
- Day/night by sunrise and sunset instead of fixed 07–21.
- Packaging: container image and an example Kubernetes manifest.
- Out-of-process integrations (a program speaking JSON over stdio), so
  integrations can be written in any language without a rebuild.
