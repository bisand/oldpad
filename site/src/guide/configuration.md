---
layout: doc.njk
title: Settings reference
description: Every environment variable oldpad reads.
---

The server is configured with environment variables. Everything else is set from your phone.

| Variable | Default | What it does |
| --- | --- | --- |
| `OLDPAD_PUBLIC_URL` | `http://localhost:8080` | The address your phone uses. Must be `https://` unless it is `localhost`, because passkeys require it. |
| `OLDPAD_BIND` | `0.0.0.0:8080` | Where to listen, in plain HTTP. Put an HTTPS proxy in front. |
| `OLDPAD_ADMIN_BIND` | unset | Optional separate port for the admin page, for setups that keep tablets on a private network. |
| `OLDPAD_DATA_DIR` | `data` (`/data` in the image) | Holds one JSON file with admins, screens and integrations, and one with the last day of readings. Keep it private. |
| `OLDPAD_DEVICE_TTL_DAYS` | `90` | A screen unused this long must pair again. |
| `OLDPAD_ADMIN_TTL_DAYS` | `30` | How long a phone stays signed in. |
| `OLDPAD_LAN_IP` | unset | Any address on the home network, so integrations can scan it for devices such as Sonos where multicast does not reach. In Kubernetes, use the node's address (`status.hostIP`). |
| `OLDPAD_LOG` | unset | Logs every request and integration event. Noisy, for troubleshooting. |

## Themes

In the admin's Settings page, screens can switch between day and night by hours you choose, or by sunrise and sunset at the MET forecast's location. Midnight sun and polar night are handled.

## Language

Text follows the tablet's language. English and Norwegian are available so far, and you can force one in Settings.
