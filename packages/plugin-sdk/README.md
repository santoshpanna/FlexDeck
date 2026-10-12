<p align="center">
  <a href="https://flex-deck.com"><img alt="flexdeck" src="https://raw.githubusercontent.com/santoshpanna/FlexDeck/main/.github/assets/logo.svg" width="72"></a>
</p>

<h1 align="center">@flexdeck/plugin-sdk</h1>

<p align="center">
  Write <a href="https://flex-deck.com">FlexDeck</a> plugins in TypeScript or JavaScript.<br>
  You describe the plugin. The SDK speaks the protocol, then builds, packs and verifies it.
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@flexdeck/plugin-sdk"><img alt="npm" src="https://img.shields.io/npm/v/@flexdeck/plugin-sdk?style=flat-square&color=ff5a1f&labelColor=111113&logo=npm&logoColor=white"></a>
  <img alt="Node 22.18+" src="https://img.shields.io/badge/node-22.18%2B-ecebe6?style=flat-square&labelColor=111113&logo=nodedotjs&logoColor=white">
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md"><img alt="Plugin guide" src="https://img.shields.io/badge/docs-plugin%20guide-ecebe6?style=flat-square&labelColor=111113"></a>
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/packages/plugin-sdk/LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

## Start in a minute

```sh
npm create flexdeck-plugin lights "Desk lights"
cd lights
npm install
npm test              # its tests, without FlexDeck
npm run verify        # ask FlexDeck itself whether it will work
npm run package       # lights-0.1.0.fdpkg, ready to install
```

## A whole plugin

Actions, values, events, feeds and gallery keys all live in one definition:

```ts
import { definePlugin } from "@flexdeck/plugin-sdk";

export default definePlugin({
  id: "lights",
  name: "Desk lights",
  version: "0.1.0",
  permissions: ["network"],
  variables: { "lights.on": { kind: "bool", description: "Whether the lights are on" } },
  actions: {
    toggle: {
      description: "Turn the lights on or off",
      arguments: { room: { kind: "text", example: "desk" } },
      run: (_args, ctx) => ctx.vars.set({ "lights.on": !ctx.vars.get("lights.on") }),
    },
  },
});
```

## The CLI

`flexdeck-plugin <command> [dir]`

| Command  | What it does |
| -------- | ------------ |
| `build`  | Bundles the plugin's entry with esbuild. |
| `check`  | Builds, then confirms `plugin.json` and the code agree. |
| `pack`   | Writes the `.fdpkg` and prints its size and sha256. |
| `verify` | Packs, then has FlexDeck run it exactly as an install would. |
| `repo`   | Writes `repository.min.json` for a folder of packed plugins. |

## Verify before you publish

`flexdeck-plugin verify` hands the package to the FlexDeck on this computer (`flexdeck verify-plugin`), which runs it with the same host, deadlines and rules as a real install. It checks:

- [x] its package and catalogue, as the agent accepts them
- [x] its gallery keys, as a saved deck would be checked
- [x] every action's example arguments, checked by the plugin itself
- [x] its values, feeds and settings changes
- [x] that it never writes outside its own namespace, emits an event it didn't declare, or floods the host
- [x] that it writes nothing but the protocol to stdout
- [x] that it stops cleanly

> [!WARNING]
> `--run-actions` also runs every action with its examples, and those actions do real things.

If FlexDeck is installed somewhere unusual, set `FLEXDECK_BIN` to its path.

## Entry points

| Import | For |
| ------ | --- |
| `@flexdeck/plugin-sdk` | `definePlugin` and the plugin API |
| `@flexdeck/plugin-sdk/testing` | An in-process harness for unit tests |
| `@flexdeck/plugin-sdk/pack` | Packing `.fdpkg` files from your own scripts |
| `@flexdeck/plugin-sdk/repo` | Building a repository index |
| `@flexdeck/plugin-sdk/conformance` | Running `verify` programmatically |

## See also

- [Plugin guide](https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md): the full reference
- [Official plugins](https://github.com/Questory-Labs/FlexDeck-Plugins): OBS, Discord, Spotify and more, built with this SDK
- [`flexdeck`](https://github.com/santoshpanna/FlexDeck/tree/main/packages/plugin-sdk-python) for Python · [`flexdeck-plugin-sdk`](https://crates.io/crates/flexdeck-plugin-sdk) for Rust

<br>

<p align="center"><sub>MIT licensed, so your plugins can use any license.</sub></p>
