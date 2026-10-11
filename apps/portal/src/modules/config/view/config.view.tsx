import type { GridSpec, Orientation } from "@flexdeck/grid";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { AppLink } from "../../../common/components/AppLink/AppLink.component.tsx";
import { SITE_HREF } from "../../../common/site/site.ts";

import { Key, Label, Led, Mark, Notice, NoticeSlot, Plate, ease, spring } from "@flexdeck/ui";
import { AsciiScene, render, rings, scan } from "@flexdeck/ui/ascii";

import { Tabs } from "../../../common/components/Tabs/Tabs.component.tsx";
import { ThemeToggle } from "../../../common/components/ThemeToggle/ThemeToggle.component.tsx";
import type {
  ActionDefinition,
  Automation,
  AutomationDocument,
  DesktopSummary,
  Device,
  PairingBegin,
  PluginEventChoice,
  Run,
  Status,
  VariableChoice,
  WireAction,
  WireConfig,
  WirePage,
  WireProfile,
} from "../../../services/api/api.ts";
import { AddDesktop } from "../components/AddDesktop/AddDesktop.component.tsx";
import { AgentCard } from "../components/AgentCard/AgentCard.component.tsx";
import { AutomationWorkbench } from "../components/AutomationEditor/AutomationWorkbench.component.tsx";
import { DesktopSwitcher } from "../components/DesktopSwitcher/DesktopSwitcher.component.tsx";
import { DevicesCard } from "../components/DevicesCard/DevicesCard.component.tsx";
import { GridPreview } from "../components/GridPreview/GridPreview.component.tsx";
import { useFacePreviews } from "../hooks/useFacePreviews/useFacePreviews.hook.ts";
import { PageBackgroundEditor } from "../components/PageBackgroundEditor/PageBackgroundEditor.component.tsx";
import { SourcesPanel } from "../components/SourcesPanel/SourcesPanel.component.tsx";
import { LocaleCard } from "../components/LocaleCard/LocaleCard.component.tsx";
import { setPageBackground, setScreenCare } from "../lib/face.edit.lib.ts";
import { ScreenCareCard } from "../components/ScreenCareCard/ScreenCareCard.component.tsx";
import { ShareCard } from "../components/ShareCard/ShareCard.component.tsx";
import { GridSizePanel } from "../components/GridSizePanel/GridSizePanel.component.tsx";
import { PageDots } from "../components/PageDots/PageDots.component.tsx";
import { PageKeys, PageRail } from "../components/PageRail/PageRail.component.tsx";
import { PairingCard } from "../components/PairingCard/PairingCard.component.tsx";
import { PluginsPanel } from "../components/PluginsPanel/PluginsPanel.component.tsx";
import { RawConfigCard } from "../components/RawConfigCard/RawConfigCard.component.tsx";
import { WidgetInspector } from "../components/WidgetInspector/WidgetInspector.component.tsx";
import type { LoadChoices } from "../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import type { AutomationsStatus } from "../hooks/useAutomations/useAutomations.hook.ts";
// Type-only: the view stays presentational, and the vocabulary of the read has
// exactly one definition — the hook that produces it.
import type { DocumentStatus } from "../hooks/useConfigDocument/useConfigDocument.hook.ts";
import { useSwipe } from "../hooks/useSwipe/useSwipe.hook.ts";
import type { GridSizePatch } from "../lib/grid.lib.ts";
import { layoutFor, portraitSpecOf } from "../lib/orientation.lib.ts";
import type { WidgetState } from "../lib/state.lib.ts";
import { CONFIG_TABS, type ConfigTab } from "../lib/tabs.lib.ts";

/**
 * What the configurator shows.
 *
 * Every field is required *here* because this is the contract `ConfigController`
 * has to satisfy — omitting one is a compile error at the controller, which is
 * where the check belongs. The view itself takes a `Partial`, because a view
 * element is written without props at the composition site in `config.page.tsx`
 * and cannot be given them until it is cloned.
 */
export interface ConfigViewProps {
  // Which desktop is being configured, and connecting another.
  desktops: DesktopSummary[] | null;
  desktop: DesktopSummary | null;
  selectDesktop: (desktopId: string) => void;
  pair: (code: string) => Promise<string>;

  // Failure, at the two levels that can fail independently.
  desktopsError: string | null;
  onRetryDesktops: () => void;
  error: string | null;
  /** Re-read everything, for the state where this desktop cannot be reached. */
  onRetry: () => void;

  // What the agent reports about itself.
  status: Status | null;
  devices: Device[];
  revokeDevice: (deviceId: string) => void;

  // Pairing a new tablet.
  pairing: PairingBegin | null;
  beginPairing: () => void;
  cancelPairing: () => void;

  // The document, and the grid it describes.
  catalogue: ActionDefinition[];
  /** Every event a plugin on this desktop can announce, for triggers. */
  pluginEvents: PluginEventChoice[];
  /** Re-read the catalogue after a plugin is installed or removed. */
  reloadCatalogue: () => void;
  config: WireConfig | null;
  /**
   * What the read of that document is doing. The three "no document" states are
   * NOT interchangeable: an editable deck must only be drawn for `loaded`.
   */
  documentStatus: DocumentStatus;
  spec: GridSpec;
  profile: WireProfile | null;
  page: WirePage | null;
  caption: string;
  /// Which layout is being edited: each has its own grid and its own places.
  orientation: Orientation;
  setOrientation: (orientation: Orientation) => void;
  /** Whether portrait has a grid of its own, rather than landscape's turned. */
  portraitOwnGrid: boolean;
  setPortraitGrid: (patch: GridSizePatch) => void;
  clearPortraitGrid: () => void;
  movePortrait: (widgetId: string, col: number, row: number) => void;
  resizePortrait: (widgetId: string, w: number, h: number) => void;
  addAtPortrait: (pageId: string, col: number, row: number) => void;
  unpinPortrait: (widgetId: string) => void;

  // Which page is showing, and the operations on pages.
  pages: WirePage[];
  currentPageId: string | null;
  selectPage: (pageId: string) => void;
  addPage: () => void;
  removePage: (pageId: string) => void;
  renamePage: (pageId: string, name: string) => void;
  movePage: (pageId: string, delta: number) => void;
  duplicatePage: (pageId: string) => void;
  setGrid: (patch: GridSizePatch) => void;

  previewState: WidgetState;
  setPreviewState: (state: WidgetState) => void;
  setIcon: (icon: string | null) => void;
  /** Bind the selected widget to live values. */
  setBinding: (binding: { stateVariable: string; labelTemplate: string }) => void;
  /** Apply a pure edit to the selected widget (its flows, its slider, its hold). */
  editSelected: (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;
  editPage: (transform: (config: WireConfig, pageId: string) => WireConfig) => void;
  /** Every live value a plugin owns, for binding widgets. */
  variables: VariableChoice[];
  duplicate: () => void;
  addAt: (pageId: string, col: number, row: number) => void;
  moveTo: (widgetId: string, col: number, row: number) => void;
  resize: (widgetId: string, w: number, h: number) => void;

  // Which section of the configurator is showing.
  tab: ConfigTab;
  selectTab: (tab: ConfigTab) => void;

  // Automations. The view is presentational — the controller owns when the
  // agent is written (a short debounce after edits stop).
  automationsDraft: AutomationDocument | null;
  automationsStatus: AutomationsStatus;
  automationsSaving: "idle" | "saving" | "saved" | "error";
  automationsDirty: boolean;
  selectedAutomation: Automation | null;
  selectedAutomationId: string | null;
  selectAutomation: (automationId: string | null) => void;
  editAutomations: (next: AutomationDocument) => void;
  saveAutomations: () => void;
  reloadAutomations: () => void;
  dryRunAutomation: (automationId: string) => Promise<void>;
  /// The trace of the last dry-run of the selected automation, or `null`.
  lastRun: Run | null;

  // Selection, and editing the selected widget.
  selectedId: string | null;
  select: (widgetId: string | null) => void;
  /// Whether the key inspector is open. Deliberately separate from `selectedId`:
  /// dragging a key selects it, and a panel that opened on pointer-down would
  /// change the layout under the drag.
  drawerOpen: boolean;
  /** Open the inspector for a key — a click on it, or its hover "edit" action. */
  openWidget: (widgetId: string) => void;
  closeWidget: () => void;
  labelDraft: string;
  setLabelDraft: (value: string) => void;
  commitLabel: () => void;
  actionDraft: Record<string, string>;
  setActionDraft: (draft: Record<string, string>) => void;
  selectedAction: WireAction | null;
  refusal: string | null;
  dismissRefusal: () => void;

  move: (dc: number, dr: number) => void;
  removeSelected: () => void;
  removeWidget: (widgetId: string) => void;
  saveAction: (name: string) => void;
  /** Save the arguments: as the fields hold them, or as `draft` when given. */
  commitActionArgs: (draft?: Record<string, string>) => void;
  /** Ask the plugin what an action's arguments can be set to. */
  loadChoices?: LoadChoices;
}

/// The grid a configuration that has not loaded yet describes: no cells, so the
/// preview draws nothing rather than drawing something invented.
const EMPTY_SPEC: GridSpec = { cols: 0, rows: 0, gapX: 0, gapY: 0, usableW: 0, usableH: 0 };

const NEVER = () => undefined;

/**
 * The configurator as a workbench: a header with which desktop and which
 * section, and the section itself. The deck section is three columns — pages as
 * keys, the device on a stage, and an inspector for the key in hand (or for the
 * page, when no key is).
 */
export function ConfigView(props: Partial<ConfigViewProps>) {
  const {
    desktops = null,
    desktop = null,
    selectDesktop = NEVER,
    pair = unsupportedPair,
    desktopsError = null,
    onRetryDesktops = NEVER,
    error = null,
    onRetry = NEVER,
    status = null,
    devices = [],
    revokeDevice = NEVER,
    pairing = null,
    beginPairing = NEVER,
    cancelPairing = NEVER,
    catalogue = [],
    pluginEvents = [],
    reloadCatalogue = NEVER,
    config = null,
    documentStatus = "idle",
    spec = EMPTY_SPEC,
    profile = null,
    orientation = "landscape",
    setOrientation = NEVER,
    portraitOwnGrid = false,
    setPortraitGrid = NEVER,
    clearPortraitGrid = NEVER,
    movePortrait = NEVER,
    resizePortrait = NEVER,
    addAtPortrait = NEVER,
    unpinPortrait = NEVER,
    page = null,
    caption = "no page",
    pages = [],
    currentPageId = null,
    selectPage = NEVER,
    addPage = NEVER,
    removePage = NEVER,
    renamePage = NEVER,
    movePage = NEVER,
    duplicatePage = NEVER,
    setGrid = NEVER,
    previewState = "idle",
    setPreviewState = NEVER,
    setIcon = NEVER,
    setBinding = NEVER,
    editSelected = NEVER,
    editPage = NEVER,
    variables = [],
    duplicate = NEVER,
    addAt = NEVER,
    moveTo = NEVER,
    resize = NEVER,
    tab = "deck",
    selectTab = NEVER,
    automationsDraft = null,
    automationsStatus = "idle",
    automationsSaving = "idle",
    automationsDirty = false,
    selectedAutomationId = null,
    selectAutomation = NEVER,
    editAutomations = NEVER,
    saveAutomations = NEVER,
    reloadAutomations = NEVER,
    dryRunAutomation = async () => undefined,
    lastRun = null,
    selectedId = null,
    select = NEVER,
    drawerOpen = false,
    openWidget = NEVER,
    closeWidget = NEVER,
    labelDraft = "",
    setLabelDraft = NEVER,
    commitLabel = NEVER,
    refusal = null,
    dismissRefusal = NEVER,

    removeSelected = NEVER,
    removeWidget = NEVER,
    loadChoices,
  } = props;

  /// Swiping across the deck moves to the next or previous page. A gesture that
  /// starts on a key is stopped by that key first, so this never competes with
  /// dragging one.
  const swipe = useSwipe((direction) => {
    const index = pages.findIndex((candidate) => candidate.id === currentPageId);
    if (index < 0) return;
    const next = direction === "left" ? pages[index + 1] : pages[index - 1];
    if (next) selectPage(next.id);
  });

  const previews = useFacePreviews(desktop?.id ?? null, page?.widgets ?? []);
  // The state the selected key is drawn in while editing; a new key starts on whichever applies.
  const [previewLook, setPreviewLook] = useState("");
  useEffect(() => setPreviewLook(""), [selectedId]);
  const portrait = orientation === "portrait";
  const placement = selectedId ? (layoutFor(profile, page, orientation).widgets.find((span) => span.id === selectedId) ?? null) : null;

  const selectedWidget = selectedId ? ((page?.widgets ?? []).find((widget) => widget.id === selectedId) ?? null) : null;
  const panelOpen = drawerOpen && selectedWidget !== null;
  const loaded = desktop !== null && documentStatus === "loaded";
  const rev = config?.rev ?? status?.rev ?? null;

  return (
    <div className="min-h-screen bg-body bg-dots text-ink">
      <header className="sticky top-0 z-30 border-b border-line/80 bg-body/80 backdrop-blur-md">
        <div className="mx-auto flex max-w-[1680px] flex-wrap items-center gap-x-5 gap-y-3 px-5 py-3">
          <AppLink to={SITE_HREF} className="flex items-center gap-2.5 text-[15px] font-bold tracking-[0.3em] text-ink no-underline">
            <Mark className="h-5" />
            FLEXDECK
          </AppLink>
          <DesktopSwitcher desktops={desktops} selected={desktop} onSelect={selectDesktop} />
          {loaded ? <Tabs tabs={CONFIG_TABS} active={tab} onSelect={selectTab} /> : null}
          <div className="ml-auto flex items-center gap-4">
            {loaded && rev !== null ? (
              <span className="flex items-center gap-2 font-mono text-[10px] tracking-[0.18em] text-muted" aria-live="polite">
                <motion.span key={rev} initial={{ scale: 2.2 }} animate={{ scale: 1 }} transition={spring.key} className="flex">
                  <Led state="ok" />
                </motion.span>
                SYNCED · REV {String(rev).padStart(3, "0")}
              </span>
            ) : null}
            <ThemeToggle />
          </div>
        </div>
      </header>

      <main className="mx-auto flex max-w-[1680px] flex-col gap-4 px-5 py-6">
        <NoticeSlot>
          {desktopsError ? (
            <Notice key="desktops">
              {desktopsError}{" "}
              <button type="button" onClick={onRetryDesktops} className="cursor-pointer text-signal underline-offset-4 hover:underline">
                Try again
              </button>
            </Notice>
          ) : null}
          {error ? <Notice key="error">{error}</Notice> : null}
        </NoticeSlot>

        {loaded && desktop ? (
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={tab}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0, transition: { duration: 0.32, ease: ease.outExpo } }}
              exit={{ opacity: 0, y: -6, transition: { duration: 0.14 } }}
            >
              {tab === "deck" ? (
                <div className="grid items-start gap-5 lg:grid-cols-[64px_minmax(0,1fr)] xl:grid-cols-[64px_minmax(0,1fr)_420px]">
                  <PageKeys pages={pages} currentId={currentPageId} onSelect={selectPage} onAdd={addPage} onMove={movePage} />

                  <section aria-label="Deck" className="relative flex min-w-0 flex-col gap-4">
                    {/* Floats over the deck: in the flow it would push the grid down and back up on every refused drag. */}
                    <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex flex-col [&>*]:pointer-events-auto">
                      <NoticeSlot>
                        {refusal ? (
                          <Notice key="refusal" tone="warn" onDismiss={dismissRefusal} className="shadow-plate">
                            {refusal}
                          </Notice>
                        ) : null}
                      </NoticeSlot>
                    </div>
                    <div {...swipe}>
                      <GridPreview
                        profile={profile}
                        page={page}
                        orientation={orientation}
                        onOrientation={setOrientation}
                        caption={caption}
                        selectedId={selectedId}
                        onSelect={select}
                        onEdit={openWidget}
                        onDelete={removeWidget}
                        previewState={previewState}
                        previews={previews}
                        previewLook={previewLook}
                        onMove={portrait ? movePortrait : moveTo}
                        onResize={portrait ? resizePortrait : resize}
                        onAdd={(col, row) => currentPageId && (portrait ? addAtPortrait : addAt)(currentPageId, col, row)}
                        footer={<PageDots pages={pages} currentId={currentPageId} onSelect={selectPage} />}
                      />
                    </div>
                    <p className="m-0 text-center font-mono text-[10px] tracking-[0.16em] text-faint">
                      {portrait ? "PORTRAIT LAYOUT · DRAG A KEY TO PIN IT HERE · CTRL+Z UNDO" : "CLICK A KEY TO EDIT · DRAG TO MOVE · CORNER TO RESIZE · CTRL+Z UNDO"}
                    </p>
                  </section>

                  <aside
                    aria-label={panelOpen ? "Key inspector" : "Page"}
                    className="lg:col-span-2 xl:sticky xl:top-[76px] xl:col-span-1 xl:max-h-[calc(100vh-96px)]"
                  >
                    <div className="flex h-full flex-col overflow-hidden rounded-plate border border-line bg-plate xl:max-h-[calc(100vh-96px)]">
                      <AnimatePresence mode="wait" initial={false}>
                        {panelOpen && selectedWidget ? (
                          <motion.div
                            key={`key-${selectedWidget.id}`}
                            className="flex min-h-0 flex-1 flex-col"
                            initial={{ opacity: 0, x: 16 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: 10 }}
                            transition={spring.panel}
                          >
                            <WidgetInspector
                              widget={selectedWidget}
                              catalogue={catalogue}
                              variables={variables}
                              loadChoices={loadChoices}
                              labelDraft={labelDraft}
                              onLabelChange={setLabelDraft}
                              onCommitLabel={commitLabel}
                              onIcon={setIcon}
                              onBinding={setBinding}
                              onEdit={editSelected}
                              previewState={previewState}
                              onPreviewState={setPreviewState}
                              preview={previews[selectedWidget.id]}
                              previewLook={previewLook}
                              onPreviewLook={setPreviewLook}
                              orientation={orientation}
                              placement={placement}
                              onUnpin={() => selectedWidget && unpinPortrait(selectedWidget.id)}
                              onDuplicate={duplicate}
                              onRemove={removeSelected}
                              onClose={closeWidget}
                            />
                          </motion.div>
                        ) : (
                          <motion.div key="page" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex flex-col gap-6 p-5">
                            <section className="flex flex-col gap-3">
                              <Label>This page</Label>
                              <PageRail
                                pages={pages}
                                currentId={currentPageId}
                                onAdd={addPage}
                                onRename={renamePage}
                                onMove={movePage}
                                onDuplicate={duplicatePage}
                                onRemove={removePage}
                              />
                            </section>
                            <PageBackgroundEditor page={page} variables={variables} onChange={(background) => editPage((c, id) => setPageBackground(c, id, background))} />
                            <ScreenCareCard care={profile?.screen} onChange={(care) => editPage((c, id) => setScreenCare(c, id, care))} />
                            <ShareCard page={page} onEdit={editPage} />
                            <section className="flex flex-col gap-3">
                              <div className="flex items-center justify-between">
                                <Label>{portrait ? "Portrait grid" : "Landscape grid"}</Label>
                                {portrait && portraitOwnGrid ? (
                                  <button type="button" onClick={clearPortraitGrid} className="lbl cursor-pointer hover:text-ink">
                                    Use landscape, turned
                                  </button>
                                ) : null}
                              </div>
                              <GridSizePanel spec={portrait ? portraitSpecOf(profile) : spec} onChange={portrait ? setPortraitGrid : setGrid} />
                              <p className="m-0 text-[12px] leading-relaxed text-faint">
                                {portrait
                                  ? portraitOwnGrid
                                    ? "Portrait has its own grid. Keys you have not placed here are fitted in for you."
                                    : "Portrait uses the landscape grid turned on its side. Change it to give portrait its own."
                                  : "Landscape and portrait each have their own grid — switch above the deck to edit the other."}
                              </p>
                            </section>
                            <Plate depth="well" pad="md" className="flex flex-col gap-2">
                              <Label className="text-signal">Tip</Label>
                              <p className="m-0 text-[13px] leading-relaxed text-ink-2">
                                Click an empty spot to add a key, then pick a ready-made one from its Gallery — or give it something to do: a tap, a double tap, a long press or a hold.
                              </p>
                            </Plate>
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  </aside>
                </div>
              ) : null}

              {tab === "automations" ? (
                automationsDraft === null ? (
                  <EmptyState
                    scene="scan"
                    title={automationsStatus === "failed" ? "Automations could not be read." : "Reading this desktop’s automations…"}
                    action={automationsStatus === "failed" ? <Key onClick={reloadAutomations}>Try again</Key> : null}
                  />
                ) : (
                  <AutomationWorkbench
                    document={automationsDraft}
                    selectedId={selectedAutomationId}
                    onSelect={selectAutomation}
                    onChange={editAutomations}
                    saving={automationsSaving}
                    dirty={automationsDirty}
                    onRetrySave={saveAutomations}
                    events={pluginEvents}
                    variables={variables}
                    catalogue={catalogue}
                    loadChoices={loadChoices}
                    pages={pages}
                    onDryRun={dryRunAutomation}
                    lastRun={lastRun}
                  />
                )
              ) : null}

              {tab === "data" ? <SourcesPanel desktopId={desktop.id} /> : null}
              {tab === "plugins" ? <PluginsPanel desktopId={desktop.id} onChanged={reloadCatalogue} /> : null}

              {tab === "devices" ? (
                <div className="grid items-start gap-5 lg:grid-cols-2">
                  <div className="flex flex-col gap-5">
                    <PairingCard
                      pairing={pairing}
                      host={status?.host ?? null}
                      port={status?.port ?? null}
                      onBegin={beginPairing}
                      onCancel={cancelPairing}
                    />
                    <DevicesCard devices={devices} onRevoke={revokeDevice} />
                  </div>
                  <div className="flex flex-col gap-5">
                    <AgentCard status={status} />
                    {/* Already connected, so adding a second computer is the rare case:
                        kept reachable, not shown as a card of its own. */}
                    <details className="group px-0.5">
                      <summary className="cursor-pointer font-mono text-[11px] tracking-[0.12em] text-muted select-none hover:text-ink">
                        ADD ANOTHER COMPUTER
                      </summary>
                      <div className="mt-3">
                        <AddDesktop pair={pair} />
                      </div>
                    </details>
                    {desktop.via === "relay" ? (
                      <Notice tone="warn">
                        You are reaching this desktop from another device, so opening a tablet pairing window or installing a plugin waits for someone to approve it in the desktop app.
                      </Notice>
                    ) : null}
                  </div>
                </div>
              ) : null}

              {tab === "system" ? (
                <div className="flex flex-col gap-6">
                  <LocaleCard desktopId={desktop.id} />
                  <RawConfigCard config={config} />
                </div>
              ) : null}
            </motion.div>
          </AnimatePresence>
        ) : desktop && documentStatus === "failed" ? (
          // The deck is not drawn at all: a document that could not be read is not
          // an empty document, and an editor over it invites edits to a machine
          // that is not there. The reason is already shown once, above.
          <EmptyState
            scene="scan"
            title={`${desktop.name} isn’t answering.`}
            body="Its deck could not be read, and nothing here has changed. On that computer, start the FlexDeck app and switch web access on, then try again."
            action={<Key tone="signal" onClick={onRetry}>Try again</Key>}
          />
        ) : desktop && documentStatus === "empty" ? (
          // The machine ANSWERED and has nothing saved: distinct from both above.
          <EmptyState
            scene="deck"
            title={`${desktop.name} has no deck yet.`}
            body="It answered, but has nothing saved. If it was just set up, give it a moment."
            action={<Key onClick={onRetry}>Try again</Key>}
          />
        ) : desktop ? (
          <EmptyState scene="scan" title="Reading this desktop’s deck…" />
        ) : desktops === null ? (
          <EmptyState scene="scan" title="Looking for your desktops…" />
        ) : (
          <EmptyState
            scene="deck"
            title="Connect a desktop."
            body="Open this page on the computer running the FlexDeck app and it connects by itself — no sign-in. From another device, use the code the app shows."
            action={
              <div className="w-full max-w-md">
                <AddDesktop pair={pair} centered />
              </div>
            }
          />
        )}
      </main>
    </div>
  );
}

/** A state with nothing to edit: a figure, a sentence, and what to do about it. */
function EmptyState({ scene, title, body, action }: { scene: "deck" | "scan" | "rings"; title: string; body?: string; action?: ReactNode }) {
  const draw = useCallback(
    (t: number) => {
      if (scene === "scan") return scan(t, 56, 9);
      if (scene === "rings") return rings(t, 56, 13);
      return render({ cols: 84, rows: 26, floor: false, yaw: 0.65 + Math.sin(t * 0.3) * 0.5, elev: 0.85, t, lit: Array.from({ length: 15 }, (_, k) => (k === Math.floor(t * 2.5) % 15 ? 1 : 0)), reveal: Math.min(1, t / 1.6) });
    },
    [scene],
  );
  return (
    <motion.section
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, ease: ease.outExpo }}
      className="mx-auto flex w-full max-w-3xl flex-col items-center gap-5 rounded-plate border border-line bg-plate px-6 py-10 text-center"
    >
      <AsciiScene draw={draw} fps={24} size={scene === "deck" ? 8 : 10} tone="text-muted" />
      <h1 className="m-0 text-3xl font-semibold tracking-[-0.01em]">{title}</h1>
      {body ? <p className="m-0 max-w-xl text-[15px] leading-relaxed text-muted">{body}</p> : null}
      {action}
    </motion.section>
  );
}

/// A pairing that reports rather than pretending: only reachable if the page is
/// composed wrongly, and a silent success would look like a desktop that never appears.
async function unsupportedPair(): Promise<string> {
  throw new Error("this view was rendered without its controller");
}
