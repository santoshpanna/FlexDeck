import json
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))

from flexdeck_plugin import Plugin, PluginError, arg  # noqa: E402
from flexdeck_plugin.conformance import conform, describe, find_flexdeck, passed  # noqa: E402
from flexdeck_plugin.pack import check, pack  # noqa: E402
from flexdeck_plugin.template import scaffold  # noqa: E402
from flexdeck_plugin.testing import Harness, check_definition  # noqa: E402


def lights():
    plugin = Plugin(id="lights", name="Lights", version="1.0.0", permissions=["network"])
    plugin.variable("lights.on", "bool", "on")
    plugin.event("toggled", fields=["on"])

    @plugin.action("toggle", "Toggle", {"room": arg("text", "desk")})
    def toggle(args, ctx):
        on = not ctx.vars.get("lights.on")
        ctx.vars.set({"lights.on": on})
        ctx.emit("toggled", {"on": str(on).lower()})
        return {"state": "on" if on else "off"}

    @plugin.action("fail", "Fail")
    def fail(args, ctx):
        raise RuntimeError("the bridge is unreachable")

    return plugin


class DefinitionTest(unittest.TestCase):
    def test_a_definition_flexdeck_would_refuse_is_refused_when_written(self):
        with self.assertRaisesRegex(PluginError, "usable id"):
            Plugin(id="Lights!", name="x", version="1.0.0")
        plugin = Plugin(id="lights", name="x", version="1.0.0")
        with self.assertRaisesRegex(PluginError, "must start with 'lights.'"):
            plugin.variable("obs.scene", "text")
        with self.assertRaisesRegex(PluginError, "one of its choices"):
            plugin.action("pick", "Pick", {"mode": arg("choice", "z", choices=["a", "b"])})
        with self.assertRaisesRegex(PluginError, "not a permission"):
            Plugin(id="lights", name="x", version="1.0.0", permissions=["everything"])

    def test_the_manifest_is_the_contracts(self):
        manifest = lights().manifest()
        self.assertEqual(manifest["abi"], 4)
        self.assertEqual(
            manifest["actions"][0],
            {"action": "toggle", "description": "Toggle", "arguments": [{"name": "room", "description": "", "example": "desk", "kind": "text", "choices": [], "optional": False}]},
        )
        self.assertEqual(manifest["uses"], ["series"])

    def test_a_sign_in_help_and_guide_reach_only_a_host_that_offers_setup(self):
        plugin = Plugin(id="signing", name="Signing", version="1.0.0", guide="https://example.com/signing#setup")
        plugin.setting("client_id", "text", "Client ID", help="From the developer portal")
        plugin.setting("token", "text", secret=True, internal=True)

        newer = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series", "setup"]})["manifest"]
        self.assertEqual(newer["guide"], "https://example.com/signing#setup")
        self.assertEqual([(s.get("internal"), s.get("help")) for s in newer["settings"]], [(None, "From the developer portal"), (True, None)])
        self.assertIn("setup", newer["uses"])
        older = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series"]})["manifest"]
        self.assertNotIn("guide", older)
        self.assertTrue(all("internal" not in s and "help" not in s for s in older["settings"]))
        self.assertNotIn("setup", older["uses"])

        with self.assertRaisesRegex(PluginError, "not an http"):
            Plugin(id="bad", name="Bad", version="1", guide="javascript:alert(1)")

    def test_a_condition_light_reaches_only_a_host_that_offers_conditions(self):
        plugin = Plugin(id="studio", name="Studio", version="1.0.0")
        when = "obs.studio_mode and obs.preview_scene != obs.scene"
        plugin.action("transition", "Transition", active={"when": when})(lambda args, ctx: None)
        newer = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series", "conditions"]})["manifest"]
        self.assertEqual(newer["actions"][0]["active"], {"when": when})
        self.assertEqual(newer["uses"], ["series", "conditions"])
        older = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series", "toggles"]})["manifest"]
        self.assertNotIn("active", older["actions"][0])
        self.assertEqual(older["uses"], ["series"])

    def test_a_condition_light_names_its_own_arguments_outside_quotes(self):
        plugin = Plugin(id="bad", name="Bad", version="1")
        args = {"a": arg("text", "x")}
        plugin.action("ok", "", args, active={"when": "`obs.input.{{a}}.muted` or obs.scene == {{a}}"})
        with self.assertRaisesRegex(PluginError, "not one of its arguments"):
            plugin.action("t1", "", args, active={"when": "obs.scene == {{b}}"})
        with self.assertRaisesRegex(PluginError, "inside quotes"):
            plugin.action("t2", "", args, active={"when": 'obs.scene == "{{a}}"'})
        with self.assertRaisesRegex(PluginError, "when alone"):
            plugin.action("t3", "", args, active={"when": "obs.on", "variable": "bad.on"})
        with self.assertRaisesRegex(PluginError, "when must be"):
            plugin.action("t4", "", args, active={"when": " "})

    def test_a_toggle_light_reaches_only_a_host_that_offers_toggles(self):
        plugin = Plugin(id="lights", name="Lights", version="1.0.0")
        plugin.variable("lights.room.desk.on", "bool")
        plugin.action("toggle", "Toggle", {"room": arg("text", "desk")}, active={"variable": "lights.room.{{room}}.on"})(lambda args, ctx: None)
        plugin.action("pick", "Pick", {"scene": arg("text", "a")}, active={"variable": "lights.room.desk.on", "equals": "scene"})(lambda args, ctx: None)

        newer = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series", "toggles"]})["manifest"]
        self.assertEqual([a.get("active") for a in newer["actions"]], [{"variable": "lights.room.{{room}}.on"}, {"variable": "lights.room.desk.on", "equals": "scene"}])
        self.assertIn("toggles", newer["uses"])
        older = plugin.handle("initialize", {"abi": 4, "hostCapabilities": ["series"]})["manifest"]
        self.assertEqual([a.get("active") for a in older["actions"]], [None, {"variable": "lights.room.desk.on", "equals": "scene"}])
        self.assertNotIn("toggles", older["uses"])

        with self.assertRaisesRegex(PluginError, "'place' is not one of its arguments"):
            plugin.action("bad", "Bad", {}, active={"variable": "lights.{{place}}.on"})
        with self.assertRaisesRegex(PluginError, "must start with 'lights.'"):
            plugin.action("bad", "Bad", {}, active={"variable": "obs.on"})


class ProtocolTest(unittest.TestCase):
    def test_actions_values_events_and_failures_as_the_host_sees_them(self):
        host = Harness(lights())
        self.assertEqual(host.invoke("toggle", {"room": "desk"}), {"ok": True, "state": "on"})
        self.assertIs(host.value("lights.on"), True)
        self.assertEqual(host.sent("event/emit"), [{"event": "toggled", "payload": {"on": "true"}}])
        self.assertEqual(host.invoke("fail"), {"ok": False, "error": "the bridge is unreachable"})
        self.assertEqual(host.sample(["lights.on", "obs.scene"]), {"lights.on": {"kind": "bool", "value": True}})

    def test_it_never_publishes_or_announces_what_it_did_not_declare(self):
        plugin = lights()
        host = Harness(plugin)
        plugin.ctx.vars.set({"lights.nope": 1})
        plugin.ctx.emit("exploded")
        self.assertEqual(host.sent("vars/set"), [])
        self.assertEqual(host.sent("event/emit"), [])
        self.assertEqual(len(host.sent("log")), 2)

    def test_an_abi_it_does_not_speak_is_refused(self):
        with self.assertRaisesRegex(PluginError, "speaks ABI 4"):
            lights().handle("initialize", {"abi": 3})

    def test_over_a_real_pipe(self):
        with tempfile.TemporaryDirectory() as folder:
            target = scaffold(Path(folder) / "hello", "hello", "Hello")
            env = {**os.environ, "PYTHONPATH": str(HERE)}
            process = subprocess.Popen([sys.executable, "-u", str(target / "plugin.py")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", env=env)

            def send(message):
                process.stdin.write(json.dumps(message) + "\n")
                process.stdin.flush()

            send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"abi": 4, "settings": {}}})
            first = json.loads(process.stdout.readline())
            self.assertEqual(first["result"]["manifest"]["id"], "hello")
            send({"jsonrpc": "2.0", "id": 2, "method": "invoke", "params": {"action": "greet", "args": {"name": "ada"}}})
            replies = [json.loads(process.stdout.readline()) for _ in range(4)]
            self.assertIn({"jsonrpc": "2.0", "id": 2, "result": {"ok": True}}, replies)
            # Not ASCII, both ways: the protocol is UTF-8 whatever the code page.
            send({"jsonrpc": "2.0", "id": 4, "method": "feed/watch", "params": {"instance": "w", "feed": "shout", "args": {"word": "grüße"}}})
            lines = [json.loads(process.stdout.readline()) for _ in range(3)]
            published = [m for m in lines if m.get("method") == "vars/set"]
            self.assertEqual(published[0]["params"]["values"]["hello.@w.word"]["value"], "GRÜSSE!")
            send({"jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": {}})
            self.assertEqual(process.wait(timeout=5), 0)
            process.stdin.close()
            process.stdout.close()


class TemplateTest(unittest.TestCase):
    def test_a_new_plugin_passes_its_own_tests_and_packs_with_the_sdk_inside(self):
        with tempfile.TemporaryDirectory() as folder:
            target = scaffold(Path(folder) / "desk-lights", "desk-lights", "Desk lights")
            plugin = check(target)
            self.assertEqual(check_definition(plugin), [])
            env = {**os.environ, "PYTHONPATH": str(HERE)}
            run = subprocess.run([sys.executable, "-m", "unittest", "-q"], cwd=target, env=env, capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stderr)
            packed = pack(target)
            names = zipfile.ZipFile(packed.file).namelist()
            self.assertIn("plugin.json", names)
            self.assertIn("plugin.py", names)
            self.assertIn("flexdeck_plugin/__init__.py", names)
            self.assertFalse(any(n.startswith("tests/") for n in names), "tests stay home")


FLEXDECK = find_flexdeck(HERE)


@unittest.skipUnless(FLEXDECK, "build the agent (cargo build -p flexdeck-agent) to run FlexDeck's own verification")
class VerifyTest(unittest.TestCase):
    def test_a_new_plugin_passes_flexdecks_own_verification(self):
        with tempfile.TemporaryDirectory() as folder:
            target = scaffold(Path(folder) / "hello", "hello", "Hello")
            report = conform(pack(target).file, FLEXDECK)
            self.assertTrue(passed(report), describe(report))
            names = [c["name"] for c in report["checks"]]
            self.assertIn("its feeds start and stop for a key", names)
            self.assertIn("its gallery keys could be saved in a deck", names)

    def test_a_plugin_that_breaks_the_rules_is_named(self):
        with tempfile.TemporaryDirectory() as folder:
            target = scaffold(Path(folder) / "rude", "rude", "Rude")
            source = (target / "plugin.py").read_text(encoding="utf-8")
            # Writes to the real stdout (the protocol) when sampled, and tries
            # another plugin's value.
            source = source.replace(
                '    return {"rude.greetings"',
                '    sys.__stdout__.write("debug!\\n"); sys.__stdout__.flush()\n    ctx._plugin._notify("vars/set", {"values": {"obs.scene": {"kind": "text", "value": "x"}}})\n    return {"rude.greetings"',
            )
            (target / "plugin.py").write_text("import sys\n" + source, encoding="utf-8")
            report = conform(pack(target).file, FLEXDECK)
            text = describe(report)
            self.assertFalse(passed(report), text)
            self.assertIn("debug!", text)
            self.assertIn("obs.scene", text)


if __name__ == "__main__":
    unittest.main()
