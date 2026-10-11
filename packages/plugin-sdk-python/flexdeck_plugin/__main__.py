"""python -m flexdeck_plugin <new|check|pack|verify> [dir]   (also: flexdeck-plugin)"""

from __future__ import annotations

import sys
from pathlib import Path

from . import PluginError, is_valid_id
from .conformance import conform, describe, find_flexdeck, passed
from .pack import check, pack
from .template import scaffold

USAGE = """usage: flexdeck-plugin <command> [dir]

  new <id> ["<Name>"]   a new plugin in ./<id>, ready to test and pack
  check [dir]           plugin.json and the code agree
  pack  [dir]           write <id>-<version>.fdpkg, ready to install
  verify [dir] [--run-actions]
                        pack, then ask FlexDeck itself whether it will work:
                        run as the app runs it, checked at every step"""


def main(argv=None) -> int:
    # A console on an old code page shows what it can rather than crashing.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="replace")
    args = list(sys.argv[1:] if argv is None else argv)
    command = args[0] if args else "help"
    rest = [a for a in args[1:] if not a.startswith("--")]
    directory = Path(rest[0]) if rest else Path(".")
    try:
        if command == "new":
            if not rest or not is_valid_id(rest[0]):
                print('usage: flexdeck-plugin new <id> ["<Name>"]   (id: lowercase letters, digits, - and _)', file=sys.stderr)
                return 1
            target = scaffold(Path(rest[0]), rest[0], rest[1] if len(rest) > 1 else rest[0])
            print(f"created {target}\nnext:\n  cd {rest[0]}\n  python -m unittest\n  python -m flexdeck_plugin verify")
            return 0
        if command == "check":
            plugin = check(directory)
            print(f"{plugin.id} {plugin.version}: plugin.json and the code agree")
            return 0
        if command == "pack":
            packed = pack(directory)
            print(f"packed {packed.file}\n  {packed.size} bytes\n  sha256 {packed.sha256}")
            return 0
        if command == "verify":
            flexdeck = find_flexdeck(directory)
            if not flexdeck:
                print("verify asks FlexDeck itself, and none was found. Install the FlexDeck app, or set FLEXDECK_BIN to its executable.", file=sys.stderr)
                return 1
            packed = pack(directory)
            report = conform(packed.file, flexdeck, run_actions="--run-actions" in args)
            print(describe(report))
            return 0 if passed(report) else 1
    except (PluginError, RuntimeError) as error:
        print(f"flexdeck-plugin: {error}", file=sys.stderr)
        return 1
    print(USAGE)
    return 0 if command in ("help", "--help", "-h") else 1


if __name__ == "__main__":
    sys.exit(main())
