# Design

Decisions made so far, with the reason for each. Open questions are at the end.

## Target device

iPad 2, iOS 9.3.5, Safari 9, 512 MB RAM, 1024×768. This sets the rules for the
tablet page:

- **ES5 only**, `XMLHttpRequest` instead of `fetch`, no framework.
- **Flexbox, no CSS grid** (grid arrived in Safari 10.1).
- **Plain HTTP on the LAN.** iOS 9 does not trust ISRG Root X1, so Let's
  Encrypt certificates fail. The phone uses the normal HTTPS address.
- Few repaints, no heavy animation.
- Installed with *Add to Home Screen* (`apple-mobile-web-app-capable`) and
  locked with Guided Access.

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

| Source | How | Notes |
| --- | --- | --- |
| Homey Pro (2023) | `homey-api`, `HomeyAPI.createLocalAPI({ address, token })` | API key scopes: `homey.device.readonly`, `homey.device.control`, `homey.zone.readonly`, `homey.flow.readonly`, `homey.flow.start`. Control, but no admin (rename, delete, settings, users, apps). |
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

- Server language and storage (Node is the natural fit for `homey-api`).
- How the phone reaches the server for the first passkey registration.
- Packaging: container image and an example Kubernetes manifest.
