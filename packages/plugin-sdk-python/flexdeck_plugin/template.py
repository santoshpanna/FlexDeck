"""The plugin ``flexdeck-plugin new`` writes: every part used once, tested, packable."""

from __future__ import annotations

import json
from pathlib import Path

PLUGIN = '''"""NAME: a FlexDeck plugin.

    python -m unittest                    run its tests, without FlexDeck
    python -m flexdeck_plugin verify      ask FlexDeck itself whether it will work
    python -m flexdeck_plugin pack        write ID-0.1.0.fdpkg, ready to install

Everything a plugin can declare is used once below. Delete what you do not need.
"""

from flexdeck_plugin import Plugin, arg

# The id is the namespace for everything this plugin owns: its values are
# `ID.<name>`, and a key's action says plugin "ID". It must match plugin.json
# and can never change once people use it.
plugin = Plugin(
    id="ID",
    name="NAME",
    version="0.1.0",
    description="A starting point for a FlexDeck plugin.",
    author="you",
    # Shown before anyone installs it. Ask only for what you use.
    permissions=["notifications"],
)

# Configuration filled in once. A secret is never shown again once saved.
plugin.setting("greeting", "text", "The word to greet with", default="Hello")

# Live values a key can show and an automation can react to.
plugin.variable("ID.greetings", "number", "How many greetings so far")

# Things that happen, which an automation can be started by.
plugin.event("greeted", "Someone was greeted", fields=["name"])


# Every argument has an example that works: the editor fills it in, and the
# tests and `verify` run every action with it.
@plugin.action("greet", "Greet someone", {"name": arg("text", "world", "Who to greet")})
def greet(args, ctx):
    count = (ctx.vars.get("ID.greetings") or 0) + 1
    ctx.vars.set({"ID.greetings": count})
    ctx.emit("greeted", {"name": args["name"]})
    ctx.notify(f"{ctx.settings['greeting']}, {args['name']}!")


# Asked only for values something is showing.
@plugin.sample
def sample(wanted, ctx):
    return {"ID.greetings": ctx.vars.get("ID.greetings") or 0}


# A feed: values each key gets for itself, with its own arguments. A key
# reads them as {{this.word}}.
@plugin.feed("shout", "A word, louder", fields={"word": "text"}, arguments={"word": arg("text", "hello", "The word")})
def shout(instance, args, ctx):
    ctx.feed(instance).set({"word": args["word"].upper() + "!"})


# A ready-made key for the gallery.
plugin.preset(
    "shout",
    "Shout",
    {"sources": [{"source": "ID/shout"}], "face": {"layout": "FACE_LAYOUT_VALUE", "elements": [{"slot": "SLOT_VALUE", "template": "{{this.word}}"}]}},
    category="Plugins",
    description="A word, louder.",
)

if __name__ == "__main__":
    plugin.serve()
'''

TEST = '''import unittest

from flexdeck_plugin.testing import Harness, check_definition

from plugin import plugin


class NAMEClassTest(unittest.TestCase):
    def test_flexdeck_would_accept_the_definition(self):
        self.assertEqual(check_definition(plugin), [])

    def test_greeting_counts_and_announces(self):
        host = Harness(plugin, settings={"greeting": "Hi"})
        self.assertTrue(host.invoke("greet", {"name": "ada"})["ok"])
        self.assertEqual(host.value("ID.greetings"), 1)
        self.assertEqual(host.sent("event/emit"), [{"event": "greeted", "payload": {"name": "ada"}}])
        self.assertEqual(host.sent("notify")[0]["title"], "Hi, ada!")

    def test_a_feed_publishes_under_its_key(self):
        host = Harness(plugin)
        host.watch("shout", "w1", {"word": "hey"})
        self.assertEqual(host.value("ID.@w1.word"), "HEY!")


if __name__ == "__main__":
    unittest.main()
'''


def scaffold(target: Path, id: str, name: str) -> Path:
    """Write a new plugin into ``target`` (which must not exist)."""
    target = Path(target)
    if target.exists():
        raise FileExistsError(f"{target} already exists")
    (target / "tests").mkdir(parents=True)
    class_name = "".join(part.capitalize() for part in id.replace("-", "_").split("_")) or "Plugin"
    fill = lambda text: text.replace("NAMEClass", class_name).replace("ID", id).replace("NAME", name)  # noqa: E731
    (target / "plugin.py").write_text(fill(PLUGIN), encoding="utf-8")
    (target / "tests" / "__init__.py").write_text("", encoding="utf-8")
    (target / "tests" / "test_plugin.py").write_text(fill(TEST), encoding="utf-8")
    package = {
        "abi": 4,
        "id": id,
        "name": name,
        "version": "0.1.0",
        "description": "A starting point for a FlexDeck plugin.",
        "author": "you",
        "runtime": "python",
        "entry": "plugin.py",
        "permissions": ["notifications"],
    }
    (target / "plugin.json").write_text(json.dumps(package, indent=2) + "\n", encoding="utf-8")
    (target / "README.md").write_text(
        f"# {name}\n\nA FlexDeck plugin.\n\n    python -m unittest\n    python -m flexdeck_plugin verify\n    python -m flexdeck_plugin pack\n",
        encoding="utf-8",
    )
    return target
