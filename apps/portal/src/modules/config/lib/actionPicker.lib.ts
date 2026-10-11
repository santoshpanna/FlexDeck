import type { ActionDefinition } from "../../../services/api/api.ts";

/** One plugin's actions, under the plugin's name. */
export interface ActionGroup {
  plugin: string;
  name: string;
  actions: ActionDefinition[];
}

/** The plugin's name as a person reads it, falling back to its id. */
export function pluginLabel(entry: Pick<ActionDefinition, "plugin" | "pluginName">): string {
  return entry.pluginName?.trim() || entry.plugin;
}

/**
 * The catalogue grouped by plugin, in the order the agent listed it, keeping
 * only what matches `query`.
 *
 * Every word of the query must appear somewhere in the action's description,
 * its name, or its plugin's name or id — so "obs rec" finds OBS's recording
 * actions, and "volume" finds the media ones wherever they live.
 */
export function groupActions(catalogue: readonly ActionDefinition[], query: string): ActionGroup[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const groups = new Map<string, ActionGroup>();
  for (const entry of catalogue) {
    const haystack = [entry.description, entry.action, entry.plugin, entry.pluginName ?? ""].join(" ").toLowerCase();
    if (!words.every((word) => haystack.includes(word))) continue;
    let group = groups.get(entry.plugin);
    if (!group) {
      group = { plugin: entry.plugin, name: pluginLabel(entry), actions: [] };
      groups.set(entry.plugin, group);
    }
    group.actions.push(entry);
  }
  return [...groups.values()];
}
