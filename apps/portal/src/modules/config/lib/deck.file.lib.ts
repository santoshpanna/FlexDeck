import type { GridSpec } from "@flexdeck/grid";

import type { ActionDefinition, Block, WireConfig, WirePage, WireProfile, WireWidget } from "../../../services/api/api.ts";
import { gridSpecOf } from "./config.lib.ts";
import { limitsFor, nextPageId, nextWidgetId, placementProblem, describeProblem } from "./editing.lib.ts";
import { portraitSpecOf } from "./orientation.lib.ts";
import { widgetConcerns, type Concern } from "./share.lib.ts";

const BUILT_IN = new Set(["core", "deck", "display", "launcher", "media", "notifications", "sensors", "system", "time", "timer", "vars"]);

export type ImportMode = "replace" | "append";

export interface DeckCheck {
  missing: string[];
  problems: string[];
}

export function exportDeck(config: WireConfig): string {
  const rest = { ...config };
  delete rest.rev;
  return JSON.stringify(rest);
}

export function parseDeck(text: string, current: WireConfig): WireConfig {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("This file is not a FlexDeck deck: it is not JSON.");
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("This file is not a FlexDeck deck.");
  const deck = parsed as WireConfig;
  if (!Array.isArray(deck.profiles)) throw new Error("This file is not a FlexDeck deck: it has no profiles.");
  if (typeof deck.schema !== "number") throw new Error("This file is not a FlexDeck deck: it has no schema.");
  if (current.schema !== undefined && deck.schema > current.schema) {
    throw new Error("This deck was saved by a newer FlexDeck. Update this desktop first.");
  }
  return { ...deck, rev: current.rev };
}

export function deckConcerns(deck: WireConfig): Concern[] {
  return (deck.profiles ?? [])
    .flatMap((profile) =>
      (profile.pages ?? []).flatMap((page) =>
        (page.widgets ?? []).flatMap((widget) => widgetConcerns(widget, `${page.name || page.id} › ${widget.props?.label || widget.id}`)),
      ),
    )
    .sort((a, b) => Number(b.severe) - Number(a.severe));
}

export function pageCount(config: WireConfig): number {
  return (config.profiles ?? []).reduce((sum, profile) => sum + (profile.pages ?? []).length, 0);
}

function actionsOf(blocks: readonly Block[] | undefined, visit: (block: Extract<Block, { block: "doAction" }>) => void): void {
  for (const block of blocks ?? []) {
    if (block.block === "doAction") visit(block);
    else if (block.block === "if") {
      actionsOf(block.then, visit);
      for (const branch of block.elseIf ?? []) actionsOf(branch.blocks, visit);
      actionsOf(block.otherwise, visit);
    } else if (block.block === "repeat") actionsOf(block.body, visit);
  }
}

function eachAction(widget: WireWidget, visit: (block: Extract<Block, { block: "doAction" }>) => void): void {
  for (const flow of widget.flows ?? []) actionsOf(flow.blocks, visit);
}

export function pluginsOf(deck: WireConfig): string[] {
  const found = new Set<string>();
  for (const profile of deck.profiles ?? []) {
    for (const page of profile.pages ?? []) {
      for (const widget of page.widgets ?? []) {
        eachAction(widget, (block) => found.add(block.plugin));
        for (const flow of widget.flows ?? []) if (flow.trigger?.event?.plugin) found.add(flow.trigger.event.plugin);
        for (const use of widget.sources ?? []) if (use.source?.includes("/")) found.add(use.source.split("/")[0]!);
      }
    }
  }
  return [...found].sort();
}

export function missingPlugins(deck: WireConfig, catalogue: readonly ActionDefinition[]): string[] {
  const have = new Set([...BUILT_IN, ...catalogue.map((entry) => entry.plugin)]);
  return pluginsOf(deck).filter((plugin) => !have.has(plugin));
}

const isCount = (value: unknown, min: number) => value === undefined || (Number.isInteger(value) && (value as number) >= min);

function pageProblems(page: WirePage, where: string, spec: GridSpec, portrait: GridSpec, catalogue: readonly ActionDefinition[], pageNames: ReadonlySet<string>): string[] {
  const problems: string[] = [];
  const widgets = page.widgets ?? [];
  if (!Array.isArray(widgets)) return [`${where}: its keys are not a list.`];
  const known = new Set(catalogue.map((entry) => `${entry.plugin}:${entry.action}`));
  const spans = widgets.map((widget) => ({ id: widget.id, col: widget.col ?? 0, row: widget.row ?? 0, w: widget.w ?? 0, h: widget.h ?? 0 }));
  widgets.forEach((widget, index) => {
    const key = `${where}, key “${widget.props?.label || widget.id}”`;
    if (typeof widget.id !== "string" || widget.id === "") {
      problems.push(`${key}: it has no id.`);
      return;
    }
    if (![widget.col, widget.row].every((n) => isCount(n, 0)) || ![widget.w, widget.h].every((n) => isCount(n, 1))) {
      problems.push(`${key}: its position is not whole cells.`);
      return;
    }
    const problem = placementProblem(spec, spans.slice(0, index), spans[index]!, limitsFor(widget.type));
    if (problem) problems.push(`${key}: ${describeProblem(problem)}.`);
    const pin = widget.portrait;
    if (pin) {
      const pinned = { id: widget.id, col: pin.col ?? 0, row: pin.row ?? 0, w: pin.w ?? 0, h: pin.h ?? 0 };
      const portraitProblem = placementProblem(portrait, [], pinned, limitsFor(widget.type));
      if (portraitProblem) problems.push(`${key}, in portrait: ${describeProblem(portraitProblem)}.`);
    }
    eachAction(widget, (block) => {
      if (!BUILT_IN.has(block.plugin) && catalogue.some((entry) => entry.plugin === block.plugin) && !known.has(`${block.plugin}:${block.action}`)) {
        problems.push(`${key}: the ${block.plugin} plugin here has no “${block.action}” action.`);
      }
      if (block.plugin === "deck" && block.action === "go_to_page") {
        const target = (block.args?.page as { text?: string } | undefined)?.text?.trim();
        if (target && !target.includes("{{") && !pageNames.has(target.toLowerCase())) problems.push(`${key}: it opens a page “${target}” the deck does not have.`);
      }
    });
  });
  return problems;
}

function namesOf(profiles: readonly WireProfile[]): Set<string> {
  const names = new Set<string>();
  for (const profile of profiles) {
    for (const page of profile.pages ?? []) {
      names.add(page.id.toLowerCase());
      if (page.name) names.add(page.name.trim().toLowerCase());
    }
  }
  return names;
}

export function checkDeck(deck: WireConfig, current: WireConfig, mode: ImportMode, profileId: string | null, catalogue: readonly ActionDefinition[]): DeckCheck {
  const missing = missingPlugins(deck, catalogue);
  const problems: string[] = [];
  const profiles = deck.profiles ?? [];
  const pageIds = new Set<string>();
  const widgetIds = new Set<string>();
  for (const profile of profiles) {
    if (!profile || typeof profile.id !== "string") {
      problems.push("A profile has no id.");
      continue;
    }
    if (!Array.isArray(profile.pages ?? [])) {
      problems.push(`Profile “${profile.name || profile.id}”: its pages are not a list.`);
      continue;
    }
    for (const page of profile.pages ?? []) {
      if (!page || typeof page.id !== "string" || page.id === "") problems.push("A page has no id.");
      else if (pageIds.has(page.id)) problems.push(`Two pages share the id “${page.id}”.`);
      else pageIds.add(page.id);
      for (const widget of page?.widgets ?? []) {
        if (typeof widget?.id === "string" && widgetIds.has(widget.id)) problems.push(`Two keys share the id “${widget.id}”.`);
        else if (typeof widget?.id === "string") widgetIds.add(widget.id);
      }
    }
  }
  if (problems.length > 0) return { missing, problems };
  if (pageCount(deck) === 0) return { missing, problems: ["The deck has no pages."] };

  const target = mode === "append" ? (current.profiles ?? []).find((profile) => profile.id === profileId) ?? null : null;
  if (mode === "append" && !target) return { missing, problems: ["There is no profile here to add the pages to."] };
  const names = namesOf(mode === "append" ? [...(current.profiles ?? []), ...profiles] : profiles);
  for (const profile of profiles) {
    const owner = target ?? profile;
    const spec = gridSpecOf(owner);
    if (spec.cols < 1 || spec.rows < 1) {
      problems.push(`Profile “${owner.name || owner.id}”: its grid has no columns or rows.`);
      continue;
    }
    for (const page of profile.pages ?? []) {
      problems.push(...pageProblems(page, `Page “${page.name || page.id}”`, spec, portraitSpecOf(owner), catalogue, names));
    }
  }
  return { missing, problems };
}

export function appendDeck(current: WireConfig, profileId: string, deck: WireConfig): WireConfig {
  const pageIds: string[] = [];
  const widgetIds: string[] = [];
  for (const profile of current.profiles ?? []) {
    for (const page of profile.pages ?? []) {
      pageIds.push(page.id);
      for (const widget of page.widgets ?? []) widgetIds.push(widget.id);
    }
  }
  const renames = new Map<string, string>();
  const incoming = (deck.profiles ?? []).flatMap((profile) => profile.pages ?? []);
  for (const page of incoming) {
    const id = nextPageId(pageIds);
    pageIds.push(id);
    renames.set(page.id, id);
  }
  const pages: WirePage[] = incoming.map((page) => {
    const widgets = (page.widgets ?? []).map((widget) => {
      const id = nextWidgetId(widgetIds);
      widgetIds.push(id);
      const copy: WireWidget = structuredClone({ ...widget, id });
      eachAction(copy, (block) => {
        const arg = block.args?.page as { text?: string } | undefined;
        const renamed = arg?.text ? renames.get(arg.text.trim()) : undefined;
        if (block.plugin === "deck" && block.action === "go_to_page" && renamed) block.args.page = { text: renamed };
      });
      return copy;
    });
    return { ...structuredClone(page), id: renames.get(page.id)!, widgets };
  });
  let placed = false;
  const profiles = (current.profiles ?? []).map((profile) => {
    if (profile.id !== profileId) return profile;
    placed = true;
    return { ...profile, pages: [...(profile.pages ?? []), ...pages] };
  });
  if (!placed) throw new Error("There is no profile here to add the pages to.");
  return { ...current, schema: Math.max(current.schema ?? 0, deck.schema ?? 0), profiles };
}
