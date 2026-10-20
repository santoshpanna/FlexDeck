import { AnimatePresence, motion, type Variants } from "motion/react";
import { useState } from "react";

import { Led, Mark, Notice, NoticeSlot, TabBar, ease } from "@flexdeck/ui";

import { ApprovalSheet } from "../components/ApprovalSheet/ApprovalSheet.component.tsx";
import { ConsentSheet } from "../components/ConsentSheet/ConsentSheet.component.tsx";
import { HomeScreen } from "../components/HomeScreen/HomeScreen.component.tsx";
import { PairScreen } from "../components/PairScreen/PairScreen.component.tsx";
import { PluginsScreen } from "../components/PluginsScreen/PluginsScreen.component.tsx";
import { SetupScreen } from "../components/SetupScreen/SetupScreen.component.tsx";
import type { Readout, NextStep } from "../lib/home.lib.ts";
import type { PluginGroup } from "../lib/plugins.lib.ts";
import type { StatusRow } from "../lib/status.lib.ts";
import type {
  ApprovalRequest,
  BrowserPairing,
  CatalogEntry,
  Consent,
  DeviceRow,
  InstalledPlugin,
  PairingPin,
  RepoPreview,
  Repository,
  TunnelState,
  WindowPrefs,
} from "../lib/types.ts";
import { TABS, tabIndex, type Tab } from "./tabs.ts";

/**
 * Every field is required: this is the controller's contract, and a forgotten
 * one is then a compile error at the composition site rather than a blank space
 * in the window. The view is declared as taking `Partial` so the page can write
 * `<ShellView />` with nothing filled in.
 */
export interface ShellViewProps {
  rows: StatusRow[];
  readouts: Readout[];
  next: NextStep | null;
  /** The address a tablet types to find this computer. */
  address: string;
  /** The port that goes with it. */
  port: number | null;
  devices: DeviceRow[];
  plugins: PluginGroup[];
  installed: InstalledPlugin[];
  consent: Consent | null;
  catalog: CatalogEntry[] | null;
  repositories: Repository[];
  vettedError: string | null;
  fetchedAt: number | null;
  preview: RepoPreview | null;
  logs: Record<string, string[]>;
  pairing: PairingPin | null;
  browserPairing: BrowserPairing | null;
  approvals: ApprovalRequest[];
  fingerprint: string;
  version: string;
  tunnel: TunnelState;
  /** How the window starts and closes; null until first read. */
  windowPrefs: WindowPrefs | null;
  onSetWindowPrefs: (patch: Partial<WindowPrefs>) => void;
  error: string | null;
  consentError: string | null;
  busy: boolean;
  loading: boolean;
  onNewPin: () => void;
  onCancelPin: () => void;
  onNewBrowserCode: () => void;
  onCancelBrowserCode: () => void;
  onAnswer: (id: number, allow: boolean) => void;
  onRevoke: (deviceId: string) => void;
  /** Remove every paired tablet and browser. */
  onRevokeAll: () => void;
  onToggleWebAccess: (enabled: boolean) => void;
  onSetPortal: (url: string) => void;
  /** Open a web page in this computer's browser. */
  onOpenLink: (url: string) => void;
  onChooseFile: (file: File) => void;
  onPick: (entry: CatalogEntry) => void;
  onRefresh: () => void;
  onPreviewRepo: (url: string) => void;
  onAddRepo: (url: string) => void;
  onCancelPreview: () => void;
  onRemoveRepo: (url: string) => void;
  onToggleRepo: (url: string, enabled: boolean) => void;
  onAccept: () => void;
  onCancel: () => void;
  onLink: (path: string) => void;
  onUninstall: (id: string) => void;
  onEnable: (id: string, enabled: boolean) => void;
  onReload: (id: string) => void;
  onSaveSettings: (id: string, values: Record<string, string>) => void;
  onShowLog: (id: string) => void;
}

const noop = () => {};

/// What the view shows before the controller has said anything. "No portal"
/// rather than "off" so a first paint cannot claim a portal that does not exist.
const DEFAULTS: ShellViewProps = {
  rows: [],
  readouts: [],
  next: null,
  address: "",
  port: null,
  devices: [],
  plugins: [],
  installed: [],
  consent: null,
  catalog: null,
  repositories: [],
  vettedError: null,
  fetchedAt: null,
  preview: null,
  logs: {},
  pairing: null,
  browserPairing: null,
  approvals: [],
  fingerprint: "",
  version: "",
  tunnel: { enabled: false, running: false, refused: null, lastError: null, portal: null },
  windowPrefs: null,
  onSetWindowPrefs: noop,
  error: null,
  consentError: null,
  busy: false,
  loading: false,
  onNewPin: noop,
  onCancelPin: noop,
  onNewBrowserCode: noop,
  onCancelBrowserCode: noop,
  onAnswer: noop,
  onRevoke: noop,
  onRevokeAll: noop,
  onToggleWebAccess: noop,
  onSetPortal: noop,
  onOpenLink: noop,
  onChooseFile: noop,
  onPick: noop,
  onRefresh: noop,
  onPreviewRepo: noop,
  onAddRepo: noop,
  onCancelPreview: noop,
  onRemoveRepo: noop,
  onToggleRepo: noop,
  onAccept: noop,
  onCancel: noop,
  onLink: noop,
  onUninstall: noop,
  onEnable: noop,
  onReload: noop,
  onSaveSettings: noop,
  onShowLog: noop,
};

/** Screens slide along the tab order: forward to the right, back to the left. */
const slide: Variants = {
  enter: (direction: number) => ({ opacity: 0, x: direction * 28 }),
  center: { opacity: 1, x: 0, transition: { duration: 0.32, ease: ease.outExpo } },
  exit: (direction: number) => ({ opacity: 0, x: direction * -20, transition: { duration: 0.14 } }),
};

/**
 * The window as a device face: a status header, one screen at a time, and a
 * row of keys to move between them. Requests that need an answer drop in over
 * everything; installs ask in a sheet that rises over the list they came from.
 */
export function ShellView(partial: Partial<ShellViewProps>) {
  const p: ShellViewProps = { ...DEFAULTS, ...partial };
  const [tab, setTab] = useState<Tab>("home");
  const [direction, setDirection] = useState(1);

  const go = (next: Tab) => {
    if (next === tab) return;
    setDirection(tabIndex(next) > tabIndex(tab) ? 1 : -1);
    setTab(next);
  };

  const updates = (p.catalog ?? []).filter((entry) => entry.update).length;
  const items = TABS.map((item) =>
    item.value === "plugins" ? { ...item, badge: updates } : item.value === "home" ? { ...item, badge: p.rows.some((row) => row.tone === "warn") ? 1 : null } : item,
  );
  const allOk = p.readouts.length > 0 && p.readouts.every((readout) => readout.led !== "warn");

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-body bg-dots text-ink">
      <header className="flex h-[52px] shrink-0 items-center justify-between border-b border-line/70 px-4">
        <div className="flex items-center gap-2.5">
          <Mark className="h-5" />
          <span className="text-sm font-bold tracking-[0.3em]">FLEXDECK</span>
          {p.version ? (
            <span className="font-mono text-[10px] tracking-[0.12em] text-faint">
              {p.version === "0.0.0" ? "DEV" : `V${p.version.toUpperCase()}`}
            </span>
          ) : null}
        </div>
        <span className="flex items-center gap-2 font-mono text-[10px] tracking-[0.16em] text-muted">
          <Led state={p.loading ? "wait" : allOk ? "ok" : "warn"} />
          {p.loading ? "READING" : allOk ? "NOMINAL" : "CHECK"}
        </span>
      </header>

      <main className="relative min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-4 pt-4 pb-4">
        <NoticeSlot>
          {p.error !== null ? (
            <Notice key="error" className="mb-3">
              {p.error}
            </Notice>
          ) : null}
        </NoticeSlot>

        <AnimatePresence mode="wait" custom={direction} initial={false}>
          <motion.div key={tab} custom={direction} variants={slide} initial="enter" animate="center" exit="exit">
            {tab === "home" ? <HomeScreen p={p} go={go} /> : null}
            {tab === "pair" ? <PairScreen p={p} go={go} /> : null}
            {tab === "plugins" ? <PluginsScreen p={p} /> : null}
            {tab === "setup" ? <SetupScreen p={p} /> : null}
          </motion.div>
        </AnimatePresence>
      </main>

      <TabBar items={items} value={tab} onChange={go} className="mx-2.5 mb-2.5 shrink-0" />

      <ConsentSheet consent={p.consent} busy={p.busy} error={p.consentError} onAccept={p.onAccept} onCancel={p.onCancel} />
      <ApprovalSheet approvals={p.approvals} busy={p.busy} onAnswer={p.onAnswer} />
    </div>
  );
}
