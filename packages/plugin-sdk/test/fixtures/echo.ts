// A plugin for the SDK's process-level test: started as a real child, it must
// speak the protocol over stdin/stdout exactly as the agent expects.
import { definePlugin } from "../../src/index.ts";

let count = 0;

export default definePlugin({
  id: "echo",
  name: "Echo",
  version: "1.0.0",
  permissions: ["notifications"],
  settings: { prefix: { kind: "text", default: ">" } },
  variables: { "echo.count": { kind: "number" } },
  events: { echoed: { fields: ["text"] } },
  actions: {
    say: {
      description: "Echo text",
      arguments: { text: { kind: "text", example: "hi" } },
      run: ({ text }, ctx) => {
        count += 1;
        // A stray console.log must not corrupt the protocol.
        console.log("this goes to stderr, not stdout");
        ctx.vars.set({ "echo.count": count });
        ctx.emit("echoed", { text: `${ctx.settings.prefix} ${text}` });
      },
    },
    fail: {
      description: "Always fails",
      run: () => {
        throw new Error("asked to fail");
      },
    },
  },
});
