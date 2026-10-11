import { useSyncExternalStore } from "react";

import { themeStore, type ThemePreference } from "@flexdeck/tokens/theme.ts";

/**
 * The current theme choice, and a way to change it.
 *
 * `useSyncExternalStore` rather than `useState` + an effect: the class on
 * `<html>` is the real state and it is set outside React (by the bootstrap
 * script, before React exists), so React has to read it rather than own it. A
 * `useState` copy would be a second source of truth that disagrees on the first
 * paint.
 */
export function useTheme(): {
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
} {
  const store = themeStore();
  const preference = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  return { preference, setPreference: store.set };
}
