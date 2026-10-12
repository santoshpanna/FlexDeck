import type { Orientation } from "@flexdeck/grid";
import { Children, cloneElement, useCallback, useEffect, useRef, useState, type ReactElement } from "react";

import { useDesktops } from "../../../hooks/useDesktops/useDesktops.hook.ts";
import { useLive } from "../../../hooks/useLive/useLive.hook.ts";
import type { AutomationDocument, Run, WireConfig } from "../../../services/api/api.ts";
import type { LiveMessage } from "../../../services/api/live.ts";
import { describeError } from "../../../services/api/errors.ts";
import { useActionCatalogue } from "../hooks/useActionCatalogue/useActionCatalogue.hook.ts";
import { useArgumentChoices } from "../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import { useAgentFacts } from "../hooks/useAgentFacts/useAgentFacts.hook.ts";
import { hasOwnPortraitGrid } from "../lib/orientation.lib.ts";
import { lightActiveKeys } from "../lib/editing.lib.ts";
import { useAutomations } from "../hooks/useAutomations/useAutomations.hook.ts";
import { useConfigDocument } from "../hooks/useConfigDocument/useConfigDocument.hook.ts";
import { useDeckPage } from "../hooks/useDeckPage/useDeckPage.hook.ts";
import { useEditor } from "../hooks/useEditor/useEditor.hook.ts";
import { useKeyboardEdit } from "../hooks/useKeyboardEdit/useKeyboardEdit.hook.ts";
import { usePageSelection } from "../hooks/usePageSelection/usePageSelection.hook.ts";
import { usePairing } from "../hooks/usePairing/usePairing.hook.ts";
import { gridSpecOf, pageById, pagesOf, profileOf } from "../lib/config.lib.ts";
import type { ConfigTab } from "../lib/tabs.lib.ts";
import type { WidgetState } from "../lib/state.lib.ts";
import type { ConfigViewProps } from "../view/config.view.tsx";

export interface ConfigControllerProps {
  /** The single view element this controller injects into. */
  children: ReactElement<Partial<ConfigViewProps>>;
}

/**
 * The configurator's controller.
 *
 * It owns every piece of business logic the page has — which desktop is being
 * edited, reading and writing the document, pairing, the editor's state — and
 * returns the view it was handed, cloned with the result. The view renders; this
 * decides.
 *
 * Note the single `error`, rather than one per hook: two error slots would give
 * the page two ways to disagree about what went wrong.
 */
export function ConfigController({ children }: ConfigControllerProps): ReactElement {
  const {
    desktops,
    selected,
    select,
    refresh: refreshDesktops,
    pair,
    forgetDesktop,
    error: desktopError,
    reconnects,
  } = useDesktops();

  // A hook cannot be called conditionally, so the desktop id is passed through
  // as `null` until one is chosen and each hook treats that as "nothing to do".
  const desktopId = selected?.id ?? null;

  const document = useConfigDocument(desktopId);
  const facts = useAgentFacts(desktopId);
  const catalogue = useActionCatalogue(desktopId);
  const loadChoices = useArgumentChoices(desktopId);
  const pairing = usePairing(desktopId, facts.devices, facts.reload);
  const automations = useAutomations(desktopId);

  const [error, setError] = useState<string | null>(null);

  /// Whether the widget panel is open. Deliberately NOT the selection: a press
  /// selects (so a drag never starts on an undescribed widget) but must not open
  /// a panel, because reflowing the deck mid-drag would move the widget out from
  /// under the pointer.
  const [drawerOpen, setDrawerOpen] = useState(false);

  /// Which section of the configurator is showing. Held here rather than in the
  /// view so the one component that owns the page's state owns all of it.
  const [tab, setTab] = useState<ConfigTab>("deck");

  /// How the selected widget is drawn in the preview while authoring. NOT part of
  /// the document — a lens on the preview, not a property of the widget.
  const [previewState, setPreviewState] = useState<WidgetState>("idle");
  /// Which layout is being edited. Each orientation has its own grid and its
  /// own places, so this is not a preview lens: it decides what an edit changes.
  const [orientation, setOrientation] = useState<Orientation>("landscape");

  /// Local edit of the automations document. The hook holds the agent's
  /// truth; the controller holds the editor's draft. Edits happen on the draft
  /// and flush through `act(automations.put)` with a short debounce so a
  /// flurry of edits becomes one save.
  const [automationsDraft, setAutomationsDraft] = useState<AutomationDocument | null>(null);
  const [automationsSaving, setAutomationsSaving] = useState<"idle" | "saving" | "saved" | "error">(
    "idle",
  );
  const [automationsDirty, setAutomationsDirty] = useState(false);
  /// Which automation is being edited. `null` means the list view.
  const [selectedAutomationId, setSelectedAutomationId] = useState<string | null>(null);

  /// Mirror the hook's loaded document into the local draft on first load
  /// and on desktop switch. A re-read after a save should NOT clobber a draft
  /// the user is still typing into — the saved `rev` propagates through the
  /// hook, so the draft is updated only when the hook's content differs.
  useEffect(() => {
    if (automations.document === null) {
      setAutomationsDraft(null);
      setAutomationsDirty(false);
      return;
    }
    if (automationsDirty) return;
    setAutomationsDraft(automations.document);
  }, [automations.document, automationsDirty]);

  const refresh = useCallback(async () => {
    try {
      await Promise.all([
        facts.reload(),
        catalogue.reload(),
        document.reload(),
        automations.reload(),
      ]);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [facts.reload, catalogue.reload, document.reload, automations.reload]);

  // Also on a reconnect: what failed against a refused token reads again with
  // the new one, rather than waiting for "Try again".
  useEffect(() => {
    void refresh();
  }, [refresh, reconnects]);

  /// What an edit to the deck can change: the deck itself and the revision the
  /// Agent card shows. Not the catalogue or the automations, so those are not
  /// re-read on every move.
  const refreshDeck = useCallback(async () => {
    await Promise.all([facts.reload(), document.reload()]);
  }, [facts.reload, document.reload]);

  /// Run a piece of work, reporting failure in the one place the view reads.
  const act = useCallback(async (work: () => Promise<void>) => {
    try {
      await work();
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, []);

  /// Commit a document, then re-read everything.
  ///
  /// The re-read is not only for the editor: an edit changes the agent's
  /// revision, and the Agent card shows it, so a save that did not refresh would
  /// leave the page reporting a revision the document no longer has.
  const write = useCallback(
    (next: WireConfig) => {
      void act(async () => {
        await document.put(next);
        await refreshDeck();
      });
    },
    [act, document.put, refreshDeck],
  );

  // The document moved somewhere else — another browser, a tablet, an undo
  // elsewhere — or the socket reopened after a gap: read it again. The agent
  // says so the moment it happens, so two editors never drift apart.
  const configRev = document.config?.rev;
  const reloadDocument = document.reload;
  const hearDeck = useCallback(
    (message: LiveMessage) => {
      if (message.kind === "open" || (message.kind === "revision" && message.rev !== Number(configRev ?? -1))) {
        void reloadDocument();
      }
    },
    [configRev, reloadDocument],
  );
  useLive(desktopId, hearDeck);

  // Scene keys made before actions could say when they are active get their
  // light once the catalogue can tell — once per desktop, so an undo of it
  // stays undone.
  const litFor = useRef<string | null>(null);
  useEffect(() => {
    if (!desktopId || !document.config || catalogue.catalogue.length === 0 || litFor.current === desktopId) return;
    litFor.current = desktopId;
    const next = lightActiveKeys(document.config, catalogue.catalogue);
    if (next !== document.config) write(next);
  }, [desktopId, document.config, catalogue.catalogue, write]);

  const profile = profileOf(document.config);
  const { currentPageId, selectPage } = usePageSelection(profile);
  // The tablets and the editor show the same page: one turned there is
  // followed here, one chosen here is sent there.
  const { choosePage } = useDeckPage({
    desktopId,
    pageIds: pagesOf(profile).map((candidate) => candidate.id),
    selectPage,
  });

  const editor = useEditor({
    config: document.config,
    catalogue: catalogue.catalogue,
    write,
    currentPageId,
    onPageCreated: choosePage,
    // A widget just added is the one about to be set up: open it.
    onWidgetCreated: () => {
      setPreviewState("idle");
      setDrawerOpen(true);
    },
  });

  /// The document's own history. Held here rather than in the view because the
  /// toolbar that used to show them is gone — they are keyboard-only now.
  const undo = useCallback(() => {
    void act(async () => {
      await document.undo();
      await refreshDeck();
    });
  }, [act, document.undo, refreshDeck]);

  const redo = useCallback(() => {
    void act(async () => {
      await document.redo();
      await refreshDeck();
    });
  }, [act, document.redo, refreshDeck]);

  useKeyboardEdit({
    selectedId: editor.selectedId,
    move: editor.move,
    remove: editor.remove,
    deselect: () => editor.select(null),
    undo,
    redo,
  });

  // A widget that is gone — deleted, or left behind on a page the user flipped
  // away from — takes its panel with it rather than leaving it describing
  // something that is no longer on screen.
  useEffect(() => {
    if (!editor.selectedId) {
      setDrawerOpen(false);
    }
  }, [editor.selectedId]);

  const pages = pagesOf(profile);
  const page = pageById(profile, currentPageId);

  /// A change to the draft: snapshot into the local state, mark dirty. The
  /// save button is the only place that actually talks to the agent, so a
  /// typo on the dashboard never costs a write.
  const editAutomations = useCallback((next: AutomationDocument) => {
    setAutomationsDraft(next);
    setAutomationsDirty(true);
    setAutomationsSaving("idle");
  }, []);

  const saveAutomations = useCallback(async () => {
    if (automationsDraft === null) return;
    await act(async () => {
      setAutomationsSaving("saving");
      try {
        await automations.put(automationsDraft);
        setAutomationsDirty(false);
        setAutomationsSaving("saved");
      } catch (cause) {
        setAutomationsSaving("error");
        throw cause;
      }
    });
  }, [act, automations, automationsDraft]);

  /// The editor wants the trace of a previous dry-run to display. It is held
  /// here, not in the editor, because the trace is part of the page state
  /// (the user navigates away and back; the trace persists for the run they
  /// last looked at).
  const [lastRun, setLastRun] = useState<Run | null>(null);

  const dryRunAutomation = useCallback(
    async (automationId: string) => {
      await act(async () => {
        const run = await automations.dryRun(automationId);
        setLastRun(run);
      });
    },
    [act, automations],
  );

  /// Which automation the editor shows. Choosing another drops the last dry-run,
  /// which belonged to the one being left.
  const selectAutomation = useCallback((automationId: string | null) => {
    setSelectedAutomationId(automationId);
    setLastRun(null);
  }, []);

  /// Autosave: a draft that has stopped changing for a moment is written. The
  /// agent keeps revisions, so this is never the only copy; the readout says
  /// when it is saving, saved, or refused.
  useEffect(() => {
    if (!automationsDirty || automationsDraft === null) return;
    const timer = setTimeout(() => void saveAutomations(), 900);
    return () => clearTimeout(timer);
  }, [automationsDirty, automationsDraft, saveAutomations]);

  /// The selected automation, looked up in the draft (or the loaded document
  /// if the draft is empty). `null` is the list view.
  const selectedAutomation = (() => {
    if (selectedAutomationId === null) return null;
    const source = automationsDraft ?? automations.document;
    if (source === null) return null;
    return (source.automations ?? []).find((a) => a.id === selectedAutomationId) ?? null;
  })();

  /// Checked against `ConfigViewProps`, so a field the view starts needing is a
  /// compile error here rather than an `undefined` at runtime.
  const props: ConfigViewProps = {
    desktops,
    desktop: selected,
    selectDesktop: select,
    pair,

    desktopsError: desktopError,
    onRetryDesktops: () => void refreshDesktops(),
    error,
    onRetry: () => void refresh(),
    onForgetDesktop: () => {
      if (desktopId) forgetDesktop(desktopId);
    },

    status: facts.status,
    devices: facts.devices,
    revokeDevice: (deviceId: string) => {
      void act(async () => {
        await facts.revoke(deviceId);
        await refresh();
      });
    },
    revokeAllDevices: () => {
      void act(async () => {
        await facts.revokeAll();
        await refresh();
      });
    },

    pairing: pairing.pairing,
    beginPairing: () => {
      void act(pairing.begin);
    },
    cancelPairing: () => {
      void act(pairing.cancel);
    },

    catalogue: catalogue.catalogue,
    pluginEvents: catalogue.events,
    variables: catalogue.variables,
    reloadCatalogue: () => void catalogue.reload(),
    config: document.config,
    documentStatus: document.status,
    spec: gridSpecOf(profile),
    profile,
    orientation,
    setOrientation,
    portraitOwnGrid: hasOwnPortraitGrid(profile),
    page,
    caption: page?.name ?? profile?.name ?? "no page",

    tab,
    selectTab: setTab,

    // Reactive layer editor — kept here so the controller's debounced-save
    // logic owns when the agent is hit, not the editor.
    automationsDraft,
    automationsStatus: automations.status,
    automationsSaving,
    automationsDirty,
    selectedAutomation,
    editAutomations,
    saveAutomations,
    reloadAutomations: () => {
      void act(async () => {
        await automations.reload();
        setAutomationsDirty(false);
        setAutomationsSaving("idle");
      });
    },
    dryRunAutomation,
    lastRun,
    selectedAutomationId,
    selectAutomation,

    pages,
    currentPageId,
    selectPage: choosePage,
    addPage: editor.addPage,
    removePage: editor.removePage,
    renamePage: editor.renamePage,
    movePage: editor.movePage,
    duplicatePage: editor.duplicatePage,
    setGrid: editor.setGrid,
    setPortraitGrid: editor.setPortraitGrid,
    clearPortraitGrid: editor.clearPortraitGrid,
    movePortrait: editor.movePortrait,
    resizePortrait: editor.resizePortrait,
    addAtPortrait: editor.addAtPortrait,
    unpinPortrait: editor.unpinPortrait,

    previewState,
    setPreviewState,
    setIcon: editor.setIcon,
    setBinding: editor.setBinding,
    editSelected: editor.editSelected,
    editPage: editor.editPage,
    duplicate: editor.duplicate,
    addAt: editor.addAt,
    moveTo: editor.moveTo,
    resize: editor.resize,

    selectedId: editor.selectedId,
    select: (widgetId) => {
      editor.select(widgetId);
      setPreviewState("idle");
    },
    drawerOpen,
    openWidget: (widgetId) => {
      editor.select(widgetId);
      setPreviewState("idle");
      setDrawerOpen(true);
    },
    closeWidget: () => {
      setDrawerOpen(false);
    },
    labelDraft: editor.labelDraft,
    setLabelDraft: editor.setLabelDraft,
    commitLabel: editor.commitLabel,
    actionDraft: editor.actionDraft,
    setActionDraft: editor.setActionDraft,
    selectedAction: editor.selectedAction,
    refusal: editor.refusal,
    dismissRefusal: editor.dismissRefusal,

    move: editor.move,
    removeSelected: editor.remove,
    removeWidget: editor.removeWidget,
    saveAction: editor.saveAction,
    commitActionArgs: editor.commitActionArgs,
    loadChoices,
  };

  // `Children.only` rather than passing `children` straight through: the page
  // must compose exactly one view, and this says so with an error that names the
  // mistake, instead of `cloneElement`'s opaque complaint about a non-element.
  return cloneElement(Children.only(children), props);
}
