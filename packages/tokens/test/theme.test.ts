import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createThemeStore,
  readPreference,
  resolveAppearance,
  THEME_KEY,
  type ThemeEnvironment,
} from "@flexdeck/tokens/theme.ts";

/**
 * A fake environment, so the store is tested without a document.
 *
 * `prefersDark` is a function rather than a value because the whole point of
 * "system" is that it can change under the store's feet.
 */
function environment(options: {
  stored?: string | null;
  prefersDark?: boolean;
  storageThrows?: boolean;
} = {}) {
  const classes = new Set<string>();
  const written: [string, string][] = [];
  let prefersDark = options.prefersDark ?? false;
  let listener: (() => void) | null = null;

  const environment: ThemeEnvironment = {
    root: {
      classList: {
        toggle(name, on) {
          if (on) {
            classes.add(name);
          } else {
            classes.delete(name);
          }
        },
      },
    },
    storage: options.storageThrows
      ? {
          getItem() {
            throw new Error("blocked");
          },
          setItem() {
            throw new Error("blocked");
          },
        }
      : {
          getItem: (key) => (key === THEME_KEY ? (options.stored ?? null) : null),
          setItem: (key, value) => written.push([key, value]),
        },
    prefersDark: () => prefersDark,
    watchSystem: (next) => {
      listener = next;
      return () => {
        listener = null;
      };
    },
  };

  return {
    environment,
    classes,
    written,
    /** Change the operating system's preference and tell the store. */
    systemGoesDark(on: boolean) {
      prefersDark = on;
      listener?.();
    },
  };
}

test("a stored choice is read back", () => {
  assert.equal(readPreference({ getItem: () => "dark", setItem: () => {} }), "dark");
  assert.equal(readPreference({ getItem: () => "light", setItem: () => {} }), "light");
  assert.equal(readPreference({ getItem: () => "system", setItem: () => {} }), "system");
});

test("a stored value that is not a choice falls back to the system", () => {
  // A key written by an older build, or by hand. Guessing at it would be worse
  // than following the machine.
  assert.equal(readPreference({ getItem: () => "midnight", setItem: () => {} }), "system");
  assert.equal(readPreference(null), "system");
});

test("storage that throws on read is the same as no storage", () => {
  const throwing = {
    getItem() {
      throw new Error("blocked");
    },
    setItem() {
      throw new Error("blocked");
    },
  };
  assert.equal(readPreference(throwing), "system");
});

test("system resolves to the operating system's preference", () => {
  assert.equal(resolveAppearance("system", true), "dark");
  assert.equal(resolveAppearance("system", false), "light");
  // A pinned choice ignores the system entirely.
  assert.equal(resolveAppearance("light", true), "light");
  assert.equal(resolveAppearance("dark", false), "dark");
});

test("the store applies the class on creation", () => {
  const dark = environment({ stored: "dark" });
  createThemeStore(dark.environment);
  assert.ok(dark.classes.has("dark"));

  const light = environment({ stored: "light", prefersDark: true });
  createThemeStore(light.environment);
  assert.ok(!light.classes.has("dark"), "a pinned light must beat a dark system");
});

test("choosing an appearance applies it and remembers it", () => {
  const fake = environment({ stored: "system", prefersDark: false });
  const store = createThemeStore(fake.environment);
  assert.ok(!fake.classes.has("dark"));

  store.set("dark");
  assert.ok(fake.classes.has("dark"));
  assert.deepEqual(fake.written, [[THEME_KEY, "dark"]]);
  assert.equal(store.getSnapshot(), "dark");

  store.set("light");
  assert.ok(!fake.classes.has("dark"));
  assert.equal(store.getSnapshot(), "light");
});

test("subscribers hear about a change, and only a real one", () => {
  const fake = environment({ stored: "light" });
  const store = createThemeStore(fake.environment);
  let heard = 0;
  const stop = store.subscribe(() => {
    heard += 1;
  });

  store.set("dark");
  assert.equal(heard, 1);

  // Setting the value that is already on is not a change: notifying would make
  // every consumer re-render for nothing.
  store.set("dark");
  assert.equal(heard, 1);

  stop();
  store.set("light");
  assert.equal(heard, 1, "an unsubscribed listener must not be called");
});

test("a system change is followed only while the choice is system", () => {
  const fake = environment({ stored: "system", prefersDark: false });
  const store = createThemeStore(fake.environment);
  assert.ok(!fake.classes.has("dark"));

  fake.systemGoesDark(true);
  assert.ok(fake.classes.has("dark"), "system must follow the machine");
  assert.equal(store.resolved(), "dark");

  // Pinned to light: the machine changing is no longer this store's business.
  store.set("light");
  fake.systemGoesDark(false);
  fake.systemGoesDark(true);
  assert.ok(!fake.classes.has("dark"), "a pinned choice must survive a system change");

  // Back to system, and it follows again.
  store.set("system");
  assert.ok(fake.classes.has("dark"));
});

test("a storage that refuses to write still applies the choice", () => {
  const fake = environment({ stored: "light", storageThrows: true });
  const store = createThemeStore(fake.environment);
  store.set("dark");
  assert.ok(fake.classes.has("dark"), "failing to remember is not a reason to refuse");
});
