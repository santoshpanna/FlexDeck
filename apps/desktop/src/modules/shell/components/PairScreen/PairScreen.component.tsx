import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Digits, Endpoint, Glyph, Key, Label, Led, Notice, Plate, Segmented, screen } from "@flexdeck/ui";
import { AsciiScene, globe, pulse, scan, typing } from "@flexdeck/ui/ascii";

import { useNow } from "../../hooks/useNow.hook.ts";
import { remaining } from "../../lib/countdown.lib.ts";
import { newTablet } from "../../lib/home.lib.ts";
import type { DeviceRow } from "../../lib/types.ts";
import type { ShellViewProps } from "../../view/shell.view.tsx";
import type { Tab } from "../../view/tabs.ts";

type Kind = "tablet" | "browser";

/**
 * How long each kind of code lasts, so its bar starts full and empties as it
 * expires. Mirror `DEFAULT_TTL_SECS` and `BROWSER_TTL_SECS` in
 * crates/flexdeck-pairing/src/session.rs.
 */
const PIN_LIFETIME = 120;
const BROWSER_CODE_LIFETIME = 300;

/** A code's remaining life, draining one second at a time. */
function ExpiryBar({ seconds, lifetime }: { seconds: number; lifetime: number }) {
  return (
    <div className="h-[3px] overflow-hidden rounded-full bg-line">
      <motion.div
        className="h-full rounded-full bg-signal shadow-[0_0_8px_var(--color-signal)]"
        initial={false}
        animate={{ width: `${Math.min(100, (seconds / lifetime) * 100)}%` }}
        transition={{ duration: 1, ease: "linear" }}
      />
    </div>
  );
}

/**
 * Pairing, in one place for both kinds of device: a code shown as a device
 * shows it, a bar that drains as it expires, and — for a tablet — the moment it
 * is used, the screen says so with the tablet's name.
 */
export function PairScreen({ p, go }: { p: ShellViewProps; go: (tab: Tab) => void }) {
  const [kind, setKind] = useState<Kind>("tablet");
  return (
    <div className="flex flex-col gap-4">
      <Segmented
        label="Pair what"
        value={kind}
        onChange={setKind}
        options={[
          { value: "tablet", label: "Tablet" },
          { value: "browser", label: "Browser" },
        ]}
        className="self-start"
      />
      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={kind} variants={screen} initial="initial" animate="animate" exit="exit">
          {kind === "tablet" ? <TabletPairing p={p} go={go} /> : <BrowserPairing p={p} go={go} />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function TabletPairing({ p, go }: { p: ShellViewProps; go: (tab: Tab) => void }) {
  const now = useNow();
  const left = p.pairing ? remaining(p.pairing.expiresAt, now) : null;
  const live = left !== null && !left.expired;

  // Which tablets existed when this code went up: a new one afterwards is the
  // tablet that just used it.
  const before = useRef<string[] | null>(null);
  const [paired, setPaired] = useState<DeviceRow | null>(null);
  useEffect(() => {
    if (live && before.current === null) before.current = p.devices.map((device) => device.id);
    if (!live) before.current = null;
  }, [live, p.devices]);
  useEffect(() => {
    if (before.current === null) return;
    const fresh = newTablet(before.current, p.devices);
    if (fresh) {
      setPaired(fresh);
      before.current = null;
    }
  }, [p.devices]);

  const drawPulse = useCallback((t: number) => pulse(t, 46, 11), []);
  const drawScan = useCallback((t: number) => scan(t, 52, 3), []);

  if (paired) {
    return (
      <motion.div initial={{ opacity: 0, scale: 0.94 }} animate={{ opacity: 1, scale: 1 }} transition={{ type: "spring", stiffness: 380, damping: 22 }}>
        <Plate depth="raised" pad="lg" className="flex flex-col gap-4 border-ok/40">
          <Label className="text-ok">Paired</Label>
          <div className="flex items-center gap-3.5">
            <span className="flex size-[52px] items-center justify-center rounded-[14px] border border-ok/40 bg-well text-ok">
              <Glyph name="tablet" size={26} />
            </span>
            <div className="min-w-0">
              <div className="truncate text-xl font-semibold">{paired.name}</div>
              <span className="flex items-center gap-2 font-mono text-[10px] tracking-[0.14em] text-muted">
                <Led state="ok" /> CONNECTED
              </span>
            </div>
          </div>
          <div className="flex gap-2.5">
            <Key tone="signal" size="lg" className="flex-1" onClick={() => go("home")}>
              Done
            </Key>
            <Key
              size="lg"
              onClick={() => {
                setPaired(null);
                p.onNewPin();
              }}
            >
              Pair another
            </Key>
          </div>
        </Plate>
      </motion.div>
    );
  }

  if (!live || !p.pairing || !left) {
    return (
      <Plate depth="well" pad="lg" className="flex flex-col items-start gap-3 overflow-hidden">
        <AsciiScene draw={drawPulse} fps={24} size={8} tone="text-faint" className="self-center" />
        <h2 className="m-0 text-2xl leading-tight font-semibold">Add a tablet</h2>
        <p className="m-0 text-[13px] leading-relaxed text-ink-2">
          {p.pairing ? "That code expired. " : ""}Show a code here, then on the tablet open FlexDeck and enter the
          address, port and code it shows. It works once.
        </p>
        <Key tone="signal" size="lg" disabled={p.busy} onClick={p.onNewPin}>
          Show a code <Glyph name="arrowRight" />
        </Key>
      </Plate>
    );
  }

  return (
    <div className="flex flex-col gap-3.5">
      <Plate depth="raised" pad="lg" className="flex flex-col items-center gap-4">
        <Label className="self-start">1 · Enter on the tablet</Label>
        <Endpoint host={p.address} port={p.port} />
        <Label className="self-start">2 · Then type this code</Label>
        <Digits value={p.pairing.pin} />
        <div className="flex w-full flex-col gap-2">
          <ExpiryBar seconds={left.seconds} lifetime={PIN_LIFETIME} />
          <div className="flex items-center justify-between font-mono text-[10px] tracking-[0.14em] text-muted">
            <span className="flex items-center gap-2">
              <Led state="wait" /> WAITING FOR TABLET
            </span>
            <span>{left.text}</span>
          </div>
        </div>
        <AsciiScene draw={drawScan} fps={24} size={8} tone="text-faint" />
        <div className="flex w-full gap-2.5">
          <Key size="md" className="flex-1" disabled={p.busy} onClick={p.onNewPin}>
            <Glyph name="refresh" /> New code
          </Key>
          <Key size="md" disabled={p.busy} onClick={p.onCancelPin}>
            Cancel
          </Key>
        </div>
      </Plate>
      <p className="m-0 px-0.5 text-[13px] leading-relaxed text-muted">
        The tablet must be on the same Wi-Fi as this computer. A new code replaces this one.
      </p>
    </div>
  );
}

function BrowserPairing({ p, go }: { p: ShellViewProps; go: (tab: Tab) => void }) {
  const now = useNow();
  const reachable = p.tunnel.enabled && p.tunnel.running;
  const left = p.browserPairing ? remaining(p.browserPairing.expiresAt, now) : null;
  const live = left !== null && !left.expired;

  const drawGlobe = useCallback((t: number) => globe(t, 46, 11), []);
  const drawTyping = useCallback((t: number) => typing(t, 52, 3), []);

  return (
    <div className="flex flex-col gap-3.5">
      {!reachable ? (
        <Notice tone="warn">
          <div className="flex flex-col items-start gap-2">
            <span>Turn on web access first, so the website can find this computer.</span>
            <Key size="sm" onClick={() => go("setup")}>
              Open Setup
            </Key>
          </div>
        </Notice>
      ) : null}
      {live && p.browserPairing && left ? (
        <Plate depth="raised" pad="lg" className="flex flex-col items-center gap-4">
          <Label>Type this into the website</Label>
          <Digits value={p.browserPairing.code} size="sm" />
          <div className="flex w-full flex-col gap-2">
            <ExpiryBar seconds={left.seconds} lifetime={BROWSER_CODE_LIFETIME} />
            <div className="flex items-center justify-between font-mono text-[10px] tracking-[0.14em] text-muted">
              <span>WORKS ONCE</span>
              <span>{left.text}</span>
            </div>
          </div>
          <AsciiScene draw={drawTyping} fps={24} size={8} tone="text-faint" />
          <div className="flex w-full gap-2.5">
            <Key size="md" className="flex-1" disabled={p.busy || !reachable} onClick={p.onNewBrowserCode}>
              <Glyph name="refresh" /> New code
            </Key>
            <Key size="md" disabled={p.busy} onClick={p.onCancelBrowserCode}>
              Cancel
            </Key>
          </div>
        </Plate>
      ) : (
        <Plate depth="well" pad="lg" className="flex flex-col items-start gap-3 overflow-hidden">
          <AsciiScene draw={drawGlobe} fps={24} size={8} tone="text-faint" className="self-center" />
          <h2 className="m-0 text-2xl leading-tight font-semibold">Add a browser</h2>
          <p className="m-0 text-[13px] leading-relaxed text-ink-2">
            {p.browserPairing ? "That code expired. " : ""}A browser on this computer connects by itself. For another
            device, show a code here and type it into the website there. It works once.
          </p>
          <Key tone="signal" size="lg" disabled={p.busy || !reachable} onClick={p.onNewBrowserCode}>
            Show a code <Glyph name="arrowRight" />
          </Key>
        </Plate>
      )}
    </div>
  );
}
