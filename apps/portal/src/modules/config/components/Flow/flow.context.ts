import { createContext, useContext } from "react";

import type { ActionDefinition, Block, VariableChoice } from "../../../../services/api/api.ts";
import type { LoadChoices } from "../../hooks/useArgumentChoices/useArgumentChoices.hook.ts";

/**
 * What every step in a flow may need to know about this desktop: the actions
 * its plugins offer, the live values they publish, and how to ask a plugin for
 * an argument's choices. Provided once around a flow editor rather than passed
 * down through every nested lane.
 */
export interface FlowEnvironment {
  catalogue: readonly ActionDefinition[];
  variables: readonly VariableChoice[];
  loadChoices?: LoadChoices;
  /** Open the step palette; resolves with the chosen block, or `null` when dismissed. */
  pick: () => Promise<Block | null>;
}

export const FlowContext = createContext<FlowEnvironment>({
  catalogue: [],
  variables: [],
  pick: async () => null,
});

export function useFlow(): FlowEnvironment {
  return useContext(FlowContext);
}

/**
 * Stable identities for blocks, so cards keep their place (and their focus)
 * across edits. Blocks are plain wire objects with no id; an edit replaces the
 * edited object, so whoever makes the replacement hands its identity on.
 */
const ids = new WeakMap<object, string>();
let next = 0;

export function blockKey(block: Block): string {
  let id = ids.get(block);
  if (!id) {
    id = `b${(next += 1)}`;
    ids.set(block, id);
  }
  return id;
}

/** The new object takes over the old one's identity. */
export function inherit(from: Block, to: Block): Block {
  ids.set(to, blockKey(from));
  return to;
}

/** One line naming a step the way a person would: "OBS Studio · Switch scene — Variety Games". */
export function summarize(block: Block, catalogue: readonly ActionDefinition[]): string {
  switch (block.block) {
    case "doAction": {
      const definition = catalogue.find((entry) => entry.plugin === block.plugin && entry.action === block.action);
      const name = definition ? `${definition.pluginName ?? definition.plugin} · ${definition.description || definition.action}` : `${block.plugin}:${block.action}`;
      const first = Object.values(block.args ?? {})[0];
      const value = first ? ("text" in first ? first.text : "variable" in first ? `{{${first.variable}}}` : "number" in first ? String(first.number) : "bool" in first ? String(first.bool) : "") : "";
      return value ? `${name} — ${value}` : name;
    }
    case "wait":
      return `Wait ${"number" in block.ms ? block.ms.number : "?"} ms`;
    case "repeat":
      return `Repeat ${"number" in block.times ? block.times.number : "?"}× · ${block.body.length} steps`;
    case "if":
      return `If… · ${block.then.length} steps`;
    case "setVariable":
      return `Remember ${block.name || "a value"}`;
  }
}
