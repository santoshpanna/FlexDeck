<p align="center">
  <a href="https://flex-deck.com"><img alt="flexdeck" src="https://raw.githubusercontent.com/santoshpanna/FlexDeck/main/.github/assets/logo.svg" width="72"></a>
</p>

<h1 align="center">flexdeck (Python)</h1>

<p align="center">
  Write <a href="https://flex-deck.com">FlexDeck</a> plugins in Python.<br>
  Standard library only. A packed plugin carries the SDK with it, so it runs on any Python 3.10+ with nothing else installed.
</p>

<p align="center">
  <img alt="Python 3.10+" src="https://img.shields.io/badge/python-3.10%2B-ecebe6?style=flat-square&labelColor=111113&logo=python&logoColor=white">
  <img alt="No dependencies" src="https://img.shields.io/badge/dependencies-none-ecebe6?style=flat-square&labelColor=111113">
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md"><img alt="Plugin guide" src="https://img.shields.io/badge/docs-plugin%20guide-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

## Start in a minute

```sh
pip install flexdeck
flexdeck-plugin new lights "Desk lights"
cd lights
python -m unittest                 # its tests, without FlexDeck
flexdeck-plugin verify             # ask FlexDeck itself whether it will work
flexdeck-plugin pack               # lights-0.1.0.fdpkg, ready to install
```

## A whole plugin

```python
from flexdeck_plugin import Plugin, arg

plugin = Plugin(id="lights", name="Desk lights", version="0.1.0", permissions=["network"])
plugin.variable("lights.on", "bool", "Whether the lights are on")

@plugin.action("toggle", "Turn the lights on or off", {"room": arg("text", "desk", "Which room")})
def toggle(args, ctx):
    ctx.vars.set({"lights.on": not ctx.vars.get("lights.on")})

if __name__ == "__main__":
    plugin.serve()
```

## Verify before you publish

`verify` packs the plugin and runs it through the FlexDeck on this computer (`flexdeck verify-plugin`), exactly as an install would. It checks:

- [x] its catalogue and gallery keys
- [x] every action's example arguments
- [x] its values, feeds and settings
- [x] that it keeps to its own namespace and its declared events
- [x] that it writes nothing but the protocol to stdout
- [x] that it stops cleanly

> [!WARNING]
> `--run-actions` also runs every action with its examples, and those actions do real things.

If FlexDeck is installed somewhere unusual, set `FLEXDECK_BIN` to its path.

## See also

- [Plugin guide](https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md): the full reference
- [Official plugins](https://github.com/Questory-Labs/FlexDeck-Plugins)
- [`@flexdeck/plugin-sdk`](https://www.npmjs.com/package/@flexdeck/plugin-sdk) for TypeScript · [`flexdeck-plugin-sdk`](https://crates.io/crates/flexdeck-plugin-sdk) for Rust

<br>

<p align="center"><sub>MIT licensed, so your plugins can use any license.</sub></p>
