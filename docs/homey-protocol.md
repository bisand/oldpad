# Homey Pro local API, as oldpad uses it

Read out of Athom's official `homey-api` 3.21.3 (`lib/HomeyAPI/HomeyAPIV3/`)
so oldpad can talk to Homey without the SDK and its dependency tree.

## REST

Base: `http://<homey>/api/manager/<manager><path>`, header
`Authorization: Bearer <api key>`, JSON bodies.

| What | Request | Scope |
| --- | --- | --- |
| Homey ID (no auth) | `GET /api/manager/system/ping` → header `X-Homey-ID` | none |
| Devices | `GET /api/manager/devices/device` | `homey.device.readonly` |
| Set capability | `PUT /api/manager/devices/device/:deviceId/capability/:capabilityId` body `{"value": …}` | `homey.device.control` |
| Zones | `GET /api/manager/zones/zone` | `homey.zone.readonly` |
| Flows | `GET /api/manager/flow/flow`, `GET /api/manager/flow/advancedflow` | `homey.flow.readonly` |
| Run flow | `POST /api/manager/flow/flow/:id/trigger`, `POST /api/manager/flow/advancedflow/:id/trigger` | `homey.flow.start` |

## Realtime: socket.io v2 (Engine.IO 3) over a WebSocket

1. Open `ws://<homey>/socket.io/?EIO=3&transport=websocket`.
2. Server sends `0{"sid":…,"pingInterval":…,"pingTimeout":…}` then `40`.
3. Handshake on the default namespace, with an ack id:
   `421["handshakeClient",{"token":"<api key>","homeyId":"<id>"}]`
   → `431[null,{"namespace":"/api/…"}]` (first element is the error).
4. Join that namespace: send `40/api/…`, wait for `40/api/…`.
5. Subscribe per item URI, with an ack:
   `42/api/…,2["subscribe","homey:device:<deviceId>"]` → `43/api/…,2[null]`.
   Manager-wide events use `homey:manager:devices`.
6. Events arrive as `42/api/…,["homey:device:<id>","capability",{"capabilityId":…,"value":…,"transactionId":…}]`:
   the event name is the URI, followed by the item event and its data.
7. Keep-alive: the client sends `2` every `pingInterval` ms, the server answers `3`.

Item URIs are `homey:<itemType>:<id>`. Subscribe timeout in the SDK is 10 s.
