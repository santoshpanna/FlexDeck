"""Write FlexDeck plugins in Python.

A plugin is one :class:`Plugin` with actions, values, events and feeds
declared on it; the SDK speaks the protocol (JSON-RPC over stdin/stdout, the
same contract as the TypeScript SDK and ``crates/flexdeck-plugin-abi``)::

    from flexdeck_plugin import Plugin, arg

    plugin = Plugin(id="lights", name="Desk lights", version="0.1.0", permissions=["network"])
    plugin.variable("lights.on", "bool", "Whether the desk lights are on")

    @plugin.action("toggle", "Turn the desk lights on or off")
    def toggle(args, ctx):
        ctx.vars.set({"lights.on": not ctx.vars.get("lights.on")})

    if __name__ == "__main__":
        plugin.serve()

Standard library only, so a packed plugin carries the SDK with it and runs
on any Python 3.10+ with nothing installed.
"""

from __future__ import annotations

import base64
import hashlib
import io
import json
import math
import os
import re
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Iterable, List, Mapping, Optional, Union

__all__ = [
    "ABI_VERSION",
    "Argument",
    "Context",
    "Plugin",
    "PluginError",
    "arg",
    "is_valid_id",
]

#: The contract version this SDK speaks. A Python plugin needs ABI 4.
ABI_VERSION = 4
SUPPORTED_ABIS = (4,)
KINDS = ("text", "number", "bool", "choice")
PERMISSIONS = ("network", "notifications", "processes", "filesystem")
MAX_SERIES_POINTS = 600
MAX_IMAGE_BYTES = 256 * 1024
IMAGE_MIMES = ("image/png", "image/jpeg", "image/webp")
CAPABILITIES = ("series", "presets", "feeds", "toggles", "setup", "conditions")
MAX_CONDITION_LEN = 2000
_PLACEHOLDER = re.compile(r"\{\{\s*([^{}\s]+)\s*\}\}")


def _placeholder_in_quotes(when: str) -> bool:
    quote = None
    i = 0
    while i < len(when):
        c = when[i]
        if quote == "`":
            if c == "`":
                quote = None
        elif quote:
            if c == "\\":
                i += 1
            elif c == quote:
                quote = None
            elif c == "{" and when[i + 1 : i + 2] == "{":
                return True
        elif c in "\"'`":
            quote = c
        i += 1
    return False

Value = Union[str, float, int, bool]

_ID = re.compile(r"^[a-z][a-z0-9_-]{0,63}$")


def is_valid_id(value: str) -> bool:
    """Ids become variable namespaces, folder names and URL segments."""
    return bool(_ID.match(value))


class PluginError(Exception):
    """A plugin defined in a way FlexDeck would refuse, said when it is defined."""


def _require(condition: Any, message: str) -> None:
    if not condition:
        raise PluginError(f"flexdeck plugin: {message}")


@dataclass
class Argument:
    """One argument of an action or feed. ``example`` must work: the editor
    fills it in, and verification runs every action with it."""

    kind: str
    example: str
    description: str = ""
    choices: List[str] = field(default_factory=list)
    optional: bool = False
    #: Suggestions for the editor, asked for while someone fills the argument in:
    #: ``suggest(args, ctx) -> [value | (value, label)]``.
    suggest: Optional[Callable[[Dict[str, str], "Context"], Iterable[Any]]] = None

    def dto(self, name: str) -> Dict[str, Any]:
        return {
            "name": name,
            "description": self.description,
            "example": self.example,
            "kind": self.kind,
            "choices": list(self.choices),
            "optional": self.optional,
        }


def arg(kind: str, example: str, description: str = "", *, choices: Iterable[str] = (), optional: bool = False, suggest=None) -> Argument:
    """An argument: ``arg("text", "desk", "Which room")``."""
    return Argument(kind=kind, example=str(example), description=description, choices=list(choices), optional=optional, suggest=suggest)


@dataclass
class _Action:
    description: str
    run: Callable[[Dict[str, str], "Context"], Any]
    arguments: Dict[str, Argument]
    check: Optional[Callable[[Dict[str, str], "Context"], Optional[str]]] = None
    active: Optional[Dict[str, str]] = None

    def needs(self) -> Optional[str]:
        """The capability a host must offer to read this light, if any."""
        if self.active is None:
            return None
        if "when" in self.active:
            return "conditions"
        plain = "equals" in self.active and not _PLACEHOLDER.search(self.active["variable"])
        return None if plain else "toggles"

    def active_dto(self, offered: Iterable[str]) -> Optional[Dict[str, str]]:
        """The light as the host reads it: the plain kind (``equals``, no
        placeholders) for any host, the rest only for one offering ``toggles``,
        and a ``when`` only for one offering ``conditions``."""
        if self.active is None:
            return None
        needs = self.needs()
        return dict(self.active) if needs is None or needs in offered else None


@dataclass
class _Feed:
    description: str
    fields: Dict[str, Dict[str, str]]
    arguments: Dict[str, Argument]
    watch: Callable[[str, Dict[str, str], "Context"], Any]
    unwatch: Optional[Callable[[str, "Context"], Any]] = None


def _wrap(kind: str, value: Value) -> Dict[str, Any]:
    if kind == "bool":
        return {"kind": kind, "value": bool(value)}
    if kind == "number":
        return {"kind": kind, "value": float(value)}
    return {"kind": kind, "value": str(value)}


class _Vars:
    def __init__(self, plugin: "Plugin") -> None:
        self._plugin = plugin

    def set(self, values: Mapping[str, Value]) -> None:
        """Publish values for variables this plugin owns."""
        wire = self._plugin._publish(values)
        if wire:
            self._plugin._notify("vars/set", {"values": wire})

    def declare(self, variables: Mapping[str, Mapping[str, Any]], replace: bool = False) -> None:
        """Declare variables at run time, one per scene, say:
        ``{"obs.scene.live": {"kind": "bool", "description": "…"}}``."""
        self._plugin._declare(variables, replace)

    def get(self, name: str) -> Optional[Value]:
        """The last value this plugin published."""
        return self._plugin._published.get(name)


class _Log:
    def __init__(self, plugin: "Plugin") -> None:
        self._plugin = plugin

    def info(self, message: str) -> None:
        self._plugin._notify("log", {"level": "info", "message": str(message)})

    def warn(self, message: str) -> None:
        self._plugin._notify("log", {"level": "warn", "message": str(message)})

    def error(self, message: str) -> None:
        self._plugin._notify("log", {"level": "error", "message": str(message)})


class _Feed_:
    def __init__(self, plugin: "Plugin", instance: str) -> None:
        self._plugin = plugin
        self._instance = instance

    def set(self, values: Mapping[str, Value]) -> None:
        """Publish one key's values: ``{"temp": 21}`` becomes ``<id>.@<instance>.temp``."""
        plugin = self._plugin
        feed = plugin._watching.get(self._instance)
        _require(feed is not None, f"nothing is watching a feed as '{self._instance}'")
        fields = plugin._feeds[feed].fields
        fresh: Dict[str, Dict[str, str]] = {}
        full: Dict[str, Value] = {}
        for name, value in values.items():
            spec = fields.get(name)
            if spec is None:
                plugin.ctx.log.error(f"'{name}' is not a field of the {feed} feed")
                continue
            full_name = f"{plugin.id}.@{self._instance}.{name}"
            if full_name not in plugin._declared:
                fresh[full_name] = spec
            full[full_name] = value
        if fresh:
            plugin._declare(fresh, False)
        plugin.ctx.vars.set(full)


class Context:
    """What an action, a sample and a feed get: settings, values, events, logs."""

    def __init__(self, plugin: "Plugin") -> None:
        self._plugin = plugin
        self.id = plugin.id
        self.vars = _Vars(plugin)
        self.log = _Log(plugin)

    @property
    def settings(self) -> Dict[str, str]:
        """This plugin's settings, defaults filled in. Never another plugin's."""
        return dict(self._plugin._settings)

    @property
    def data_dir(self) -> Optional[str]:
        """A folder of this plugin's own, kept across updates."""
        return self._plugin._data_dir

    def emit(self, event: str, payload: Optional[Mapping[str, str]] = None) -> None:
        """Announce an event this plugin declares."""
        if event not in self._plugin._events:
            self.log.error(f"'{event}' is not a declared event")
            return
        self._plugin._notify("event/emit", {"event": event, "payload": {k: str(v) for k, v in (payload or {}).items()}})

    def notify(self, title: str, body: str = "") -> None:
        """Show a notification. Needs the ``notifications`` permission."""
        self._plugin._notify("notify", {"title": title, "body": body})

    def store_settings(self, values: Mapping[str, str]) -> None:
        """Keep values for settings this plugin declares (a refreshed token)."""
        for key in values:
            _require(key in self._plugin._settings_spec, f"setting '{key}' is not declared, so it cannot be stored")
        self._plugin._settings.update(values)
        self._plugin._notify("settings/store", {"values": dict(values)})

    def open_url(self, url: str) -> None:
        """Open an http(s) address in the person's browser. Needs ``network``."""
        _require(re.match(r"^https?://\S+$", url, re.I), f"'{url}' is not an http(s) address")
        self._plugin._notify("host/open_url", {"url": url})

    def put_image(self, data: bytes, mime: str) -> str:
        """Hand over a picture (≤256 KiB); returns ``img:<sha256>`` for a value to hold."""
        _require(mime in IMAGE_MIMES, f"'{mime}' is not an image type the agent shows")
        _require(len(data) <= MAX_IMAGE_BYTES, f"the image is {len(data)} bytes; the limit is {MAX_IMAGE_BYTES}")
        sha = hashlib.sha256(data).hexdigest()
        self._plugin._notify("images/put", {"sha256": sha, "mime": mime, "data": base64.b64encode(data).decode("ascii")})
        return f"img:{sha}"

    def set_series(self, name: str, points: Iterable[float]) -> None:
        """A value's recent history, oldest first, for a graph. At most 600 points."""
        _require(name.startswith(f"{self.id}."), f"'{name}' must start with '{self.id}.'")
        finite = [float(p) for p in points if isinstance(p, (int, float)) and math.isfinite(p)][-MAX_SERIES_POINTS:]
        self._plugin._notify("series/set", {"name": name, "points": finite})

    def feed(self, instance: str) -> _Feed_:
        """Publish values for one key's run of a feed."""
        return _Feed_(self._plugin, instance)


class Plugin:
    """A FlexDeck plugin. Declare on it, then :meth:`serve`."""

    def __init__(
        self,
        id: str,
        name: str,
        version: str,
        description: str = "",
        author: str = "",
        permissions: Iterable[str] = (),
        guide: Optional[str] = None,
    ) -> None:
        _require(is_valid_id(id), f"'{id}' is not a usable id (lowercase letters, digits, - and _)")
        _require(guide is None or re.match(r"^https?://", guide) is not None, f"guide '{guide}' is not an http(s) address")
        _require(re.match(r"^\d[0-9A-Za-z.+-]*$", version) and ".." not in version, f"'{version}' is not a version")
        for permission in permissions:
            _require(permission in PERMISSIONS, f"'{permission}' is not a permission: {', '.join(PERMISSIONS)}")
        self.id = id
        self.name = name
        self.version = version
        self.description = description
        self.author = author
        self.permissions = list(permissions)
        self.guide = guide
        self._actions: Dict[str, _Action] = {}
        self._settings_spec: Dict[str, Dict[str, Any]] = {}
        self._events: Dict[str, Dict[str, Any]] = {}
        self._feeds: Dict[str, _Feed] = {}
        self._presets: List[Dict[str, Any]] = []
        self._declared: Dict[str, Dict[str, Any]] = {}
        self._published: Dict[str, Value] = {}
        self._watching: Dict[str, str] = {}
        self._settings: Dict[str, str] = {}
        self._data_dir: Optional[str] = None
        self._sample: Optional[Callable[[List[str], Context], Optional[Mapping[str, Value]]]] = None
        self._start: Optional[Callable[[Context], Any]] = None
        self._stop: Optional[Callable[[Context], Any]] = None
        self._settings_changed: Optional[Callable[[Context], Any]] = None
        self._lock = threading.RLock()
        self._out_lock = threading.Lock()
        #: What the plugin sent the host unprompted. For tests.
        self.sent: List[Dict[str, Any]] = []
        self._write: Callable[[Dict[str, Any]], None] = self.sent.append
        self.ctx = Context(self)

    # ---- Declaring ---------------------------------------------------------------

    def variable(self, name: str, kind: str, description: str = "", *, unit: Optional[str] = None, choices: Iterable[str] = (), read_only: bool = True) -> None:
        """A live value a key can show and an automation react to, under ``<id>.``."""
        prefix = f"{self.id}."
        _require(name.startswith(prefix) and len(name) > len(prefix), f"variable '{name}' must start with '{prefix}'")
        _require(kind in KINDS, f"variable '{name}': '{kind}' is not a kind: {', '.join(KINDS)}")
        self._declared[name] = {"kind": kind, "description": description, "unit": unit, "choices": list(choices), "readOnly": read_only}

    def setting(self, key: str, kind: str = "text", description: str = "", *, default: Optional[str] = None, choices: Iterable[str] = (), secret: bool = False, internal: bool = False, help: Optional[str] = None) -> None:
        """Configuration filled in once. A ``secret`` is never shown again; an
        ``internal`` one is kept by the plugin (a sign-in), hidden from the form
        and cleared by signing out; ``help`` is shown behind an info button."""
        _require(kind in KINDS, f"setting '{key}': '{kind}' is not a kind")
        self._settings_spec[key] = {"kind": kind, "description": description, "default": default, "choices": list(choices), "secret": secret, "internal": internal, "help": help}

    def event(self, name: str, description: str = "", fields: Iterable[str] = ()) -> None:
        """Something that happens, which an automation can be started by."""
        self._events[name] = {"description": description, "fields": list(fields)}

    def action(self, name: str, description: str, arguments: Optional[Mapping[str, Argument]] = None, *, check=None, active: Optional[Mapping[str, str]] = None):
        """Decorate a function ``run(args, ctx)`` as an action. It may return
        ``{"state": "<look>"}`` for the key to show.

        ``active`` says when a key running it is lit: ``{"variable": "x.muted"}``
        while that is true, ``{"variable": "x.mode", "is": "ptt"}`` while it holds
        that value, ``{"variable": "x.scene", "equals": "scene"}`` while it holds
        the ``scene`` argument's value. The variable may hold ``{{argument}}``
        placeholders. ``{"when": "obs.studio_mode and obs.preview_scene !=
        obs.scene"}`` while that condition holds; its placeholders may be part
        of a name or a whole value, never inside quotes."""
        arguments = dict(arguments or {})
        for arg_name, argument in arguments.items():
            _require(argument.kind in KINDS, f"{name}.{arg_name}: '{argument.kind}' is not a kind")
            if argument.kind == "choice":
                _require(argument.example in argument.choices, f"{name}.{arg_name}: the example must be one of its choices")
        if active is not None and "when" in active:
            active = dict(active)
            when = active["when"]
            _require(set(active) == {"when"}, f"{name}.active: takes when alone")
            _require(isinstance(when, str) and when.strip() != "" and len(when) <= MAX_CONDITION_LEN, f"{name}.active: when must be 1 to {MAX_CONDITION_LEN} characters")
            _require(not _placeholder_in_quotes(when), f"{name}.active: a placeholder inside quotes in when; put it outside, as a whole value")
            for named in _PLACEHOLDER.findall(when):
                _require(named in arguments, f"{name}.active: '{named}' is not one of its arguments")
        elif active is not None:
            active = dict(active)
            variable = active.get("variable", "")
            _require(set(active) <= {"variable", "equals", "is"}, f"{name}.active: takes variable, equals and is")
            _require(not ("equals" in active and "is" in active), f"{name}.active: takes equals or is, not both")
            _require(variable.startswith(f"{self.id}."), f"{name}.active: variable '{variable}' must start with '{self.id}.'")
            for named in _PLACEHOLDER.findall(variable) + ([active["equals"]] if "equals" in active else []):
                _require(named in arguments, f"{name}.active: '{named}' is not one of its arguments")

        def register(run):
            self._actions[name] = _Action(description=description, run=run, arguments=arguments, check=check, active=active)
            return run

        return register

    def feed(self, name: str, description: str = "", *, fields: Mapping[str, Union[str, Mapping[str, str]]], arguments: Optional[Mapping[str, Argument]] = None, unwatch=None):
        """Decorate ``watch(instance, args, ctx)``: values for one key, with its
        own arguments (a city), published with ``ctx.feed(instance).set``."""
        specs = {f: ({"kind": s, "description": ""} if isinstance(s, str) else {"kind": s["kind"], "description": s.get("description", "")}) for f, s in fields.items()}

        def register(watch):
            self._feeds[name] = _Feed(description=description, fields=specs, arguments=dict(arguments or {}), watch=watch, unwatch=unwatch)
            return watch

        return register

    def preset(self, id: str, name: str, widget: Mapping[str, Any], *, category: str = "", description: str = "", w: int = 1, h: int = 1) -> None:
        """A ready-made key for the gallery: a widget without id or place."""
        self._presets.append({"id": id, "name": name, "category": category, "description": description, "w": w, "h": h, "widget": dict(widget)})

    def sample(self, function):
        """Decorate ``sample(wanted, ctx) -> {name: value}``: asked only for what a key shows."""
        self._sample = function
        return function

    def on_start(self, function):
        self._start = function
        return function

    def on_stop(self, function):
        self._stop = function
        return function

    def on_settings_changed(self, function):
        self._settings_changed = function
        return function

    # ---- What the host is told -------------------------------------------------

    def manifest(self, offered: Iterable[str] = CAPABILITIES) -> Dict[str, Any]:
        """What ``initialize`` answers with, to a host offering ``offered``."""
        offered = tuple(offered)
        actions = []
        for n, a in self._actions.items():
            dto: Dict[str, Any] = {"action": n, "description": a.description, "arguments": [arg.dto(k) for k, arg in a.arguments.items()]}
            light = a.active_dto(offered)
            if light is not None:
                dto["active"] = light
            elif a.active is not None:
                print(f"plugin: this agent has no {a.needs()}, so {n} is never lit", file=sys.stderr)
            actions.append(dto)
        manifest: Dict[str, Any] = {
            "abi": ABI_VERSION,
            "id": self.id,
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "author": self.author,
            "actions": actions,
            "settings": [
                {"key": k, "description": s["description"], "kind": s["kind"], "choices": s["choices"], "defaultValue": s["default"], "secret": s["secret"]}
                for k, s in self._settings_spec.items()
            ],
            "events": [{"name": n, "description": e["description"], "fields": e["fields"]} for n, e in self._events.items()],
            "permissions": list(self.permissions),
        }
        uses = ["series"]
        if self._presets:
            uses.append("presets")
            manifest["presets"] = list(self._presets)
        if self._feeds:
            uses.append("feeds")
            manifest["feeds"] = [
                {
                    "name": n,
                    "description": f.description,
                    "arguments": [arg.dto(k) for k, arg in f.arguments.items()],
                    "fields": [{"name": k, "kind": s["kind"], "description": s["description"]} for k, s in f.fields.items()],
                }
                for n, f in self._feeds.items()
            ]
        for capability in ("toggles", "conditions"):
            if capability in offered and any(a.needs() == capability for a in self._actions.values()):
                uses.append(capability)
        if "setup" in offered:
            for dto, spec in zip(manifest["settings"], self._settings_spec.values()):
                if spec["internal"]:
                    dto["internal"] = True
                if spec["help"]:
                    dto["help"] = spec["help"]
            if self.guide:
                manifest["guide"] = self.guide
            if self.guide or any("internal" in dto or "help" in dto for dto in manifest["settings"]):
                uses.append("setup")
        manifest["uses"] = uses
        return manifest

    def variables(self) -> List[Dict[str, Any]]:
        with self._lock:
            return [{"name": n, "description": s.get("description", ""), "kind": s["kind"], "unit": s.get("unit"), "choices": s.get("choices", []), "readOnly": s.get("readOnly", True)} for n, s in self._declared.items()]

    def _notify(self, method: str, params: Dict[str, Any]) -> None:
        self._write({"jsonrpc": "2.0", "method": method, "params": params})

    def _publish(self, values: Mapping[str, Value]) -> Dict[str, Any]:
        wire: Dict[str, Any] = {}
        with self._lock:
            for name, value in values.items():
                spec = self._declared.get(name)
                if spec is None:
                    self.ctx.log.error(f"'{name}' was never declared, so it was not published")
                    continue
                if self._published.get(name, object()) == value and type(self._published.get(name)) is type(value):
                    continue
                self._published[name] = value
                wire[name] = _wrap(spec["kind"], value)
        return wire

    def _declare(self, variables: Mapping[str, Mapping[str, Any]], replace: bool) -> None:
        prefix = f"{self.id}."
        for name, spec in variables.items():
            _require(name.startswith(prefix), f"variable '{name}' must start with '{prefix}'")
            _require(spec.get("kind") in KINDS, f"variable '{name}': '{spec.get('kind')}' is not a kind")
        with self._lock:
            if replace:
                for name in list(self._declared):
                    if name not in variables:
                        self._declared.pop(name, None)
                        self._published.pop(name, None)
            for name, spec in variables.items():
                self._declared[name] = {"kind": spec["kind"], "description": spec.get("description", ""), "unit": spec.get("unit"), "choices": list(spec.get("choices", [])), "readOnly": spec.get("readOnly", True)}
        self._notify("vars/declare", {"variables": self.variables(), "replace": replace})

    def configure(self, settings: Mapping[str, str], data_dir: Optional[str] = None) -> None:
        """Settings as ``initialize`` gives them, defaults filled in. For tests."""
        defaults = {k: s["default"] for k, s in self._settings_spec.items() if s["default"] is not None}
        self._settings = {**defaults, **dict(settings)}
        self._data_dir = data_dir

    # ---- Answering the host ----------------------------------------------------

    def handle(self, method: str, params: Optional[Mapping[str, Any]] = None) -> Any:
        """Answer one request as the host sends it. Raises for an error answer."""
        body = dict(params or {})
        ctx = self.ctx
        if method == "initialize":
            asked = body.get("abi")
            if asked not in SUPPORTED_ABIS:
                raise PluginError(f"this plugin speaks ABI {' or '.join(map(str, SUPPORTED_ABIS))}, the agent speaks {asked}")
            self.configure(body.get("settings") or {}, body.get("dataDir"))
            return {"manifest": self.manifest(body.get("hostCapabilities") or ()), "variables": self.variables()}
        if method == "variables":
            return self.variables()
        if method == "check":
            action = self._actions.get(body.get("action", ""))
            if action is None:
                return {"ok": False, "error": f"no action called '{body.get('action')}'"}
            problem = action.check(dict(body.get("args") or {}), ctx) if action.check else None
            return {"ok": False, "error": str(problem)} if problem else {"ok": True}
        if method == "invoke":
            action = self._actions.get(body.get("action", ""))
            if action is None:
                return {"ok": False, "error": f"no action called '{body.get('action')}'"}
            try:
                result = action.run(dict(body.get("args") or {}), ctx)
            except Exception as error:  # an action's failure is an answer, not a crash
                return {"ok": False, "error": str(error) or type(error).__name__}
            answer: Dict[str, Any] = {"ok": True}
            if isinstance(result, Mapping) and isinstance(result.get("state"), str):
                answer["state"] = result["state"]
            return answer
        if method == "sample":
            with self._lock:
                wanted = [n for n in body.get("wanted") or [] if n in self._declared]
            if not wanted:
                return {"values": {}}
            returned = self._sample(wanted, ctx) if self._sample else None
            self._publish(returned or {})
            values = {}
            with self._lock:
                for name in wanted:
                    if name in self._published:
                        values[name] = _wrap(self._declared[name]["kind"], self._published[name])
            return {"values": values}
        if method == "choices":
            action = self._actions.get(body.get("action", ""))
            answer = {"arguments": {}}
            if action is None:
                return {**answer, "error": f"no action called '{body.get('action')}'"}
            args = dict(body.get("args") or {})
            for name, argument in action.arguments.items():
                if not argument.suggest:
                    continue
                try:
                    suggestions = argument.suggest(args, ctx) or []
                except Exception as error:
                    answer.setdefault("error", str(error))
                    continue
                answer["arguments"][name] = [
                    {"value": str(s[0]), "label": str(s[1])} if isinstance(s, (tuple, list)) else {"value": str(s)} for s in suggestions
                ]
            return answer
        if method == "feed/watch":
            feed = self._feeds.get(body.get("feed", ""))
            if feed is None:
                raise PluginError(f"no feed called '{body.get('feed')}'")
            instance = str(body.get("instance", ""))
            self._watching[instance] = body["feed"]
            given = dict(body.get("args") or {})
            filled = {n: (given[n] if str(given.get(n, "")).strip() else a.example) for n, a in feed.arguments.items()}
            feed.watch(instance, filled, ctx)
            return {}
        if method == "feed/unwatch":
            instance = str(body.get("instance", ""))
            feed = self._watching.pop(instance, None)
            prefix = f"{self.id}.@{instance}."
            with self._lock:
                for name in [n for n in self._declared if n.startswith(prefix)]:
                    self._declared.pop(name, None)
                    self._published.pop(name, None)
            if feed and self._feeds[feed].unwatch:
                self._feeds[feed].unwatch(instance, ctx)
            return {}
        if method == "settings/changed":
            self.configure(body.get("settings") or {}, self._data_dir)
            if self._settings_changed:
                self._settings_changed(ctx)
            return {}
        if method == "shutdown":
            if self._stop:
                self._stop(ctx)
            return {}
        raise PluginError(f"no method '{method}'")

    def serve(self) -> None:
        """Speak the protocol on this process's stdin and stdout until the agent goes."""
        # The protocol is UTF-8 lines ending in a bare newline, whatever the
        # console's code page; stdout is only the protocol, so a print by habit
        # goes to the log (stderr) instead.
        out = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", newline="\n")
        incoming = io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8")
        sys.stdout = sys.stderr

        def write(message: Dict[str, Any]) -> None:
            line = json.dumps(message, separators=(",", ":"), ensure_ascii=False)
            with self._out_lock:
                out.write(line + "\n")
                out.flush()

        self._write = write
        pool = ThreadPoolExecutor(max_workers=4, thread_name_prefix="flexdeck")
        started = threading.Event()

        def answer(message: Dict[str, Any]) -> None:
            method = message["method"]
            try:
                result = self.handle(method, message.get("params"))
                write({"jsonrpc": "2.0", "id": message["id"], "result": result if result is not None else {}})
            except Exception as error:
                write({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32000, "message": str(error)}})
            if method == "initialize" and not started.is_set():
                started.set()
                if self._start:
                    # After the answer, so a slow connect never counts against the deadline.
                    def start() -> None:
                        try:
                            self._start(self.ctx)
                        except Exception as error:
                            self.ctx.log.error(f"start failed: {error}")

                    pool.submit(start)
            if method == "shutdown":
                os._exit(0)

        for line in incoming:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except ValueError:
                self.ctx.log.error(f"the agent sent something that is not JSON: {line[:200]}")
                continue
            if "id" not in message or not message.get("method"):
                continue
            if message["method"] in ("initialize", "shutdown"):
                answer(message)
            else:
                pool.submit(answer, message)
        # The agent closed stdin: it is gone, so follow it.
        if self._stop:
            try:
                self._stop(self.ctx)
            except Exception:
                pass
        os._exit(0)
