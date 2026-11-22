---
layout: doc.njk
title: How to contribute
permalink: /contribute/
description: Ways to help oldpad, with or without writing Rust.
---

You don't need to know Rust to help. Some of the most useful contributions need only an old device and a few minutes.

## Without writing code

- **Test an old device.** iPad 1, iPad mini 1 and iOS versions before 9 are untested. Try it and [open an issue](https://github.com/bisand/oldpad/issues) with the model, iOS version and what happened.
- **Try another e-reader.** Tell us whether its browser can show `/kindle`.
- **Translate it.** The interface is in English and Norwegian so far.
- **Improve these docs.** Every guide page is a Markdown file in `site/src/guide/`.
- **Share a photo** of your setup in an issue or discussion. It shows others what's possible.

## With code

Good places to start, all from the project's open questions:

- A new integration for a system you use. See [Writing an integration](/dev/integrations/).
- UPnP event subscriptions for Sonos, instead of polling every 4 seconds.
- Doorbell triggers from Netatmo.
- An example Kubernetes manifest.
- Out-of-process integrations, so they can be written in any language.

## Sending a change

1. Fork the repository and create a branch.
2. Run `cargo test` before you push.
3. Open a pull request that says what changed and why. If it touches the tablet page, say which device you tried it on.

## What we look for

- **It still runs on an old iPad.** The tablet page stays ES5, with no flexbox.
- **No new crates without a reason in the pull request.**
- **Secrets are never logged** or sent to a browser.
- **Integrations have unit tests** for how they map a system's data to entities.

By contributing, you agree that your work is released under the project's [MIT license](https://github.com/bisand/oldpad/blob/main/LICENSE).
