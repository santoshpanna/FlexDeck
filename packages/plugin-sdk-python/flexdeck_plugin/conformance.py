"""Will this plugin work in FlexDeck? Asked of FlexDeck itself.

``flexdeck-plugin verify`` packs the plugin and hands it to the FlexDeck on
this computer (``flexdeck verify-plugin``), which runs it exactly as it would
after an install (the same host, deadlines and rules) and checks it at every
step. Nothing here re-implements those rules, so they cannot drift from the
app's; the TypeScript SDK asks the same way.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Mapping, Optional

EXE = "flexdeck.exe" if os.name == "nt" else "flexdeck"


def find_flexdeck(start: Optional[Path] = None, env: Optional[Mapping[str, str]] = None) -> Optional[str]:
    """FLEXDECK_BIN; a build in a FlexDeck checkout above ``start``; the PATH; where the app installs."""
    env = os.environ if env is None else env
    if env.get("FLEXDECK_BIN"):
        return env["FLEXDECK_BIN"] if Path(env["FLEXDECK_BIN"]).is_file() else None
    here = Path(start or Path.cwd()).resolve()
    for directory in [here, *here.parents]:
        for build in ("debug", "release"):
            candidate = directory / "target" / build / EXE
            if candidate.is_file():
                return str(candidate)
    on_path = shutil.which("flexdeck", path=env.get("PATH"))
    if on_path:
        return on_path
    if os.name == "nt":
        installed = [Path(env[k]) / "flexdeck" / EXE for k in ("LOCALAPPDATA", "ProgramFiles") if env.get(k)]
    elif sys.platform == "darwin":
        installed = [Path("/Applications/flexdeck.app/Contents/MacOS/flexdeck")]
    else:
        installed = [Path("/usr/bin/flexdeck"), Path("/usr/local/bin/flexdeck")]
    return next((str(p) for p in installed if p.is_file()), None)


def conform(package: Path, flexdeck: str, run_actions: bool = False) -> dict:
    """Ask ``flexdeck`` whether the package (a .fdpkg or a package folder) will work."""
    with tempfile.TemporaryDirectory(prefix="flexdeck-verify-") as scratch:
        out = Path(scratch) / "report.json"
        args = [flexdeck, "verify-plugin", str(Path(package).resolve()), "--out", str(out)]
        if run_actions:
            args.append("--run-actions")
        run = subprocess.run(args, capture_output=True, text=True, timeout=120)
        if not out.is_file():
            raise RuntimeError(f"{flexdeck} could not verify it: {(run.stderr or 'no report').strip()}")
        return json.loads(out.read_text(encoding="utf-8"))


def passed(report: dict) -> bool:
    return all(check["ok"] for check in report["checks"])


def describe(report: dict) -> str:
    lines = [f"{report['plugin']} ({report['runtime']})"]
    for check in report["checks"]:
        lines.append(("  ok  " if check["ok"] else "  FAIL") + f"  {check['name']}" + (f" — {check['detail']}" if check["detail"] else ""))
    failed = sum(1 for check in report["checks"] if not check["ok"])
    if failed == 0:
        lines += ["", "It will work in FlexDeck."]
    else:
        lines += ["", f"{failed} check{'' if failed == 1 else 's'} failed: fix {'it' if failed == 1 else 'them'} before anyone installs it."]
    return "\n".join(lines)
