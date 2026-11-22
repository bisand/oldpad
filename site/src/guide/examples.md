---
layout: doc.njk
title: Examples
description: Docker Compose, nginx and room-by-room ideas for oldpad.
---

## Docker Compose

```yaml
services:
  oldpad:
    image: ghcr.io/bisand/oldpad:latest
    restart: unless-stopped
    ports:
      - "127.0.0.1:8080:8080"
    environment:
      OLDPAD_PUBLIC_URL: https://dash.example.com
    volumes:
      - oldpad-data:/data

volumes:
  oldpad-data:
```

The port is bound to `127.0.0.1` so only the proxy on the same machine can reach it. On Linux, add `network_mode: host` instead of `ports` if you want Sonos found by multicast, and set `OLDPAD_BIND: 127.0.0.1:8080`.

<h2 id="nginx-in-front">nginx in front</h2>

Replace the name and certificate paths. This goes in the `http` context, for example in `/etc/nginx/conf.d/oldpad.conf`.

```nginx
limit_req_zone $binary_remote_addr zone=oldpad:10m rate=20r/s;

server {
    listen 80;
    server_name dash.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    server_name dash.example.com;
    ssl_certificate     /etc/letsencrypt/live/dash.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/dash.example.com/privkey.pem;
    add_header Strict-Transport-Security "max-age=31536000" always;

    location / {
        limit_req zone=oldpad burst=60 nodelay;
        proxy_pass http://127.0.0.1:8080;
    }

    # Live updates: a long-lived stream that must not be buffered.
    location /api/events {
        limit_req zone=oldpad burst=60 nodelay;
        proxy_pass http://127.0.0.1:8080;
        proxy_buffering off;
        proxy_request_buffering off;
        proxy_read_timeout 1h;
    }

    location = /api/admin/restore {
        limit_req zone=oldpad burst=60 nodelay;
        client_max_body_size 4m;
        proxy_pass http://127.0.0.1:8080;
    }
}
```

Install ISRG Root X1 on old iPads [before turning on the redirect](/guide/https/#old-ipads).

## Ideas for your rooms

**Kitchen.** Clock, forecast and the next departures on the home screen. Tiles for the worktop lights, the coffee maker and a "Good morning" scene.

**Hallway.** A Kindle by the door with a "Leaving home" scene and the outdoor temperature. Wake it on your way out.

**Living room.** The music page front and centre, with room grouping and a page of its own for evening lights.

**Bedroom.** Turn on the night clock so the screen stays dim until you touch it.

**Two matching screens.** Set one up, then let the other share its layout, so a change on your phone reaches both.
