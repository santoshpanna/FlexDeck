import { AnimatePresence, motion } from "motion/react";
import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";

import { Glyph, HoldToConfirm, Key, Label, Led, Notice, Pager, Pill, Plate, Readout, item, list, paginate, useSteadyHeight } from "@flexdeck/ui";
import { AsciiScene, render } from "@flexdeck/ui/ascii";

import type { DeviceKind } from "../../lib/types.ts";
import type { ShellViewProps } from "../../view/shell.view.tsx";
import type { Tab } from "../../view/tabs.ts";

const KIND: Record<DeviceKind, string> = {
  tablet: "Tablet",
  "browser-local": "Browser",
  "browser-remote": "Browser",
};

/**
 * Where the window opens: three lights for the state of things, then the one
 * thing worth doing next, then who is connected. Everything else is a tab away.
 */
/** A short page: the window is narrow and the readouts sit above the list. */
const DEVICES_PER_PAGE = 6;

export function HomeScreen({ p, go }: { p: ShellViewProps; go: (tab: Tab) => void }) {
  const attention = p.rows.filter((row) => row.tone === "warn" && row.label === "Needs attention");
  const [page, setPage] = useState(0);
  const newest = useMemo(() => [...p.devices].sort((a, b) => b.created_at - a.created_at), [p.devices]);
  const paged = paginate(newest, page, DEVICES_PER_PAGE);
  const steady = useSteadyHeight<HTMLUListElement>(paged.pages);

  return (
    <motion.div variants={list} initial="initial" animate="animate" className="flex flex-col gap-3.5">
      <motion.div variants={item}>
        {/* Three indicator modules, like lamps set into a panel: what each one
            watches in the corner, its light beside it, the reading below. */}
        <div className="grid grid-cols-3 gap-2">
          {(p.readouts.length > 0 ? p.readouts : PLACEHOLDER).map((readout) => (
            <Plate key={readout.label} depth="well" pad="none" className="flex min-w-0 flex-col gap-2.5 px-3 pt-2.5 pb-3">
              <div className="flex items-center justify-between gap-2">
                <Label className="truncate">{readout.label}</Label>
                <Led state={readout.led} />
              </div>
              <span className="truncate font-mono text-[15px] leading-none font-semibold tracking-[0.04em] text-ink uppercase">
                {readout.value}
              </span>
            </Plate>
          ))}
        </div>
      </motion.div>

      <motion.div variants={item}>
        <NextStepPlate p={p} go={go} />
      </motion.div>

      {attention.length > 0 ? (
        <motion.div variants={item} className="flex flex-col gap-2">
          {attention.map((row, index) => (
            <Notice key={index} tone="warn">
              {row.detail}
            </Notice>
          ))}
        </motion.div>
      ) : null}

      <motion.section variants={item} className="flex flex-col gap-1.5" aria-label="Devices">
        <div className="flex items-center justify-between px-0.5 pb-1">
          <Label>Devices · {p.devices.length}</Label>
          <div className="flex items-center gap-2">
            {p.devices.length > 1 ? (
              <HoldToConfirm size="sm" tone="danger" disabled={p.busy} onConfirm={p.onRevokeAll}>
                Remove all
              </HoldToConfirm>
            ) : null}
            <button type="button" onClick={() => go("pair")} className="lbl cursor-pointer hover:text-ink">
              + Add
            </button>
          </div>
        </div>
        {p.devices.length === 0 ? (
          <p className="px-0.5 text-[13px] text-muted">No tablets or browsers yet.</p>
        ) : (
          // A page change swaps the rows outright: no fade, no rows animating in or
          // out, so nothing moves. The steady height keeps a short last page from
          // pulling the pager up.
          <ul
            key={paged.page}
            ref={steady.ref}
            style={steady.style}
            className="flex flex-col gap-1.5"
          >
            <AnimatePresence initial={false}>
              {paged.items.map((device) => (
                <motion.li
                  key={device.id}
                  layout
                  variants={item}
                  initial="initial"
                  animate="animate"
                  exit="exit"
                  className="flex items-center gap-3 rounded-xl border border-line bg-plate px-3 py-2.5"
                >
                  <Led
                    state={device.kind === "tablet" ? (device.online ? "ok" : "off") : "off"}
                    label={device.kind === "tablet" ? (device.online ? "Connected" : "Not connected") : undefined}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className="truncate text-sm">{device.name}</span>
                      <Pill>{KIND[device.kind] ?? "Device"}</Pill>
                    </div>
                    {/* Not "last seen": the agent records when a device paired, not when it was last here. */}
                    <Readout className="text-[10px] tracking-[0.08em]">
                      Added {new Date(device.created_at * 1000).toLocaleDateString()}
                      {device.online ? <span className="text-ink"> · connected</span> : null}
                    </Readout>
                  </div>
                  <HoldToConfirm size="sm" tone="danger" disabled={p.busy} onConfirm={() => p.onRevoke(device.id)}>
                    Remove
                  </HoldToConfirm>
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        )}
        <Pager {...paged} total={p.devices.length} onPage={setPage} className="pt-1" />
      </motion.section>
    </motion.div>
  );
}

const PLACEHOLDER = [
  { label: "Agent", value: "—", led: "wait" as const },
  { label: "Tablets", value: "—", led: "off" as const },
  { label: "Web", value: "—", led: "off" as const },
];

function NextStepPlate({ p, go }: { p: ShellViewProps; go: (tab: Tab) => void }) {
  const next = p.next;
  const draw = useCallback((t: number) => {
    const lit = Array.from({ length: 15 }, (_, k) => (k === Math.floor(t * 3) % 15 ? 1 : 0));
    return render({ cols: 58, rows: 26, yaw: 0.7 + Math.sin(t * 0.35) * 0.4, elev: 0.85, dist: 5, fl: 2.6, t, lit, floor: false });
  }, []);

  let tag = "Next step";
  let title = "Reading this computer…";
  let body: ReactNode = null;
  let action: ReactNode = null;

  if (next?.kind === "blocked") {
    tag = "Needs attention";
    title = "Tablets can’t reach this PC.";
    body = next.detail;
  } else if (next?.kind === "pair") {
    title = "Pair your first tablet.";
    body = "Takes about twenty seconds. Keep the tablet on the same Wi-Fi as this computer.";
    action = (
      <Key
        tone="signal"
        size="lg"
        onClick={() => {
          if (p.pairing === null || p.pairing.expiresAt * 1000 < Date.now()) p.onNewPin();
          go("pair");
        }}
      >
        Show a code <Glyph name="arrowRight" />
      </Key>
    );
  } else if (next?.kind === "wake") {
    title = `Wake ${next.tablet}.`;
    body = "Open FlexDeck on it. It reconnects by itself, with nothing to type.";
    action = (
      <Key size="md" onClick={() => go("pair")}>
        Pair another tablet
      </Key>
    );
  } else if (next?.kind === "build") {
    title = "Build your deck.";
    body = next.editor
      ? "Open the editor in a browser on this computer. The tablet updates as you go."
      : "Set the portal address under Setup to get a link to the editor.";
    action = next.editor ? <EditorLink url={next.editor} onOpen={p.onOpenLink} /> : (
      <Key size="md" onClick={() => go("setup")}>
        Open Setup
      </Key>
    );
  }

  return (
    <Plate depth="well" pad="lg" className="overflow-hidden">
      <AsciiScene
        draw={draw}
        fps={20}
        size={6}
        tone="text-faint"
        className="pointer-events-none absolute -top-2 -right-16 opacity-80"
      />
      <div className="relative flex max-w-[230px] flex-col gap-2.5">
        <Label className={next?.kind === "blocked" ? "text-warn" : "text-signal"}>{tag}</Label>
        <AnimatePresence mode="wait" initial={false}>
          <motion.h2
            key={title}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.24 }}
            className="m-0 text-[26px] leading-[1.05] font-semibold tracking-[-0.01em]"
          >
            {title}
          </motion.h2>
        </AnimatePresence>
        {body ? <p className="m-0 text-[13px] leading-relaxed text-ink-2">{body}</p> : null}
        {action ? <div className="pt-1.5">{action}</div> : null}
      </div>
    </Plate>
  );
}

/**
 * Open the editor in this computer's browser, or copy its link to paste
 * elsewhere. The address itself stays out of sight: nobody types it.
 */
function EditorLink({ url, onOpen }: { url: string; onOpen: (url: string) => void }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  return (
    <div className="flex gap-2">
      <Key tone="signal" size="md" className="flex-1" onClick={() => onOpen(url)}>
        <Glyph name="globe" /> Open editor
      </Key>
      <Key
        size="md"
        aria-label={copied ? "Link copied" : "Copy editor link"}
        title={copied ? "Copied" : "Copy link"}
        onClick={() => {
          void navigator.clipboard.writeText(url).then(() => {
            setCopied(true);
            if (timer.current) clearTimeout(timer.current);
            timer.current = setTimeout(() => setCopied(false), 1600);
          });
        }}
      >
        <Glyph name={copied ? "check" : "copy"} />
      </Key>
    </div>
  );
}
