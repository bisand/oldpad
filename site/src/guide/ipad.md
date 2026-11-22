---
layout: doc.njk
title: Set up an iPad
description: Pair an old iPad with oldpad and lock it to the dashboard.
---

The tablet page is written for iOS 5 and later. It is in daily use on an iPad 2 with iOS 9.3.5. Older versions are untested, so [tell us](https://github.com/bisand/oldpad/issues) how yours does.

## 1. Trust the certificate

iOS 9 and older do not trust today's Let's Encrypt root. If your address uses Let's Encrypt, install **ISRG Root X1** on the iPad once, and compare its fingerprint with the one [Let's Encrypt publishes](https://letsencrypt.org/certificates/).

## 2. Pair it

1. Open your oldpad address in Safari. The page shows a QR code, a code and a number.
2. Scan the QR code with your phone and sign in with your passkey.
3. Type the number shown on the tablet and name the screen, for example "Kitchen".

The tablet reloads into its dashboard. It keeps a token that renews with use, and you can revoke it from your phone at any time.

![The pairing screen with a QR code, a code and a number.](/img/pair.webp)

## 3. Make it feel like an appliance

1. In Safari, choose **Share**, then **Add to Home Screen**.
2. Start oldpad from the new icon, so it runs full screen.
3. Lock it with **Settings**, **General**, **Accessibility**, **Guided Access**.
4. Set the brightness yourself. A web page cannot change the backlight.

## 4. Arrange the screen

On your phone, open **Screens** and choose **Edit layout**. Each screen has six home tiles, its own panels for climate, weather and music, and up to 12 pages of your own. Another screen can share the same layout.

## Night clock

Turn this on with the cog on the tablet. During night hours, after 5 minutes without a touch, the page gives way to a dim clock on black. Orange and red weather warnings still show under it.

## Doorbell

Pick a doorbell or motion sensor from Homey or Home Assistant, and optionally a camera, for each screen. When it goes off, the picture shows for a minute, even at night. It is silent, because old iPads do not let a web page play sound by itself.
