// Verifies that a configuration produced by the portal's editing model survives
// a round trip through a *real running agent*, reached through the cloud.
//
// The editing model is unit tested and the agent's API is integration tested,
// but neither checks the seam between them: that what the model emits is
// something the agent's proto parser will accept, that the revision it reports
// matches the one in the document, and that undo actually reverses the edit.
//
// It now also covers the seam the retarget added: that the cloud relays a call
// down a desktop's tunnel and hands back the agent's own answer, status and all.
//
// Ordinary tests cannot cover this, because it needs a live cloud, a live
// desktop and a live tunnel between them. Run it by hand after changing either
// side:
//
//   flexdeck --cloud ws://127.0.0.1:8787/tunnel   # turn web access on
//   # in the desktop app: Browsers -> Connect a browser, note the code
//   node scripts/roundtrip.check.ts http://127.0.0.1:8787 <code>
//
// The code pairs this script with the desktop exactly as a browser on another
// device would, so the relay below runs with the token the desktop issued.
//
// Exits non-zero if anything does not hold.

import { Api } from "../src/services/api/api.ts";
import { addWidget, findWidget, moveWidget } from "../src/modules/config/lib/editing.lib.ts";

const [url, code] = process.argv.slice(2);
if (!url || !code) {
  console.error("usage: node scripts/roundtrip.check.ts <base-url> <code-shown-by-the-desktop>");
  process.exit(2);
}

const api = new Api(url);
const failures: string[] = [];

function check(label: string, condition: boolean) {
  console.log(`${condition ? "ok  " : "FAIL"}  ${label}`);
  if (!condition) {
    failures.push(label);
  }
}

// Pair first, as a remote browser would, then route every call through the cloud.
const paired = await api.pair(code, "roundtrip check");
const desktopId = paired.fingerprint;
api.setRoutes(
  new Map([
    [desktopId, { fingerprint: desktopId, name: paired.name ?? "", token: paired.token, via: "relay" as const }],
  ]),
);

const before = await api.config(desktopId);
const beforeCount = before.profiles?.[0]?.pages?.[0]?.widgets?.length ?? 0;
console.log(`before: rev ${before.rev}, ${beforeCount} widget(s)`);

const withOne = addWidget(before, { type: "WIDGET_TYPE_BUTTON", props: { label: "Round trip" } });
const added = withOne.profiles?.[0]?.pages?.[0]?.widgets?.at(-1);
const moved = moveWidget(withOne, added?.id ?? "", 2, 1);

const put = await api.putConfig(desktopId, moved);
const after = await api.config(desktopId);

check("the agent accepted the configuration the model produced", put.rev > (before.rev ?? 0));
check(
  "the new widget came back",
  after.profiles?.[0]?.pages?.[0]?.widgets?.length === beforeCount + 1,
);
check("its id survived", findWidget(after, added?.id ?? "") !== null);
check("its label survived", findWidget(after, added?.id ?? "")?.widget.props?.label === "Round trip");
check("its position survived", findWidget(after, added?.id ?? "")?.widget.col === 2);
check(
  "the document's rev matches the revision the agent reported",
  after.rev === put.rev,
);

const undo = await api.undo(desktopId);
const reverted = await api.config(desktopId);
check("undo removed it again", findWidget(reverted, added?.id ?? "") === null);
check("undo went back a revision", undo.rev < put.rev);

const redo = await api.redo(desktopId);
const again = await api.config(desktopId);
check("redo brought it back", findWidget(again, added?.id ?? "") !== null);
check("redo went forward again", redo.rev === put.rev);

// Leave the agent as we found it.
await api.putConfig(desktopId, before);

if (failures.length > 0) {
  console.error(`\n${failures.length} check(s) failed`);
  process.exit(1);
}
console.log("\nall checks passed");
