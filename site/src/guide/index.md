---
layout: doc.njk
title: Try it in two minutes
description: Run oldpad on your computer with a pretend smart home, no hardware needed.
---

You can try oldpad on your computer with a pretend house. You need [Docker](https://docs.docker.com/get-docker/) and a browser that supports passkeys.

## 1. Start it

```bash
docker run -d --name oldpad -p 8080:8080 -v oldpad-data:/data ghcr.io/bisand/oldpad:latest
```

## 2. Get the setup code

```bash
docker logs oldpad
```

The log shows a one-time setup code.

## 3. Create your passkey

Open <http://localhost:8080/admin>, enter the code and create a passkey. Passkeys replace passwords here, so you sign in with Touch ID, Face ID or your device's lock.

## 4. Add the demo house

Under **Integrations**, add **Demo house**. It has pretend lights, a heater, sensors, speakers and a scene.

## 5. Pair a screen

Open <http://localhost:8080> in a second browser window. It shows a pairing code and a number. In the admin page choose **Add screen**, enter them, name the screen and choose its tiles.

## Clean up

```bash
docker rm -f oldpad
```

```bash
docker volume rm oldpad-data
```

Liked it? [Install it on a server](/guide/install/) so your tablet can reach it.
