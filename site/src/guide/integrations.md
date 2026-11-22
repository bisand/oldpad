---
layout: doc.njk
title: Connect your home
description: Every system oldpad can talk to and what you need for each.
---

On your phone, open **Integrations**, pick a type, fill in the fields, test the connection and save.

| Integration | What it brings | What you need |
| --- | --- | --- |
| Homey Pro | Lights, sockets, thermostats, sensors and Flows | Its address and an API key |
| Home Assistant | Lights, switches, climate, sensors, doorbells, motion sensors, media players, cameras, scenes and scripts | Its address and a long-lived access token |
| Sonos | Play and pause, volume, now playing, favourites and room groups | Found on your network, or the address of one speaker |
| Netatmo | Weather stations, thermostats, Legrand and BTicino lights and sockets, camera pictures | A Netatmo account |
| InfluxDB | Indoor and outdoor readings from an InfluxDB 1.x database | Its address and your measurement names |
| MET Norway | Forecast for anywhere in the world, and weather warnings | Latitude and longitude |
| Calendar (iCal) | Events from Google, iCloud, Outlook and others | The calendar's iCal address |
| Calendar (CalDAV) | Events from a CalDAV account, read-only | Your CalDAV account |
| Entur | Next buses, trams, trains and boats in Norway, with realtime | Your stops |
| Electricity price | Today's and tomorrow's spot prices in Norway | Your price area, such as `NO1` |
| Ships nearby | Boats and ships around you | Your own AIS receiver |
| Demo house | A pretend home for trying oldpad | Nothing |

## Homey API key

Give the key only what oldpad needs. It controls devices but cannot rename, delete or change settings:

`homey.device.readonly`, `homey.device.control`, `homey.zone.readonly`, `homey.flow.readonly`, `homey.flow.start`

## Sonos in a container

Speakers are found by multicast. Where multicast does not reach, such as a Kubernetes pod, give oldpad one speaker's address, or set `OLDPAD_LAN_IP` to any address on your home network so it can scan for them.

## Your system isn't here?

Adding one is a single Rust module. See [Writing an integration](/dev/integrations/), or [open an issue](https://github.com/bisand/oldpad/issues) to ask for it.
