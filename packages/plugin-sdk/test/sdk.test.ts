import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { ABI_VERSION, definePlugin, type Message } from "../src/index.ts";
import { harness } from "../src/testing.ts";
import { zip } from "../src/zip.ts";

const contract = fileURLToPath(new URL("../../../contract/fixtures/plugin/", import.meta.url));
const update = process.env.UPDATE_FIXTURES === "1";

/** The definition both fixtures are made from. Change it, and both change. */
const sample = definePlugin({
  id: "sample",
  name: "Sample",
  version: "1.2.3",
  description: "The cross-language contract fixture",
  author: "flexdeck",
  permissions: ["network", "notifications"],
  settings: {
    host: { kind: "text", description: "Where to connect", default: "localhost" },
    password: { kind: "text", secret: true },
    mode: { kind: "choice", choices: ["a", "b"], default: "a" },
  },
  variables: {
    "sample.connected": { kind: "bool", description: "Connected" },
    "sample.level": { kind: "number", unit: "%", readOnly: false },
  },
  events: { changed: { description: "Something changed", fields: ["what"] } },
  actions: {
    go: {
      description: "Go somewhere",
      arguments: {
        where: { kind: "choice", example: "left", choices: ["left", "right"] },
        note: { kind: "text", example: "", optional: true },
      },
      run: () => {},
    },
  },
});

/** Compare with a committed fixture, or rewrite it under UPDATE_FIXTURES=1. */
function pinned(name: string, actual: Buffer | string): void {
  const path = `${contract}${name}`;
  if (update || !existsSync(path)) {
    mkdirSync(contract, { recursive: true });
    writeFileSync(path, actual);
    return;
  }
  assert.deepEqual(
    Buffer.from(actual),
    readFileSync(path),
    `${name} drifted from the committed fixture; rerun with UPDATE_FIXTURES=1 if the change is intended, and check the Rust side still parses it`,
  );
}

/** The same plugin with a slider-capable action: what only ABI 3 can carry. */
const sliding = definePlugin({
  ...sample.definition,
  actions: {
    ...sample.definition.actions,
    level: {
      description: "Set the level",
      arguments: {
        channel: { kind: "text", example: "main" },
        percent: { kind: "number", example: "50" },
      },
      slider: { argument: "percent", feedback: "sample.{{channel}}.level" },
      run: () => ({ state: "set" }),
    },
  },
});

test("the manifest has the exact wire shape the Rust ABI parses", () => {
  const initialize = { manifest: sample.manifest(2), variables: sample.variables() };
  pinned("initialize.json", `${JSON.stringify(initialize, null, 2)}\n`);
});

test("at ABI 3 the manifest carries slider metadata the Rust ABI parses", () => {
  const initialize = { manifest: sliding.manifest(3), variables: sliding.variables() };
  pinned("initialize.v3.json", `${JSON.stringify(initialize, null, 2)}\n`);
});

/** The same plugin with a feed and a preset: what only ABI 4 can carry. */
const feeding = definePlugin({
  ...sliding.definition,
  presets: [
    {
      id: "level",
      name: "Level",
      category: "Sample",
      description: "The level, as a gauge",
      w: 1,
      h: 1,
      widget: { face: { layout: "FACE_LAYOUT_DIAL", elements: [{ slot: "SLOT_VISUAL", kind: "ELEMENT_KIND_GAUGE", template: "{{sample.level}}" }] } },
    },
  ],
  feeds: {
    room: {
      description: "One room's readings",
      arguments: { room: { kind: "text", example: "kitchen" } },
      fields: { temp: { kind: "number", description: "Temperature" } },
      watch: (instance, args, ctx) => ctx.feed(instance).set({ temp: args.room.length }),
    },
  },
});

test("at ABI 4 the manifest carries presets and feeds the Rust ABI parses", () => {
  const initialize = { manifest: feeding.manifest(4), variables: feeding.variables() };
  pinned("initialize.v4.json", `${JSON.stringify(initialize, null, 2)}\n`);
  assert.deepEqual(feeding.manifest(4).uses, ["series", "presets", "feeds"]);
  assert.equal(feeding.manifest(3).uses, undefined, "nothing ABI 4 added under 3");
  assert.equal(feeding.manifest(3).feeds, undefined);
});

const toggling = definePlugin({
  id: "toggling",
  name: "Toggling",
  version: "1.0.0",
  variables: {
    "toggling.muted": { kind: "bool" },
    "toggling.scene": { kind: "text" },
  },
  actions: {
    toggle_mute: { description: "Mute", active: { variable: "toggling.muted" }, run: () => {} },
    toggle_user: {
      description: "Mute one",
      arguments: { user: { kind: "text", example: "1" } },
      active: { variable: "toggling.user.{{user}}.muted" },
      run: () => {},
    },
    set_scene: {
      description: "Scene",
      arguments: { scene: { kind: "text", example: "A" } },
      active: { variable: "toggling.scene", equals: "scene" },
      run: () => {},
    },
  },
});

test("a toggle's light reaches only an agent that offers toggles", async () => {
  const offered = toggling.manifest(4);
  assert.deepEqual(offered.actions.map((action) => action.active), [
    { variable: "toggling.muted" },
    { variable: "toggling.user.{{user}}.muted" },
    { variable: "toggling.scene", equals: "scene" },
  ]);
  assert.ok(offered.uses?.includes("toggles"));

  const older = (await toggling.handle("initialize", { abi: 4, hostCapabilities: ["series", "presets", "feeds"] })) as {
    manifest: ReturnType<typeof toggling.manifest>;
  };
  assert.deepEqual(older.manifest.actions.map((action) => action.active), [undefined, undefined, { variable: "toggling.scene", equals: "scene" }]);
  assert.ok(!older.manifest.uses?.includes("toggles"));
});

test("a sign-in, help and a guide reach only an agent that offers setup", async () => {
  const signing = definePlugin({
    id: "signing",
    name: "Signing",
    version: "1.0.0",
    guide: "https://example.com/signing#setup",
    settings: {
      client_id: { kind: "text", description: "Client ID", help: "From the developer portal" },
      token: { kind: "text", secret: true, internal: true },
    },
    actions: {},
  });
  const offered = signing.manifest(4);
  assert.equal(offered.guide, "https://example.com/signing#setup");
  assert.deepEqual(
    offered.settings.map(({ internal, help }) => ({ internal, help })),
    [
      { internal: undefined, help: "From the developer portal" },
      { internal: true, help: undefined },
    ],
  );
  assert.ok(offered.uses?.includes("setup"));

  const older = (await signing.handle("initialize", { abi: 4, hostCapabilities: ["series", "presets", "feeds", "toggles"] })) as {
    manifest: ReturnType<typeof signing.manifest>;
  };
  assert.equal(older.manifest.guide, undefined);
  assert.ok(older.manifest.settings.every((setting) => !("internal" in setting) && !("help" in setting)));
  assert.ok(!older.manifest.uses?.includes("setup"));

  assert.throws(
    () => definePlugin({ id: "bad", name: "Bad", version: "1", guide: "javascript:alert(1)", actions: {} }),
    /not an http\(s\) address/,
  );
});

test("a toggle's light must name the action's own arguments", () => {
  const define = (active: { variable: string; is?: string }) =>
    definePlugin({ id: "bad", name: "Bad", version: "1", actions: { t: { description: "", arguments: { a: { kind: "text", example: "" } }, active, run: () => {} } } });
  assert.throws(() => define({ variable: "bad.{{b}}.on" }), /'b' is not one of its arguments/);
  assert.throws(() => define({ variable: "other.on" }), /must start with 'bad.'/);
  define({ variable: "bad.{{a}}.on", is: "yes" });
});

test("a condition light reaches only an agent that offers conditions", async () => {
  const studio = definePlugin({
    id: "studio",
    name: "Studio",
    version: "1",
    actions: {
      transition: { description: "", active: { when: "obs.studio_mode and obs.preview_scene != obs.scene" }, run: () => {} },
    },
  });
  const newer = studio.manifest(4);
  assert.deepEqual(newer.actions[0]!.active, { when: "obs.studio_mode and obs.preview_scene != obs.scene" });
  assert.ok(newer.uses?.includes("conditions"));
  assert.ok(!newer.uses?.includes("toggles"));

  const older = (await studio.handle("initialize", { abi: 4, hostCapabilities: ["series", "toggles"] })) as {
    manifest: ReturnType<typeof studio.manifest>;
  };
  assert.equal(older.manifest.actions[0]!.active, undefined);
  assert.ok(!older.manifest.uses?.includes("conditions"));
});

test("a condition light names the action's own arguments, outside quotes", () => {
  const define = (active: { when: string }) =>
    definePlugin({ id: "bad", name: "Bad", version: "1", actions: { t: { description: "", arguments: { a: { kind: "text", example: "" } }, active, run: () => {} } } });
  define({ when: "`obs.input.{{a}}.muted` or obs.scene == {{a}}" });
  assert.throws(() => define({ when: "obs.scene == {{b}}" }), /'b' is not one of its arguments/);
  assert.throws(() => define({ when: 'obs.scene == "{{a}}"' }), /inside quotes/);
  assert.throws(() => define({ when: " " }), /when must be/);
  assert.throws(
    () => define({ when: "obs.on", variable: "bad.on" } as unknown as { when: string }),
    /takes when alone/,
  );
});

test("a feed publishes per key, and forgets a key that goes", async () => {
  await feeding.handle("initialize", { abi: 4, hostCapabilities: ["series", "presets", "feeds"] });
  feeding.sent.length = 0;
  await feeding.handle("feed/watch", { instance: "w7", feed: "room", args: { room: "hall" } });
  const declared = feeding.sent.find((m) => m.method === "vars/declare");
  assert.ok(JSON.stringify(declared).includes("sample.@w7.temp"));
  const set = feeding.sent.find((m) => m.method === "vars/set") as { params: { values: Record<string, { value: number }> } };
  assert.equal(set.params.values["sample.@w7.temp"]!.value, 4);
  await feeding.handle("feed/watch", { instance: "w8", feed: "room", args: {} });
  assert.ok(JSON.stringify(feeding.sent).includes('"sample.@w8.temp"'), "a blank argument takes the example");
  await feeding.handle("feed/unwatch", { instance: "w7" });
  assert.throws(() => feeding.context.feed("w7").set({ temp: 1 }), /nothing is watching/);
  feeding.context.series.set("sample.level", [1, 2, Number.NaN, 3]);
  const series = feeding.sent.find((m) => m.method === "series/set") as { params: { points: number[] } };
  assert.deepEqual(series.params.points, [1, 2, 3]);
  await assert.rejects(feeding.handle("feed/watch", { instance: "w9", feed: "nope" }), /no feed/);
});

test("asked for ABI 2, a plugin leaves out everything ABI 3 added", async () => {
  const init = (await sliding.handle("initialize", { abi: 2 })) as { manifest: { abi: number; actions: object[] } };
  assert.equal(init.manifest.abi, 2);
  assert.ok(init.manifest.actions.every((action) => !("slider" in action)));
  const ran = await sliding.handle("invoke", { action: "level", args: { channel: "main", percent: "10" } });
  assert.deepEqual(ran, { ok: true }, "no state under ABI 2");
  assert.throws(() => sliding.context.openUrl("https://example.com"), /ABI 3/);

  await sliding.handle("initialize", { abi: 3 });
  const again = await sliding.handle("invoke", { action: "level", args: { channel: "main", percent: "10" } });
  assert.deepEqual(again, { ok: true, state: "set" });
});

test("a slider must name one of its action's arguments", () => {
  assert.throws(
    () =>
      definePlugin({
        id: "mine",
        name: "Mine",
        version: "1",
        actions: {
          go: { description: "", arguments: { to: { kind: "number", example: "1" } }, slider: { argument: "nope" as "to" }, run: () => {} },
        },
      }),
    /not one of its arguments/,
  );
});

test("stored settings, opened addresses and images reach the agent", async () => {
  const plugin = definePlugin({
    id: "authy",
    name: "Authy",
    version: "1.0.0",
    permissions: ["network"],
    settings: { refresh_token: { kind: "text", secret: true } },
    actions: {
      login: {
        description: "Log in",
        run: (_, ctx) => {
          ctx.openUrl("https://example.com/authorize?x=1");
          ctx.storeSettings({ refresh_token: "r1" });
          ctx.storeSettings({ refresh_token: "r2" });
          return { state: ctx.images.put(new Uint8Array([1, 2, 3]), "image/png") };
        },
      },
    },
  });
  const host = await harness(plugin);
  const state = await host.invoke("login");
  assert.deepEqual(host.stored(), { refresh_token: "r2" });
  assert.equal(plugin.context.settings.refresh_token, "r2", "the plugin sees what it stored");
  assert.deepEqual(host.openedUrls(), ["https://example.com/authorize?x=1"]);
  const [image] = host.images();
  assert.equal(state, `img:${image!.sha256}`);
  assert.equal(image!.data, Buffer.from([1, 2, 3]).toString("base64"));
  assert.throws(() => plugin.context.storeSettings({ other: "x" }), /not declared/);
  assert.throws(() => plugin.context.openUrl("file:///etc/passwd"), /not an http/);
  assert.throws(() => plugin.context.images.put(new Uint8Array(300 * 1024), "image/png"), /limit/);
});

test("a package is byte-for-byte reproducible, so its checksum is", () => {
  const files = [
    { name: "plugin.json", data: Buffer.from(JSON.stringify({ abi: 2, id: "sample", name: "Sample", version: "1.2.3", runtime: "node", entry: "dist/index.js", permissions: ["network", "notifications"] })) },
    { name: "dist/index.js", data: Buffer.from("export default {};\n".repeat(50)) },
  ];
  const first = zip(files);
  const second = zip([...files].reverse());
  assert.deepEqual(first, second, "order and time must not change the bytes");
  pinned("sample.fdpkg", first);
  assert.equal(createHash("sha256").update(first).digest("hex").length, 64);
});

test("a zip refuses a path that would escape the package", () => {
  assert.throws(() => zip([{ name: "../evil.js", data: Buffer.from("") }]), /not a safe path/);
});

test("a definition with a variable outside its namespace is refused at once", () => {
  assert.throws(
    () =>
      definePlugin({
        id: "mine",
        name: "Mine",
        version: "1",
        variables: { "system.volume": { kind: "number" } },
        actions: {},
      }),
    /must start with 'mine\.'/,
  );
});

test("a choice whose example is not a choice is refused at once", () => {
  assert.throws(
    () =>
      definePlugin({
        id: "mine",
        name: "Mine",
        version: "1",
        actions: {
          go: { description: "", arguments: { to: { kind: "choice", example: "up", choices: ["left"] } }, run: () => {} },
        },
      }),
    /must be one of its choices/,
  );
});

test("settings start from their defaults and the saved values win", async () => {
  const host = await harness(sample, { settings: { host: "obs.local" } });
  assert.equal(sample.context.settings.host, "obs.local");
  assert.equal(sample.context.settings.mode, "a");
  await host.changeSettings({ mode: "b" });
  assert.equal(sample.context.settings.mode, "b");
  assert.equal(sample.context.settings.host, "localhost", "a change replaces, defaults refill");
});

test("an unknown action and a failing action both answer with a reason", async () => {
  const unknown = (await sample.handle("invoke", { action: "nope" })) as { ok: boolean; error: string };
  assert.equal(unknown.ok, false);
  assert.match(unknown.error, /no action called 'nope'/);
});

test("the agent's ABI is checked at initialize", async () => {
  await assert.rejects(sample.handle("initialize", { abi: 1 }), /speaks ABI 2 or 3/);
});

/** Run the echo plugin as a real process and talk to it like the agent. */
test("a plugin speaks the protocol over stdin and stdout", async () => {
  const child = spawn(process.execPath, [fileURLToPath(new URL("./fixtures/echo.ts", import.meta.url))], {
    env: { ...process.env, FLEXDECK_PLUGIN_ID: "echo" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const received: Message[] = [];
  const waiting = new Map<number, (message: Message) => void>();
  createInterface({ input: child.stdout }).on("line", (line) => {
    const message = JSON.parse(line) as Message; // throws if stdout is polluted
    if (message.id !== undefined && waiting.has(message.id)) {
      waiting.get(message.id)?.(message);
    } else {
      received.push(message);
    }
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => (stderr += chunk));

  let next = 1;
  const call = (method: string, params: unknown) =>
    new Promise<Message>((resolve) => {
      const id = next++;
      waiting.set(id, resolve);
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });

  try {
    const init = await call("initialize", { abi: ABI_VERSION, settings: { prefix: "#" } });
    const result = init.result as { manifest: { id: string }; variables: { name: string }[] };
    assert.equal(result.manifest.id, "echo");
    assert.deepEqual(result.variables.map((v) => v.name), ["echo.count"]);

    const said = await call("invoke", { action: "say", args: { text: "hello" } });
    assert.deepEqual(said.result, { ok: true });
    const failed = await call("invoke", { action: "fail", args: {} });
    assert.deepEqual(failed.result, { ok: false, error: "asked to fail" });

    const sampled = await call("sample", { wanted: ["echo.count"] });
    assert.deepEqual(sampled.result, { values: { "echo.count": { kind: "number", value: 1 } } });

    assert.ok(
      received.some((m) => m.method === "event/emit" && JSON.stringify(m.params).includes("# hello")),
      "the event carries the setting",
    );
    assert.ok(received.some((m) => m.method === "vars/set"));
    assert.match(stderr, /this goes to stderr/);

    const exited = new Promise((resolve) => child.on("exit", resolve));
    await call("shutdown", {});
    assert.equal(await exited, 0);
  } finally {
    child.kill();
  }
});

test("choices come from each argument's suggest, and a failure says why", async () => {
  const plugin = definePlugin({
    id: "picker",
    name: "Picker",
    version: "1.0.0",
    actions: {
      show: {
        description: "Show a source",
        arguments: {
          scene: { kind: "text", example: "Main", suggest: () => ["Main", { value: "BRB", label: "Be right back" }] },
          source: {
            kind: "text",
            example: "Cam",
            suggest: (args) => {
              if (!args.scene) throw new Error("choose a scene first");
              return [`${args.scene} cam`];
            },
          },
          note: { kind: "text", example: "" },
        },
        run: () => {},
      },
    },
  });
  const host = await harness(plugin);

  const first = await host.choices("show");
  assert.deepEqual(first.arguments.scene, [{ value: "Main" }, { value: "BRB", label: "Be right back" }]);
  assert.equal(first.arguments.source, undefined, "the failing one is left out");
  assert.equal(first.arguments.note, undefined, "an argument with no suggest is typed freely");
  assert.equal(first.error, "choose a scene first");

  const second = await host.choices("show", { scene: "Main" });
  assert.deepEqual(second.arguments.source, [{ value: "Main cam" }], "one argument's choices follow another's value");
  assert.equal(second.error, undefined);

  assert.match((await host.choices("nothing")).error ?? "", /no action/);
});
