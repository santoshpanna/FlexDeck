import type { TabBarItem } from "@flexdeck/ui";

/** The window's four places. Order matters: it sets which way a screen slides. */
export type Tab = "home" | "pair" | "plugins" | "setup";

export const TABS: readonly TabBarItem<Tab>[] = [
  { value: "home", label: "HOME", glyph: "home" },
  { value: "pair", label: "PAIR", glyph: "tablet" },
  { value: "plugins", label: "PLUGINS", glyph: "plug" },
  { value: "setup", label: "SETUP", glyph: "sliders" },
];

export function tabIndex(tab: Tab): number {
  return TABS.findIndex((item) => item.value === tab);
}
