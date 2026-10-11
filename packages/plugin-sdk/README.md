# @flexdeck/plugin-sdk

Write FlexDeck plugins in TypeScript or JavaScript. You define actions, values, events, feeds and gallery keys; the SDK speaks the protocol, and builds, packs and verifies the plugin.

    npm create flexdeck-plugin lights "Desk lights"
    cd lights
    npm install
    npm test              # its tests, without FlexDeck
    npm run verify        # ask FlexDeck itself whether it will work
    npm run package       # lights-0.1.0.fdpkg, ready to install

```ts
import { definePlugin } from "@flexdeck/plugin-sdk";

export default definePlugin({
  id: "lights",
  name: "Desk lights",
  version: "0.1.0",
  permissions: ["network"],
  variables: { "lights.on": { kind: "bool", description: "Whether the lights are on" } },
  actions: {
    toggle: {
      description: "Turn the lights on or off",
      arguments: { room: { kind: "text", example: "desk" } },
      run: (_args, ctx) => ctx.vars.set({ "lights.on": !ctx.vars.get("lights.on") }),
    },
  },
});
```

## Verify before you publish

`flexdeck-plugin verify` packs the plugin and hands it to the FlexDeck on this computer (`flexdeck verify-plugin`), which runs it exactly as an install would: the same host, deadlines and rules. It checks:
- its package and catalogue, as the agent accepts them;
- its gallery keys, as a saved deck would be checked;
- every action's example arguments, checked by the plugin itself;
- its values, feeds and settings changes;
- that it never writes outside its own namespace, emits an event it didn't declare, or floods the host;
- that it writes nothing but the protocol to stdout;
- that it stops cleanly.

`--run-actions` also runs every action with its examples, and those actions do real things. If FlexDeck is installed somewhere unusual, set `FLEXDECK_BIN` to its path.

`@flexdeck/plugin-sdk/testing` has an in-process harness for unit tests.
