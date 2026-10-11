/**
 * A flexdeck plugin, ready to copy.
 *
 * Run `pnpm create-plugin <id> "<Name>"` from the repository root to get your
 * own copy with the id replaced everywhere. Then:
 *
 *   pnpm test          run the tests below, without the agent
 *   pnpm run package   write <id>-<version>.fdpkg, ready to install
 *
 * While writing it, link the folder from the flexdeck window (Plugins → Link a
 * folder) and every save reloads it in the running agent.
 *
 * Everything a plugin can declare is used once below, with a note on why it
 * looks the way it does. Delete what you do not need.
 */

import { definePlugin } from "@flexdeck/plugin-sdk";

/** State lives in the process. The agent keeps one instance running. */
let greetings = 0;

export default definePlugin({
  // The id is the namespace for everything this plugin owns: variables are
  // `template.<name>`, and a widget's action says `plugin: "template"`. It must
  // match plugin.json, and it can never change once people have used it.
  id: "template",
  name: "Template",
  version: "0.1.0",
  description: "A starting point for a flexdeck plugin.",
  author: "you",

  // Shown to the person before they install. Ask only for what you use:
  // network, notifications, processes, filesystem. It must match plugin.json.
  permissions: ["notifications"],

  // Configuration the person fills in once. A `secret` is never shown again
  // after it is saved — use it for passwords and tokens.
  settings: {
    greeting: { kind: "text", description: "The word to greet with", default: "Hello" },
  },

  // Live values a deck can show and an automation can react to. Declared up
  // front when you know them; `ctx.vars.declare` adds more at run time.
  variables: {
    "template.greetings": { kind: "number", description: "How many greetings so far" },
  },

  // Things that happen, which an automation can trigger on.
  events: {
    greeted: { description: "Someone was greeted", fields: ["name"] },
  },

  actions: {
    greet: {
      description: "Greet someone",
      // Every argument has an example that works: the editor prefills it, and
      // the tests below run every action with its examples.
      arguments: {
        name: { kind: "text", description: "Who to greet", example: "world" },
      },
      // Refuse a bad value *before* anything runs, with a reason.
      check: ({ name }) => (name.trim().length === 0 ? "a name is required" : undefined),
      // Do the thing. Throwing fails the press with your message.
      run: ({ name }, ctx) => {
        greetings += 1;
        ctx.notify(ctx.settings.greeting ?? "Hello", name);
        ctx.vars.set({ "template.greetings": greetings });
        ctx.emit("greeted", { name });
      },
    },
  },

  // Asked for only the variables someone is watching. A plugin that is not
  // watched is never asked, so this can be as expensive as it needs to be.
  sample: () => ({ "template.greetings": greetings }),

  // After the agent has started the plugin: connect, subscribe, start timers.
  start: (ctx) => {
    ctx.log.info("started");
  },

  // Before the agent stops the plugin: close connections.
  stop: () => {},
});
