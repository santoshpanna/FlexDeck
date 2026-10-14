import { useEffect, useState } from "react";

import type { ActionDefinition, WireAction, WireConfig } from "../../../../services/api/api.ts";
import { gridSpecOf, pagesOf, profileOf } from "../../lib/config.lib.ts";
import {
  describeGridProblem,
  gridProblem,
  setGridSpec,
  type GridSizePatch,
} from "../../lib/grid.lib.ts";
import {
  addPage,
  addWidgetAt,
  findPage,
  addWidget,
  duplicatePage,
  duplicateWidget,
  findWidget,
  movePage,
  moveWidget,
  nextPageId,
  pressActionKey,
  pressActionOf,
  removePage,
  removeWidget,
  renamePage,
  resizeWidget,
  setWidgetAction,
  setWidgetIcon,
  setActiveLight,
  setWidgetBinding,
  stateVariableOf,
  setWidgetLabel,
  type Located,
} from "../../lib/editing.lib.ts";
import {
  clearPortraitGrid as clearPortraitGridEdit,
  layoutFor,
  pinPortrait,
  setPortraitGrid as setPortraitGridEdit,
  unpinPortrait as unpinPortraitEdit,
} from "../../lib/orientation.lib.ts";

export interface EditorValue {
  selectedId: string | null;
  select: (widgetId: string | null) => void;
  selected: Located | null;
  /** The selected widget's label while it is being typed. */
  labelDraft: string;
  setLabelDraft: (value: string) => void;
  commitLabel: () => void;
  /** The selected widget's action arguments while they are being typed. */
  actionDraft: Record<string, string>;
  setActionDraft: (draft: Record<string, string>) => void;
  selectedAction: WireAction | null;
  /** Why the last edit was refused, or `null`. */
  refusal: string | null;
  /** Put the refusal away before it goes on its own. */
  dismissRefusal: () => void;
  move: (dc: number, dr: number) => void;
  remove: () => void;
  /** Remove one widget by id — the hover action, which may not be the selection. */
  removeWidget: (widgetId: string) => void;
  addPage: () => void;
  removePage: (pageId: string) => void;
  renamePage: (pageId: string, name: string) => void;
  movePage: (pageId: string, delta: number) => void;
  duplicatePage: (pageId: string) => void;
  /** Set or clear the selected widget's icon. */
  setIcon: (icon: string | null) => void;
  /** Bind the selected widget to live values, or unbind it. */
  setBinding: (binding: { stateVariable: string; labelTemplate: string }) => void;
  /** Apply a pure edit to the selected widget — its slider, its hold. */
  editSelected: (transform: (config: WireConfig, widgetId: string) => WireConfig) => void;
  /** Apply a pure edit to the page on screen — its background. */
  editPage: (transform: (config: WireConfig, pageId: string) => WireConfig) => void;
  /** Copy the selected widget into its page's first free cell. */
  duplicate: () => void;
  /** Add a widget at an exact cell — the "+" on an empty cell. */
  addAt: (pageId: string, col: number, row: number) => void;
  /** Move the widget to an exact cell — a drag. */
  moveTo: (widgetId: string, col: number, row: number) => void;
  /** Set the widget's span — a resize drag. */
  resize: (widgetId: string, w: number, h: number) => void;
  /** Change the grid's shape: columns, rows and gaps. */
  setGrid: (patch: GridSizePatch) => void;
  /** Pin a widget's portrait place — a drag in portrait. */
  movePortrait: (widgetId: string, col: number, row: number) => void;
  /** Pin a widget's portrait span — a resize in portrait. */
  resizePortrait: (widgetId: string, w: number, h: number) => void;
  /** Add a widget at a portrait cell: it is pinned there, and given a free landscape cell. */
  addAtPortrait: (pageId: string, col: number, row: number) => void;
  /** Let portrait assume the widget's place again. */
  unpinPortrait: (widgetId: string) => void;
  /** Give portrait its own grid, or change it. */
  setPortraitGrid: (patch: GridSizePatch) => void;
  /** Go back to the landscape grid turned on its side. */
  clearPortraitGrid: () => void;
  saveAction: (name: string) => void;
  commitActionArgs: (draft?: Record<string, string>) => void;
}

export interface EditorOptions {
  config: WireConfig | null;
  catalogue: ActionDefinition[];
  /** The controller's way of committing a document; this hook never talks to the network. */
  write: (next: WireConfig) => void;
  /** The page being edited. A change drops the widget selection with it. */
  currentPageId: string | null;
  /** Called with the new id after a page is created, so the caller can select it. */
  onPageCreated?: (pageId: string) => void;
  /** Called with the new id after a widget is added or duplicated, so the caller can open it. */
  onWidgetCreated?: (widgetId: string) => void;
  /** An edit came to nothing: what is saved is what the field now says. */
  onUnchanged?: () => void;
}

/** How long a refused edit's explanation stays up. */
const REFUSAL_MS = 4000;

/**
 * Editing state: what is selected, what is being typed, and the edits themselves.
 *
 * All of the decisions live in `lib/editing.lib.ts` as pure functions; this hook
 * only holds the state and calls them.
 */
export function useEditor({
  config,
  catalogue,
  write,
  currentPageId,
  onPageCreated,
  onWidgetCreated,
  onUnchanged,
}: EditorOptions): EditorValue {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [labelDraft, setLabelDraft] = useState("");
  const [actionDraft, setActionDraft] = useState<Record<string, string>>({});
  const [refusal, setRefusal] = useState<string | null>(null);

  // A refusal explains the edit just tried; it is not a standing state, so it
  // goes on its own rather than waiting for some later edit to clear it.
  useEffect(() => {
    if (refusal === null) {
      return;
    }
    const timer = setTimeout(() => setRefusal(null), REFUSAL_MS);
    return () => clearTimeout(timer);
  }, [refusal]);

  const selected = config && selectedId ? findWidget(config, selectedId) : null;
  const selectedAction = selected ? pressActionOf(selected.widget) : null;

  // Keyed on the selection alone. Re-seeding these whenever the document changes
  // would throw away whatever someone is halfway through typing, because every
  // save produces a new document. Also once more when the selected widget first
  // appears: a widget selected as it is added is not in the document until the
  // save comes back.
  const selectedFound = selected !== null;
  useEffect(() => {
    setLabelDraft(selected?.widget.props?.label ?? "");
    setActionDraft(selected ? (pressActionOf(selected.widget)?.args ?? {}) : {});
  }, [selectedId, selectedFound]);

  // A selection belongs to the page it is on, so switching page — or losing the
  // page it was on — drops it, rather than leaving the sidebar editing a widget
  // that is not on screen.
  useEffect(() => {
    setSelectedId(null);
  }, [currentPageId]);

  /// Apply an edit.
  ///
  /// The editing model hands back the *same object* when it refuses a change, so
  /// identity is how a refusal is told apart from a successful edit — which is
  /// why the UI can say "another widget is in the way" rather than appearing to
  /// do nothing at all.
  const applyEdit = (next: WireConfig, why: string) => {
    if (next === config) {
      setRefusal(why);
      return;
    }
    setRefusal(null);
    write(next);
  };

  const pages = pagesOf(profileOf(config));

  /// Same refusal contract as `applyEdit`, plus the page id a create produced, so
  /// the caller can select it — the one edit whose result is a new place to be.
  const applyPageEdit = (next: WireConfig, why: string, createdPageId?: string) => {
    if (next === config) {
      setRefusal(why);
      return;
    }
    setRefusal(null);
    write(next);
    if (createdPageId) {
      onPageCreated?.(createdPageId);
    }
  };

  /// Same refusal contract as `applyEdit`, then selects the widget the edit put
  /// on `pageId`, so a new widget is the one being edited.
  const applyWidgetEdit = (next: WireConfig, why: string, pageId: string) => {
    if (!config) {
      return;
    }
    const before = new Set((findPage(config, pageId)?.widgets ?? []).map((widget) => widget.id));
    applyEdit(next, why);
    if (next === config) {
      return;
    }
    const fresh = (findPage(next, pageId)?.widgets ?? []).find((widget) => !before.has(widget.id));
    if (fresh) {
      setSelectedId(fresh.id);
      onWidgetCreated?.(fresh.id);
    }
  };

  const addPageAt = () => {
    if (!config) {
      return;
    }
    const id = nextPageId(pages.map((page) => page.id));
    applyPageEdit(addPage(config, id, `Page ${pages.length + 1}`), "cannot add a page here", id);
  };

  const removePageAt = (pageId: string) => {
    if (!config) {
      return;
    }
    applyPageEdit(removePage(config, pageId), "a deck needs at least one page");
  };

  const renamePageAt = (pageId: string, name: string) => {
    if (!config) {
      return;
    }
    // `renamePage` returns the SAME object when nothing changed, so an untouched
    // field spends no revision and does not clear a standing refusal.
    const next = renamePage(config, pageId, name);
    if (next === config) {
      return;
    }
    setRefusal(null);
    write(next);
  };

  const movePageAt = (pageId: string, delta: number) => {
    if (!config) {
      return;
    }
    applyPageEdit(movePage(config, pageId, delta), "that page cannot move any further");
  };

  const duplicatePageAt = (pageId: string) => {
    if (!config) {
      return;
    }
    const id = nextPageId(pages.map((page) => page.id));
    applyPageEdit(duplicatePage(config, pageId, id), "that page could not be duplicated", id);
  };

  const setBinding = (binding: { stateVariable: string; labelTemplate: string }) => {
    if (!config || !selectedId || !selected) {
      return;
    }
    const props = selected.widget.props ?? {};
    // An unchanged binding must not spend a revision.
    if (
      stateVariableOf(selected.widget) === binding.stateVariable.trim() &&
      (props.labelTemplate ?? "") === binding.labelTemplate.trim()
    ) {
      onUnchanged?.();
      return;
    }
    setRefusal(null);
    write(setWidgetBinding(config, selectedId, binding));
  };

  /// Apply a pure edit to the selected widget — a slider's target, a hold's
  /// steps. One command for the edits whose logic lives in a lib, so each one
  /// is not another prop threaded through the controller and the view.
  const editSelected = (transform: (config: WireConfig, widgetId: string) => WireConfig) => {
    if (!config || !selectedId) {
      return;
    }
    const next = transform(config, selectedId);
    // Unchanged is not a refusal here: re-choosing what is already chosen.
    if (next === config) {
      onUnchanged?.();
      return;
    }
    setRefusal(null);
    write(next);
  };

  const editPage = (transform: (config: WireConfig, pageId: string) => WireConfig) => {
    if (!config || !currentPageId) {
      return;
    }
    const next = transform(config, currentPageId);
    if (next === config) {
      onUnchanged?.();
      return;
    }
    setRefusal(null);
    write(next);
  };

  const setIcon = (icon: string | null) => {
    if (!config || !selectedId || !selected) {
      return;
    }
    // An unchanged icon must not spend a revision.
    if ((selected.widget.props?.icon ?? "") === (icon ?? "")) {
      onUnchanged?.();
      return;
    }
    const next = setWidgetIcon(config, selectedId, icon);
    if (next === config) {
      return;
    }
    setRefusal(null);
    write(next);
  };

  const duplicate = () => {
    if (!config || !selectedId) {
      return;
    }
    const pageId = findWidget(config, selectedId)?.page.id;
    const next = duplicateWidget(config, selectedId);
    if (pageId) {
      applyWidgetEdit(next, "there is no room left on this page", pageId);
    } else {
      applyEdit(next, "there is no room left on this page");
    }
  };

  const addAt = (pageId: string, col: number, row: number) => {
    if (!config) {
      return;
    }
    applyWidgetEdit(
      addWidgetAt(config, pageId, col, row, {
        type: "WIDGET_TYPE_BUTTON",
        props: { label: "New button" },
      }),
      "something is already in that cell",
      pageId,
    );
  };

  const moveTo = (widgetId: string, col: number, row: number) => {
    if (!config) {
      return;
    }
    applyEdit(
      moveWidget(config, widgetId, col, row),
      "another widget is in the way, or that is off the grid",
    );
  };

  const resize = (widgetId: string, w: number, h: number) => {
    if (!config) {
      return;
    }
    applyEdit(resizeWidget(config, widgetId, w, h), "that size does not fit");
  };

  /** The widget's span as portrait lays it out now, pinned or assumed. */
  const portraitSpan = (widgetId: string) => {
    if (!config || !currentPageId) return null;
    const profile = profileOf(config);
    const page = profile?.pages?.find((candidate) => candidate.id === currentPageId) ?? null;
    return layoutFor(profile, page, "portrait").widgets.find((widget) => widget.id === widgetId) ?? null;
  };

  const movePortrait = (widgetId: string, col: number, row: number) => {
    const span = portraitSpan(widgetId);
    if (!config || !currentPageId || !span) return;
    applyEdit(pinPortrait(config, currentPageId, widgetId, { col, row, w: span.w, h: span.h }), "another widget is in the way, or that is off the grid");
  };

  const resizePortrait = (widgetId: string, w: number, h: number) => {
    const span = portraitSpan(widgetId);
    if (!config || !currentPageId || !span) return;
    applyEdit(pinPortrait(config, currentPageId, widgetId, { col: span.col, row: span.row, w, h }), "that size does not fit");
  };

  const addAtPortrait = (pageId: string, col: number, row: number) => {
    if (!config) return;
    const before = new Set((findPage(config, pageId)?.widgets ?? []).map((widget) => widget.id));
    const added = addWidget(config, { type: "WIDGET_TYPE_BUTTON", props: { label: "New button" } }, pageId);
    const fresh = (findPage(added, pageId)?.widgets ?? []).find((widget) => !before.has(widget.id));
    if (!fresh) {
      applyEdit(config, "the landscape grid is full — make room there first");
      return;
    }
    const pinned = pinPortrait(added, pageId, fresh.id, { col, row, w: 1, h: 1 });
    applyWidgetEdit(pinned === added ? config : pinned, "something is already in that cell", pageId);
  };

  const unpinPortrait = (widgetId: string) => {
    if (!config) return;
    applyEdit(unpinPortraitEdit(config, widgetId), "it is already placed for you");
  };

  const setPortraitGrid = (patch: GridSizePatch) => {
    if (!config) return;
    const next = setPortraitGridEdit(config, patch);
    if (next !== config) {
      setRefusal(null);
      write(next);
    }
  };

  const clearPortraitGrid = () => {
    if (!config) return;
    const next = clearPortraitGridEdit(config);
    if (next !== config) write(next);
  };

  const setGrid = (patch: GridSizePatch) => {
    if (!config) {
      return;
    }
    const profile = profileOf(config);
    if (!profile) {
      return;
    }

    const next = setGridSpec(config, patch);
    if (next === config) {
      // An unchanged value is not an error, so only explain a real refusal.
      const problem = gridProblem({ ...gridSpecOf(profile), ...patch }, pagesOf(profile));
      if (problem) {
        setRefusal(describeGridProblem(problem));
      }
      return;
    }

    setRefusal(null);
    write(next);
  };

  const move = (dc: number, dr: number) => {
    if (!config || !selectedId || !selected) {
      return;
    }
    applyEdit(
      moveWidget(config, selectedId, (selected.widget.col ?? 0) + dc, (selected.widget.row ?? 0) + dr),
      "another widget is in the way, or that is off the grid",
    );
  };

  const remove = () => {
    if (!config || !selectedId || !selected) {
      return;
    }
    applyEdit(removeWidget(config, selectedId), "");
    setSelectedId(null);
  };

  /// Remove a widget named directly rather than by selection.
  ///
  /// The hover actions sit on whatever the pointer is over, which is not
  /// necessarily the selection, so this cannot go through `remove`.
  const removeWidgetById = (widgetId: string) => {
    if (!config) {
      return;
    }
    applyEdit(removeWidget(config, widgetId), "");
    if (selectedId === widgetId) {
      setSelectedId(null);
    }
  };

  /// Commit the label on blur or Enter rather than on every keystroke: each
  /// commit is a new revision, and one per character would bury the undo history.
  const commitLabel = () => {
    if (!config || !selectedId || !selected) {
      return;
    }
    if ((selected.widget.props?.label ?? "") === labelDraft.trim()) {
      return;
    }
    write(setWidgetLabel(config, selectedId, labelDraft));
  };

  /**
   * Attach, change or clear the selected widget's press action.
   *
   * The argument fields come from the catalogue's own list, so an action the
   * agent learns to do becomes editable here without changing this file.
   *
   * The lookup is by `(plugin, action)` rather than by action alone, because
   * two plugins can publish actions with the same name (e.g. `notify` lives
   * on both `core` and `notifications` historically). Matching only by
   * `action` would silently bind a button to the wrong plugin.
   */
  const saveAction = (key: string, draft: Record<string, string> = actionDraft) => {
    if (!config || !selectedId || !selected) {
      return;
    }

    if (!key) {
      write(setActiveLight(setWidgetAction(config, selectedId, null), selectedId, null, {}));
      return;
    }

    const [pluginId, actionName] = key.split(":", 2);
    const definition =
      catalogue.find(
        (entry) => entry.plugin === pluginId && entry.action === actionName,
      ) ?? null;
    if (!definition) {
      return;
    }

    const args: Record<string, string> = {};
    for (const argument of definition.arguments) {
      const value = (draft[argument.name] ?? "").trim();
      if (value) {
        args[argument.name] = value;
      }
    }

    // A scene key lights up while its scene is live: the action says how to tell.
    const next = setWidgetAction(config, selectedId, {
      plugin: definition.plugin,
      action: definition.action,
      args,
    });
    write(setActiveLight(next, selectedId, definition, args));
  };

  /**
   * Re-save the current action with whatever the argument fields now hold —
   * or with `draft`, for a pick that saves at once, before the fields' own
   * state has caught up with it.
   */
  const commitActionArgs = (draft?: Record<string, string>) => {
    // The stored action is two fields; `saveAction` takes the picker's
    // `<plugin>:<action>` key. Passing the bare name saved nothing.
    const key = pressActionKey(selectedAction, catalogue);
    if (key) {
      saveAction(key, draft ?? actionDraft);
    }
  };

  return {
    editPage,
    selectedId,
    select: setSelectedId,
    selected,
    labelDraft,
    setLabelDraft,
    commitLabel,
    actionDraft,
    setActionDraft,
    selectedAction,
    refusal,
    dismissRefusal: () => setRefusal(null),
    move,
    remove,
    removeWidget: removeWidgetById,
    addPage: addPageAt,
    removePage: removePageAt,
    renamePage: renamePageAt,
    movePage: movePageAt,
    duplicatePage: duplicatePageAt,
    setIcon,
    setBinding,
    editSelected,
    duplicate,
    addAt,
    moveTo,
    movePortrait,
    resizePortrait,
    addAtPortrait,
    unpinPortrait,
    setPortraitGrid,
    clearPortraitGrid,
    resize,
    setGrid,
    saveAction,
    commitActionArgs,
  };
}
