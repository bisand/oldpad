---
layout: doc.njk
title: Reach it over HTTPS
description: Put oldpad behind a TLS proxy, safely, with old iPads in mind.
---

oldpad speaks plain HTTP and expects a proxy in front to handle TLS. Admin and tablets share one HTTPS address, for example `https://dash.example.com`.

## What the proxy must do

- Terminate TLS and permanently redirect all HTTP to HTTPS.
- Buffer requests, except on `/api/events`, which is a live stream.
- Rate-limit by client address.
- Set HSTS.
- Allow bodies up to 4 MB on `/api/admin/restore`. nginx stops at 1 MB by default.

A [ready-made nginx configuration](/guide/examples/#nginx-in-front) covers all of this.

<h2 id="old-ipads">Old iPads and Let's Encrypt</h2>

Old iPads do not trust Let's Encrypt's root. Install **ISRG Root X1** on each one **before** you turn on the redirect, and compare its fingerprint with the one [Let's Encrypt publishes](https://letsencrypt.org/certificates/). Otherwise the tablet can no longer reach the server to be fixed.

## What protects you if it is public

- Admins sign in with passkeys only. There are no passwords to steal.
- Pairing needs the number shown on the physical screen.
- Backups, new admins, approving a screen, removing access and moving a stored key to a new address ask for a fresh passkey check.
- A screen can only read and control its own tiles.
- Integration secrets are never sent to a browser.

To keep the admin page off the internet entirely, set `OLDPAD_ADMIN_BIND` to serve it on a separate port.
