import type {
  ActionDefinition,
  Block,
  CompareOp,
  Condition,
  Gesture,
  ValueRef,
  WireConfig,
  WireFlow,
  WireWidget,
} from "../../../services/api/api.ts";
import { withWidget } from "./editing.lib.ts";

/**
 * Flows as data: one vocabulary of edits over `Block[]`, shared by what a key
 * does when pressed and by automations — they are the same blocks on the wire,
 * so they get the same editor.
 *
 * Nested lists (an `if`'s branches, a `repeat`'s body) are addressed by a
 * LANE: the path of (block index, branch) pairs from the top-level list down.
 * `[]` is the top level; `[{ at: 2, branch: "then" }]` is the "then" list of
 * the third block. Every edit is pure and returns a new list.
 */

export type Branch = "then" | "otherwise" | "body" | `elseIf:${number}`;

export interface LaneStep {
  at: number;
  branch: Branch;
}

export type Lane = readonly LaneStep[];

/** The list of blocks a branch of `block` holds, or `null` if it has no such branch. */
export function branchOf(block: Block, branch: Branch): Block[] | null {
  if (block.block === "repeat") return branch === "body" ? block.body : null;
  if (block.block !== "if") return null;
  if (branch === "then") return block.then;
  if (branch === "otherwise") return block.otherwise ?? [];
  if (branch.startsWith("elseIf:")) return block.elseIf?.[Number(branch.slice(7))]?.blocks ?? null;
  return null;
}

function withBranch(block: Block, branch: Branch, blocks: Block[]): Block {
  if (block.block === "repeat" && branch === "body") return { ...block, body: blocks };
  if (block.block !== "if") return block;
  if (branch === "then") return { ...block, then: blocks };
  if (branch === "otherwise") {
    const next = { ...block };
    if (blocks.length > 0) next.otherwise = blocks;
    else delete next.otherwise;
    return next;
  }
  if (branch.startsWith("elseIf:")) {
    const index = Number(branch.slice(7));
    const elseIf = (block.elseIf ?? []).map((arm, i) => (i === index ? { ...arm, blocks } : arm));
    return { ...block, elseIf };
  }
  return block;
}

/** The blocks at `lane`, or `[]` when the lane does not exist. */
export function laneBlocks(blocks: readonly Block[], lane: Lane): Block[] {
  let current: Block[] = [...blocks];
  for (const step of lane) {
    const block = current[step.at];
    const next = block ? branchOf(block, step.branch) : null;
    if (!next) return [];
    current = next;
  }
  return current;
}

/** Replace the list at `lane` with what `edit` makes of it. */
export function editLane(blocks: readonly Block[], lane: Lane, edit: (list: Block[]) => Block[]): Block[] {
  if (lane.length === 0) return edit([...blocks]);
  const [head, ...rest] = lane;
  if (!head) return [...blocks];
  return blocks.map((block, index) => {
    if (index !== head.at) return block;
    const inner = branchOf(block, head.branch);
    if (!inner) return block;
    return withBranch(block, head.branch, editLane(inner, rest, edit));
  });
}

export function insertBlock(blocks: readonly Block[], lane: Lane, at: number, block: Block): Block[] {
  return editLane(blocks, lane, (list) => {
    const index = Math.max(0, Math.min(at, list.length));
    return [...list.slice(0, index), block, ...list.slice(index)];
  });
}

export function removeBlock(blocks: readonly Block[], lane: Lane, at: number): Block[] {
  return editLane(blocks, lane, (list) => list.filter((_, index) => index !== at));
}

export function replaceBlock(blocks: readonly Block[], lane: Lane, at: number, block: Block): Block[] {
  return editLane(blocks, lane, (list) => list.map((current, index) => (index === at ? block : current)));
}

/** Move a block within its lane. Out-of-range targets are clamped; a no-op returns the same order. */
export function moveBlock(blocks: readonly Block[], lane: Lane, from: number, to: number): Block[] {
  return editLane(blocks, lane, (list) => {
    if (from < 0 || from >= list.length) return list;
    const target = Math.max(0, Math.min(to, list.length - 1));
    const next = [...list];
    const [moved] = next.splice(from, 1);
    if (moved) next.splice(target, 0, moved);
    return next;
  });
}

export function duplicateBlock(blocks: readonly Block[], lane: Lane, at: number): Block[] {
  return editLane(blocks, lane, (list) => {
    const block = list[at];
    return block ? [...list.slice(0, at + 1), structuredClone(block), ...list.slice(at + 1)] : list;
  });
}

export type BlockKind = Block["block"];

/** A fresh block of each kind — always valid on the wire, so an edit never writes something the agent refuses. */
export function emptyBlock(kind: BlockKind): Block {
  switch (kind) {
    case "if":
      return { block: "if", condition: { kind: "compare", left: { variable: "" }, op: "eq", right: { bool: true } }, then: [] };
    case "repeat":
      return { block: "repeat", times: { number: 3 }, body: [] };
    case "wait":
      return { block: "wait", ms: { number: 500 } };
    case "setVariable":
      return { block: "setVariable", name: "", value: { text: "" } };
    case "doAction":
      return { block: "doAction", plugin: "", action: "", args: {} };
  }
}

export function actionBlock(action: Pick<ActionDefinition, "plugin" | "action">): Block {
  return { block: "doAction", plugin: action.plugin, action: action.action, args: {} };
}

/* ---- values ------------------------------------------------------------- */

/** A value as one line of text: literal text, or `{{name}}` for a variable. */
export function valueText(value: ValueRef): string {
  if ("text" in value) return value.text;
  if ("number" in value) return String(value.number);
  if ("bool" in value) return String(value.bool);
  if ("input" in value) return `{{input.${value.input}}}`;
  return `{{${value.variable}}}`;
}

/**
 * Text as a value: `{{name}}` alone is a variable, `{{input.x}}` the run's
 * input, anything else text — whose holes the agent fills when the step runs
 * ("Temp: {{vars.temp | round}}°").
 */
export function textValue(text: string): ValueRef {
  const match = /^\{\{\s*([^{}\s]+)\s*\}\}$/.exec(text.trim());
  if (!match) return { text };
  const name = match[1]!;
  return name.startsWith("input.") ? { input: name.slice(6) } : { variable: name };
}

/** A typed literal: numbers and booleans as themselves, so comparisons compare like with like. */
export function literalValue(text: string): ValueRef {
  const trimmed = text.trim();
  if (trimmed === "true" || trimmed === "false") return { bool: trimmed === "true" };
  if (trimmed !== "" && !Number.isNaN(Number(trimmed))) return { number: Number(trimmed) };
  return textValue(text);
}

export function argsText(args: Record<string, ValueRef>): Record<string, string> {
  return Object.fromEntries(Object.entries(args).map(([name, value]) => [name, valueText(value)]));
}

export function textArgs(args: Record<string, string>): Record<string, ValueRef> {
  return Object.fromEntries(Object.entries(args).filter(([, text]) => text !== "").map(([name, text]) => [name, textValue(text)]));
}

/* ---- conditions --------------------------------------------------------- */

export const COMPARE_OPS: readonly { op: CompareOp; label: string }[] = [
  { op: "eq", label: "is" },
  { op: "notEq", label: "is not" },
  { op: "greater", label: "is above" },
  { op: "greaterOrEqual", label: "is at least" },
  { op: "less", label: "is below" },
  { op: "lessOrEqual", label: "is at most" },
  { op: "contains", label: "contains" },
];

/** The comparisons a condition is made of, flattened for a list editor, and how they combine. */
export function conditionParts(condition: Condition): { mode: "all" | "any"; parts: Condition[] } {
  if (condition.kind === "all" || condition.kind === "any") return { mode: condition.kind, parts: condition.conditions };
  return { mode: "all", parts: [condition] };
}

/** Build a condition back from parts: one part stands alone, several are joined. */
export function joinConditions(mode: "all" | "any", parts: Condition[]): Condition {
  if (parts.length === 1) return parts[0]!;
  return { kind: mode, conditions: parts };
}

/* ---- a key's gestures ----------------------------------------------------- */

export type Slot = "tap" | "double" | "long" | "hold";

export const SLOT_GESTURES: Record<Exclude<Slot, "hold">, Gesture> = {
  tap: "GESTURE_SHORT_PRESS",
  double: "GESTURE_DOUBLE_TAP",
  long: "GESTURE_LONG_PRESS",
};

export const HOLD_DOWN: Gesture = "GESTURE_HOLD_START";
export const HOLD_UP: Gesture = "GESTURE_HOLD_END";

/** The blocks a gesture runs on this widget — on one zone of its strip, or (no zone) the key itself. */
export function gestureBlocks(widget: WireWidget, gesture: Gesture, zone = ""): Block[] {
  return widget.flows?.find((flow) => flow.trigger?.gesture === gesture && (flow.zone ?? "") === zone)?.blocks ?? [];
}

/** Whether the widget is pressed as a hold: then the tablet sends down/up, never a tap. */
export function holdsOf(widget: WireWidget): boolean {
  return (widget.flows ?? []).some((flow) => !flow.zone && (flow.trigger?.gesture === HOLD_DOWN || flow.trigger?.gesture === HOLD_UP));
}

/**
 * Set what one gesture runs, keeping every other flow — other gestures, and the
 * event-triggered ones this editor does not draw. An empty list removes the
 * flow; a widget with no flows stores none, as the agent sends it.
 *
 * A hold and a tap are exclusive on the tablet (a held key never reports a tap),
 * so giving the hold something to do drops the tap, and giving the tap
 * something drops the hold — the editor says so before it happens.
 */
export function setGestureBlocks(config: WireConfig, widgetId: string, gesture: Gesture, blocks: Block[], zone = ""): WireConfig {
  return withWidget(config, widgetId, (widget) => {
    const isHold = gesture === HOLD_DOWN || gesture === HOLD_UP;
    const filled = blocks.length > 0;
    const keep = (widget.flows ?? []).filter((flow) => {
      // A zone's flows and the key's never touch each other.
      if ((flow.zone ?? "") !== zone) return true;
      const own = flow.trigger?.gesture;
      if (own === gesture) return false;
      if (filled && isHold && own === "GESTURE_SHORT_PRESS") return false;
      if (filled && gesture === "GESTURE_SHORT_PRESS" && (own === HOLD_DOWN || own === HOLD_UP)) return false;
      return true;
    });
    const mine: WireFlow[] = filled ? [zone ? { trigger: { gesture }, zone, blocks } : { trigger: { gesture }, blocks }] : [];
    const flows = [...mine, ...keep];
    const next = { ...widget };
    if (flows.length > 0) next.flows = flows;
    else delete next.flows;
    return next;
  });
}

/** How many steps each slot runs, for the slot keys. */
export function slotCounts(widget: WireWidget): Record<Slot, number> {
  return {
    tap: gestureBlocks(widget, SLOT_GESTURES.tap).length,
    double: gestureBlocks(widget, SLOT_GESTURES.double).length,
    long: gestureBlocks(widget, SLOT_GESTURES.long).length,
    hold: gestureBlocks(widget, HOLD_DOWN).length + gestureBlocks(widget, HOLD_UP).length,
  };
}
