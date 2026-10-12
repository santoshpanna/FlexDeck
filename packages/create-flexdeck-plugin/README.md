<p align="center">
  <a href="https://flex-deck.com"><img alt="flexdeck" src="https://raw.githubusercontent.com/santoshpanna/FlexDeck/main/.github/assets/logo.svg" width="72"></a>
</p>

<h1 align="center">create-flexdeck-plugin</h1>

<p align="center">
  A new <a href="https://flex-deck.com">FlexDeck</a> plugin, already building, tested and packable.
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/create-flexdeck-plugin"><img alt="npm" src="https://img.shields.io/npm/v/create-flexdeck-plugin?style=flat-square&color=ff5a1f&labelColor=111113&logo=npm&logoColor=white"></a>
  <img alt="Node 22.18+" src="https://img.shields.io/badge/node-22.18%2B-ecebe6?style=flat-square&labelColor=111113&logo=nodedotjs&logoColor=white">
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/packages/create-flexdeck-plugin/LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

```sh
npm create flexdeck-plugin lights "Desk lights"
```

That makes `./lights`, copied from the template the FlexDeck repository tests, so it builds, passes its tests and packs from the start. Then:

```sh
cd lights
npm install
npm test            # its tests, without FlexDeck
npm run verify      # ask FlexDeck itself whether it will work
npm run package     # lights-0.1.0.fdpkg
```

The plugin is written with [`@flexdeck/plugin-sdk`](https://www.npmjs.com/package/@flexdeck/plugin-sdk); its README and the [plugin guide](https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md) cover what to do next.

<br>

<p align="center"><sub>MIT licensed, and so is the template, so your plugin can use any license.</sub></p>
