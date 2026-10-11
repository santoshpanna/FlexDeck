/**
 * Which appearance is on, and how it changes.
 *
 * ONE mechanism: the `dark` class on `<html>`. `prefers-color-scheme` is read in
 * exactly one place — to resolve the "system" preference — and never sets a
 * colour itself, so there is no second source of truth that can half-apply and
 * leave an interface with one foot in each theme.
 *
 * The class is put on before the first paint by an inline script in each app's
 * `index.html`; this module takes over from there. That duplication is
 * deliberate: a module cannot run early enough to stop a light flash in a
 * production build, and preventing the flash is the only reason the script
 * exists. The two must agree on `THEME_KEY` and `DARK_CLASS`.
 */

export type ThemePreference = "light" | "dark" | "system";
export type Appearance = "light" | "dark";

/** Where the choice is remembered. Must match the inline bootstrap scripts. */
export const THEME_KEY = "flexdeck.theme";

/** The class `tokens.css` keys its dark block off. Must match the bootstrap. */
export const DARK_CLASS = "dark";

const PREFERENCES: readonly ThemePreference[] = ["light", "dark", "system"];

/**
 * Everything the store needs from the outside world, so a test can supply it
 * instead of standing up a document. Nothing here is optional: a default would
 * quietly change the behaviour a test is asserting.
 */
export interface ThemeEnvironment {
  /** `<html>`, or anything that can carry the class. */
  root: { classList: { toggle(name: string, on: boolean): void } };
  /** `null` when storage is unavailable — private mode, a blocked cookie jar. */
  storage: {
    getItem(key: string): string | null;
    setItem(key: string, value: string): void;
  } | null;
  /** The operating system's preference, right now. */
  prefersDark: () => boolean;
  /** Call back whenever the operating system's preference changes. */
  watchSystem: (listener: () => void) => () => void;
}

export interface ThemeStore {
  /** The user's choice, including "system". What the toggle reads. */
  getSnapshot: () => ThemePreference;
  subscribe: (listener: () => void) => () => void;
  /** Choose an appearance. Persisted, applied, and pushed to subscribers. */
  set: (preference: ThemePreference) => void;
  /** The appearance actually on screen, with "system" already resolved. */
  resolved: () => Appearance;
}

/** A stored value that is not one of the three preferences is not a choice. */
export function readPreference(storage: ThemeEnvironment["storage"]): ThemePreference {
  if (storage === null) {
    return "system";
  }
  let stored: string | null = null;
  try {
    stored = storage.getItem(THEME_KEY);
  } catch {
    // A storage that throws on read is the same as no storage at all.
    return "system";
  }
  const match = PREFERENCES.find((candidate) => candidate === stored);
  return match ?? "system";
}

export function resolveAppearance(
  preference: ThemePreference,
  prefersDark: boolean,
): Appearance {
  if (preference === "system") {
    return prefersDark ? "dark" : "light";
  }
  return preference;
}

export function createThemeStore(environment: ThemeEnvironment): ThemeStore {
  let preference = readPreference(environment.storage);
  const listeners = new Set<() => void>();

  const apply = (appearance: Appearance) => {
    environment.root.classList.toggle(DARK_CLASS, appearance === "dark");
  };

  const notify = () => {
    for (const listener of listeners) {
      listener();
    }
  };

  // The bootstrap script has already put the class on; doing it again is
  // idempotent and means the store is correct even where that script did not
  // run (a test, a storybook, a future embed).
  const sync = () => apply(resolveAppearance(preference, environment.prefersDark()));

  sync();

  // Follow the operating system only while "system" is the choice. A pinned
  // preference must survive a change that is not about it.
  environment.watchSystem(() => {
    if (preference === "system") {
      sync();
      notify();
    }
  });

  return {
    getSnapshot: () => preference,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    set: (next) => {
      if (!PREFERENCES.includes(next) || next === preference) {
        return;
      }
      preference = next;
      try {
        environment.storage?.setItem(THEME_KEY, next);
      } catch {
        // Failing to remember the choice is not a reason to refuse to apply it.
      }
      sync();
      notify();
    },
    resolved: () => resolveAppearance(preference, environment.prefersDark()),
  };
}

/** Storage that is not there, or that refuses to be read, means "follow the system". */
function safeStorage(): ThemeEnvironment["storage"] {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The store the two apps share, wired to the real document. */
export function browserEnvironment(): ThemeEnvironment {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  return {
    root: document.documentElement,
    storage: safeStorage(),
    prefersDark: () => media.matches,
    watchSystem: (listener) => {
      media.addEventListener("change", listener);
      return () => media.removeEventListener("change", listener);
    },
  };
}

let shared: ThemeStore | null = null;

/**
 * The one store for this document. Memoised so the portal and any other
 * consumer cannot end up with two subscriptions disagreeing about the class.
 */
export function themeStore(): ThemeStore {
  shared ??= createThemeStore(browserEnvironment());
  return shared;
}

/** Called once at startup, so a later system change is followed. */
export function startTheme(): void {
  themeStore();
}
