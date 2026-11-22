---
layout: doc.njk
title: Install on a server
description: Run oldpad with Docker or from source, on a home server or Raspberry Pi.
---

oldpad needs one always-on machine on your network. A home server, a NAS or a Raspberry Pi with a 64-bit OS all work. The container image is built for `linux/amd64` and `linux/arm64`.

## What you need

- A machine that can run Docker, or a Rust toolchain to build it yourself.
- An HTTPS address for it, such as `https://dash.example.com`. Passkeys only work over HTTPS or on `localhost`. See [Reach it over HTTPS](/guide/https/).

## With Docker

```bash
docker run -d --name oldpad --restart unless-stopped -p 8080:8080 -v oldpad-data:/data -e OLDPAD_PUBLIC_URL=https://dash.example.com ghcr.io/bisand/oldpad:latest
```

The container runs as user 65534. A new named volume gets the right owner by itself. A host folder, or a volume created by an older image, must be handed over once:

```bash
docker run --rm -v oldpad-data:/data alpine chown -R 65534:65534 /data
```

## From source

```bash
cargo build --release
```

```bash
OLDPAD_PUBLIC_URL=https://dash.example.com ./target/release/oldpad
```

## First start

1. Find the **setup code** in the log (`docker logs oldpad`).
2. Open `https://dash.example.com/admin` on your phone.
3. Enter the code and create your passkey.
4. [Connect your home](/guide/integrations/).
5. Set up an [iPad](/guide/ipad/) or a [Kindle](/guide/kindle/).

## Which version to run

| Image tag | Points to |
| --- | --- |
| `1.2.3` | Exactly that release. It never changes, so pin this in production. |
| `1.2`, `1` | The newest release in that line. |
| `latest` | The newest stable release. |
| `main`, `sha-<commit>` | The latest push to `main`, and one commit. For development. |

oldpad never updates itself. The admin's Settings page shows the running version and whether a newer stable release exists. To check, the server asks GitHub's public releases API every six hours and sends nothing about your home.

## Back up

Your admins, screens and integrations live in `oldpad.json` in the data folder. It holds your integration secrets, so keep it and any copy of it private.
