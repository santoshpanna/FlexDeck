<a href="https://flex-deck.com">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/banner-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset=".github/assets/banner-light.svg">
    <img alt="flexdeck — every key, exactly yours. Turn the tablet in your drawer into a control surface." src=".github/assets/banner-dark.svg" width="100%">
  </picture>
</a>

<p align="center">
  <a href="https://flex-deck.com"><img alt="Website" src="https://img.shields.io/badge/flex--deck.com-ff5a1f?style=flat-square&labelColor=111113&logo=googlechrome&logoColor=white"></a>
  <a href="https://github.com/santoshpanna/FlexDeck/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/santoshpanna/FlexDeck?style=flat-square&color=ff5a1f&labelColor=111113&label=release"></a>
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%20%2F%2011-ecebe6?style=flat-square&labelColor=111113&logo=windows&logoColor=white">
  <img alt="Android 5.0+" src="https://img.shields.io/badge/Android-5.0%2B-ecebe6?style=flat-square&labelColor=111113&logo=android&logoColor=3ddc84">
  <a href="#license"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

<p align="center">
  Keys, faders, live states and automations for your PC,<br>
  on the tablet you already own. <b>Your Wi-Fi. No accounts.</b>
</p>

<p align="center">
  <a href="https://github.com/santoshpanna/FlexDeck/releases/latest"><img alt="Download for Windows" src=".github/assets/download-windows.svg" height="60"></a>
  &nbsp;
  <a href="https://github.com/santoshpanna/FlexDeck/releases/latest"><img alt="Download the Android APK" src=".github/assets/download-android.svg" height="60"></a>
</p>

<p align="center">
  <sub>Windows 10 / 11 · Android 5.0+ · then edit your deck at <a href="https://flex-deck.com">flex-deck.com</a></sub>
</p>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/features-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/features-light.svg">
  <img alt="Features: four gestures per key, keys that react to live values, faders, automations without code, an open plugin system, any grid on any tablet, Wi-Fi only with pinned TLS, no accounts." src=".github/assets/features-dark.svg" width="100%">
</picture>

<br><br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/how-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/how-light.svg">
  <img alt="How it works: install the desktop app, pair the tablet with six digits, build your deck in a browser. The tablet talks to the PC over your Wi-Fi with pinned TLS." src=".github/assets/how-dark.svg" width="100%">
</picture>

<br><br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/plugins-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/plugins-light.svg">
  <img alt="Plugins: OBS Studio, Discord, Spotify, Streamer.bot, Soundboard, Weather, Home Assistant, and your own." src=".github/assets/plugins-dark.svg" width="100%">
</picture>

<p align="center">
  Write your own in TypeScript, Python or Rust. <a href="docs/plugins.md"><b>Plugin guide →</b></a>
</p>

<br>

## Docs

**[Getting started](docs/getting-started.md)** · **[Plugins](docs/plugins.md)** ·
**[Architecture](docs/architecture.md)** · **[Security](docs/security.md)** ·
**[Development](docs/development.md)** · **[Releasing](docs/release.md)**

## Build from source

```sh
git clone https://github.com/santoshpanna/FlexDeck.git flexdeck && cd flexdeck
pnpm install
pnpm dev:desktop     # the desktop app
pnpm dev:web         # the web editor + relay
```

Needs Node 22+, Rust 1.88+, and JDK 17 for Android. The full setup is in
[docs/development.md](docs/development.md).

## Contributing

Issues, plugins and pull requests are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) first. Your first pull request asks you to sign
the [CLA](CLA.md) with a one-line comment. Report security issues privately, as
described in [SECURITY.md](SECURITY.md).

## License

[AGPL-3.0](LICENSE). The plugin SDKs, the plugin contract and the plugin template
are [MIT](packages/plugin-sdk/LICENSE), so your plugins can use any license.

<br>

<p align="center">
  <a href="https://flex-deck.com"><img src=".github/assets/logo.svg" width="36" height="36" alt="flexdeck"></a>
</p>
