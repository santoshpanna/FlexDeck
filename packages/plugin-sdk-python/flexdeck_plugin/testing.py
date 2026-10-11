"""Test a plugin without the agent: the host's side of the conversation, in process.

    from flexdeck_plugin.testing import Harness, check_definition
    from plugin import plugin

    def test_every_action_accepts_its_examples():
        assert check_definition(plugin) == []

    def test_toggle():
        host = Harness(plugin)
        assert host.invoke("toggle")["ok"]
        assert host.value("lights.on") is True

For the whole picture (run as FlexDeck runs it), ``flexdeck-plugin verify``.
"""

from __future__ import annotations

from typing import Any, Dict, List, Mapping, Optional

from . import ABI_VERSION, KINDS, Plugin


class Harness:
    """Plays the agent: ``initialize`` first, then whatever a test asks."""

    def __init__(self, plugin: Plugin, settings: Optional[Mapping[str, str]] = None, data_dir: Optional[str] = None) -> None:
        self.plugin = plugin
        plugin.sent.clear()
        self.introduction = plugin.handle("initialize", {"abi": ABI_VERSION, "settings": dict(settings or {}), "dataDir": data_dir})

    def invoke(self, action: str, args: Optional[Mapping[str, str]] = None) -> Dict[str, Any]:
        return self.plugin.handle("invoke", {"action": action, "args": dict(args or {})})

    def check(self, action: str, args: Optional[Mapping[str, str]] = None) -> Dict[str, Any]:
        return self.plugin.handle("check", {"action": action, "args": dict(args or {})})

    def sample(self, wanted: List[str]) -> Dict[str, Any]:
        return self.plugin.handle("sample", {"wanted": list(wanted)})["values"]

    def watch(self, feed: str, instance: str = "test", args: Optional[Mapping[str, str]] = None) -> None:
        self.plugin.handle("feed/watch", {"instance": instance, "feed": feed, "args": dict(args or {})})

    def unwatch(self, instance: str = "test") -> None:
        self.plugin.handle("feed/unwatch", {"instance": instance})

    def value(self, name: str) -> Any:
        """The last value published for ``name``, as the host would hold it."""
        found = None
        for message in self.plugin.sent:
            if message["method"] == "vars/set" and name in message["params"]["values"]:
                found = message["params"]["values"][name]["value"]
        return found

    def sent(self, method: str) -> List[Dict[str, Any]]:
        return [m["params"] for m in self.plugin.sent if m["method"] == method]


def check_definition(plugin: Plugin) -> List[str]:
    """Problems FlexDeck would find in the definition: arguments without a
    working example, values outside the namespace, actions that refuse their
    own examples. Empty: none."""
    problems: List[str] = []
    manifest = plugin.manifest()
    owners = [(a["action"], a["arguments"]) for a in manifest["actions"]] + [(f["name"], f["arguments"]) for f in manifest.get("feeds", [])]
    for owner, arguments in owners:
        for argument in arguments:
            where = f"{owner}.{argument['name']}"
            example = argument["example"]
            if argument["kind"] not in KINDS:
                problems.append(f"{where}: '{argument['kind']}' is not a kind")
            elif not example:
                if not argument["optional"]:
                    problems.append(f"{where} has no example")
            elif argument["kind"] == "number":
                try:
                    float(example)
                except ValueError:
                    problems.append(f"{where}: '{example}' is not a number")
            elif argument["kind"] == "bool" and example not in ("true", "false"):
                problems.append(f"{where}: '{example}' is not true or false")
    for variable in plugin.variables():
        if not variable["name"].startswith(f"{plugin.id}."):
            problems.append(f"{variable['name']} is not under {plugin.id}.")
    host = Harness(plugin)
    for action in manifest["actions"]:
        args = {a["name"]: a["example"] for a in action["arguments"] if a["example"]}
        answer = host.check(action["action"], args)
        if not answer["ok"]:
            problems.append(f"{action['action']} refuses its own examples: {answer.get('error')}")
    return problems
