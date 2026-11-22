---
layout: doc.njk
title: Developer guide
description: How oldpad is built, how to run it and where things live.
---

oldpad is one Rust binary plus a deliberately old-fashioned web page. The server does the heavy lifting and the tablet only draws what it is sent.

```text
 phone (HTTPS) ──┐
                 ├──> oldpad server ──> home: Homey, Home Assistant, Sonos, InfluxDB, AIS
 tablet (HTTPS) ─┘        │        ──> internet: MET, Netatmo, Entur, prices, calendars
       ^                  │
       └── SSE ───────────┘
```

## Run it locally

```bash
git clone https://github.com/bisand/oldpad.git
```

```bash
cd oldpad
```

```bash
cargo run
```

Open <http://localhost:8080/admin>, enter the setup code from the terminal, and add the **Demo house** integration. You now have something to work against with no hardware.

## Test it

```bash
cargo test
```

The tests include a full phone and tablet run over real HTTP, and a check that fails if modern JavaScript slips into the tablet page.

## Where things live

| Path | What it is |
| --- | --- |
| `src/main.rs`, `src/http.rs` | Startup and the HTTP server |
| `src/api.rs`, `src/app.rs` | Routes and application state |
| `src/store.rs` | The JSON file that holds everything |
| `src/webauthn.rs`, `src/cbor.rs` | Passkeys |
| `src/client.rs` | HTTP and WebSocket client for integrations |
| `src/integrations/` | One module per system. `demo.rs` is the smallest complete example. |
| `src/kindle.rs` | The Kindle pages |
| `web/tablet.*` | The tablet page, ES5 only |
| `web/admin.*` | The phone admin page |
| `docs/` | Design notes, shown on this site |
| `site/` | This website |

## The rules that keep it small

- **The tablet page is ES5 and inline-block CSS.** No `fetch`, no flexbox, no frameworks. `cargo test` enforces the JavaScript part.
- **Few dependencies, on purpose.** HTTP, WebSocket, CBOR and the passkey parsing are written here and tested. Crates are for JSON, cryptography and QR codes only.
- **No async runtime and no web framework.** One thread per connection, capped at 128, is plenty for a household of screens.
- **Cryptography is never hand-rolled.**

Read the [design decisions](/dev/design/) for the reasoning, or start [writing an integration](/dev/integrations/).

## Work on this website

The site is built with [Eleventy](https://www.11ty.dev) and ships no framework JavaScript. The developer pages are generated from `docs/`, so edit them there.

```bash
cd site
```

```bash
npm ci
```

```bash
npm run dev
```
