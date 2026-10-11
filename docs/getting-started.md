# Getting started

> Part of the [flexdeck docs](README.md).

You need a Windows PC and an Android tablet (or phone) running Android 5.0 or
newer, on the same Wi-Fi. Setup takes about five minutes, and you never create an
account.

## 1. Install the desktop app

Download the installer from **[flex-deck.com](https://flex-deck.com)** or the
[latest release](https://github.com/santoshpanna/FlexDeck/releases/latest) and run it.

flexdeck starts in the tray. Closing the window only hides it: the app keeps
serving your tablets until you quit it from the tray.

The first time it starts, Windows asks whether to let it through the firewall.
Allow it on **private networks**, or tablets will not be able to reach it.

## 2. Install the Android app

Install the APK from the
[latest release](https://github.com/santoshpanna/FlexDeck/releases/latest) on the
tablet. A Google Play listing is coming.

The app is native and light on purpose. It runs well on 2015-era tablets with old
WebViews and little RAM, which are exactly the tablets sitting in drawers.

## 3. Pair the tablet

1. On the PC, open flexdeck and go to **Pair**. It shows the PC's address and a
   six-digit code that is valid for two minutes.
2. On the tablet, type the address and the code.
3. The tablet remembers the PC and **pins its certificate**. From then on it
   reconnects by itself, and it refuses a PC that presents a different
   certificate.

Pair as many tablets as you like. Each one appears in the desktop app with a live
connected / not-connected light, and you can revoke any of them there.

## 4. Build your deck

Open the editor in a browser:

* **On the same PC**, open [flex-deck.com](https://flex-deck.com) and choose
  *Open the editor*. It finds the desktop app on this computer by itself.
* **On another device**, open the same page. In the desktop app, go to
  *Browsers → Connect a browser* and type the code it shows into the page.

Then drop actions on keys. The tablet updates live while you edit. Undo and redo
work the way you would expect.

Each key can have:

* **Four gestures**: tap, double tap, long press, and hold-and-release. Each one
  runs its own flow.
* **Looks that follow live values**, such as a key that turns orange while OBS is
  recording or shows the album art of the song that is playing.
* **Faders**, for volume and other ranges.

## 5. Add plugins

The desktop app's **Plugins** tab is a marketplace: OBS Studio, Discord, Spotify,
Streamer.bot, a soundboard, weather and more. Every plugin shows its permissions
before it installs. See [plugins.md](plugins.md) for using, writing and
publishing plugins.

## Troubleshooting

**The tablet cannot find the PC.**
Check that both devices are on the same network and that the firewall allows
flexdeck on private networks. Guest networks often isolate devices from each
other. The desktop app's **Home** tab lists the problems it can detect, including
the ones that only show up from a tablet.

**The browser warns about the certificate.**
That is expected once. The desktop app uses its own self-signed certificate,
which is what the tablet pins. The [security notes](security.md) explain why.

**Safari will not connect on the same Mac or PC.**
Safari can refuse an HTTPS page that calls `http://127.0.0.1`. Use the
*Connect a browser* code instead, which works in any browser.

**A key does nothing.**
Open the key in the editor and check that its action has a target. If the
action comes from a plugin, check in the desktop app that the plugin is running.
