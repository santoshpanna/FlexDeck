# flexdeck-plugin (Python)

Write FlexDeck plugins in Python. It uses the standard library only, and a packed plugin carries the SDK with it, so it runs on any Python 3.10+ with nothing else installed.

    pip install flexdeck-plugin
    flexdeck-plugin new lights "Desk lights"
    cd lights
    python -m unittest                 # its tests, without FlexDeck
    flexdeck-plugin verify             # ask FlexDeck itself whether it will work
    flexdeck-plugin pack               # lights-0.1.0.fdpkg, ready to install

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

`verify` packs the plugin and runs it through the FlexDeck on this computer (`flexdeck verify-plugin`), exactly as an install would. It checks:
- its catalogue and gallery keys;
- every action's example arguments;
- its values, feeds and settings;
- that it keeps to its own namespace and its declared events;
- that it writes nothing but the protocol to stdout;
- that it stops cleanly.

`--run-actions` also runs every action with its examples, and those actions do real things. If FlexDeck is installed somewhere unusual, set `FLEXDECK_BIN` to its path.
