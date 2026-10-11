"""Package a plugin: ``<id>-<version>.fdpkg``, ready to install.

The package carries the plugin's own Python files and this SDK beside them, so
it runs on any Python 3.10+ with nothing installed. ``plugin.json`` and the
code must agree on who the plugin is (the agent refuses a plugin whose answer
differs from its package), so that is checked first.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional

from . import ABI_VERSION, Plugin, PluginError, is_valid_id

SDK_DIR = Path(__file__).resolve().parent
#: Never packed: tests, caches, packages, environments, and a copy of the SDK
#: (the one running this is packed instead).
SKIP_DIRS = {"__pycache__", "tests", "test", ".venv", "venv", "dist", "build", ".git", ".pytest_cache", "flexdeck_plugin"}
PACKED = (".py", ".json", ".txt", ".md", ".png", ".svg")


@dataclass
class Packed:
    file: Path
    size: int
    sha256: str


def read_package(directory: Path) -> dict:
    path = directory / "plugin.json"
    if not path.is_file():
        raise PluginError(f"no plugin.json in {directory}")
    package = json.loads(path.read_text(encoding="utf-8"))
    problems = []
    if package.get("runtime") != "python":
        problems.append('runtime must be "python"')
    if package.get("abi") != ABI_VERSION:
        problems.append(f"abi must be {ABI_VERSION}")
    if not is_valid_id(str(package.get("id", ""))):
        problems.append("id: lowercase letters, digits, - and _, starting with a letter")
    entry = package.get("entry")
    if not entry or not (directory / entry).is_file():
        problems.append(f"entry: '{entry}' is not a file in the plugin")
    if problems:
        raise PluginError("plugin.json: " + "; ".join(problems))
    return package


def load_plugin(directory: Path, entry: str) -> Plugin:
    """Import the entry file (without serving it) and find its Plugin."""
    sys.path.insert(0, str(directory))
    try:
        spec = importlib.util.spec_from_file_location("_flexdeck_entry", directory / entry)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
    finally:
        sys.path.remove(str(directory))
    found = [value for value in vars(module).values() if isinstance(value, Plugin)]
    if len(found) != 1:
        raise PluginError(f"{entry} should define exactly one Plugin; it defines {len(found)}")
    return found[0]


def check(directory: Path) -> Plugin:
    """plugin.json and the code agree, as the agent will require."""
    directory = Path(directory).resolve()
    package = read_package(directory)
    plugin = load_plugin(directory, package["entry"])
    for key, value in (("id", plugin.id), ("name", plugin.name), ("version", plugin.version)):
        if package.get(key) != value:
            raise PluginError(f"plugin.json says {key} '{package.get(key)}', the code says '{value}'")
    extra = sorted(set(plugin.permissions) - set(package.get("permissions", [])))
    if extra:
        raise PluginError(f"the code asks for {', '.join(extra)}, which plugin.json does not declare")
    return plugin


def _files(directory: Path) -> List[Path]:
    files = []
    for root, dirs, names in os.walk(directory):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS and not d.startswith("."))
        for name in sorted(names):
            if name.endswith(PACKED):
                files.append(Path(root) / name)
    return files


def pack(directory: Path, out: Optional[Path] = None) -> Packed:
    directory = Path(directory).resolve()
    check(directory)
    package = read_package(directory)
    target = Path(out) if out else directory / f"{package['id']}-{package['version']}.fdpkg"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in _files(directory):
            archive.write(path, path.relative_to(directory).as_posix())
        for path in sorted(SDK_DIR.rglob("*")):
            if path.is_file() and "__pycache__" not in path.parts and path.suffix in (".py", ".json"):
                archive.write(path, "flexdeck_plugin/" + path.relative_to(SDK_DIR).as_posix())
    data = target.read_bytes()
    return Packed(file=target, size=len(data), sha256=hashlib.sha256(data).hexdigest())
