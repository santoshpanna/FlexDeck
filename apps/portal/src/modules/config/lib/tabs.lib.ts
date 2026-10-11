/**
 * The configurator's top-level sections.
 *
 * A section is deliberately *not* a route. The whole configurator is one screen
 * backed by one loaded document, and switching sections must not refetch it or
 * drop the editor's selection. Tabs held in state say that; links would promise
 * a URL per section and a remount per click.
 */

export type ConfigTab = "deck" | "automations" | "data" | "plugins" | "devices" | "system";

/** The sections, in the order they are shown. */
export const CONFIG_TABS: readonly { id: ConfigTab; label: string }[] = [
  { id: "deck", label: "Deck" },
  { id: "automations", label: "Automations" },
  { id: "data", label: "Data" },
  { id: "plugins", label: "Plugins" },
  { id: "devices", label: "Devices" },
  { id: "system", label: "System" },
];
