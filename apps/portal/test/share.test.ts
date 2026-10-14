import assert from "node:assert/strict";
import { test } from "node:test";

import type { DataSource, WireConfig } from "../src/services/api/api.ts";
import { exportPage, importBundle, parseBundle, picturesOf, reviewBundle, sourcesOf } from "../src/modules/config/lib/share.lib.ts";

const SHA = "ab".repeat(32);
const run = (plugin: string, action: string, arg = "") => ({ block: "doAction" as const, plugin, action, args: (arg ? { a: { text: arg } } : {}) as Record<string, { text: string }> });

const config: WireConfig = {
  schema: 4,
  rev: 3,
  profiles: [
    {
      id: "p",
      pages: [
        { id: "page-1", name: "Home", widgets: [{ id: "widget-1", type: "WIDGET_TYPE_BUTTON", col: 0, row: 0, w: 1, h: 1 }] },
        {
          id: "page-2",
          name: "Stream",
          background: { image: `img:${SHA}` },
          widgets: [
            {
              id: "widget-2",
              type: "WIDGET_TYPE_BUTTON",
              col: 0,
              row: 0,
              w: 1,
              h: 1,
              props: { label: "Go live", labelTemplate: "{{src.btc.price}}" },
              flows: [{ trigger: { gesture: "GESTURE_SHORT_PRESS" }, blocks: [run("launcher", "run", "obs.exe"), run("obs", "start_stream")] }],
              sources: [{ source: "ha", args: { entity: "light.desk" } }],
            },
          ],
        },
      ],
    },
  ],
};

const yours: DataSource[] = [
  { id: "btc", name: "Bitcoin", http: { url: "https://api.coingecko.com/x" }, fields: [] },
  { id: "ha", name: "Home", lan: true, http: { url: "http://homeassistant.local:8123/api/states/{{args.entity}}" }, args: [{ name: "token", secret: true, default: "should-not-leave" }], fields: [] },
  { id: "unused", http: { url: "https://x" }, fields: [] },
];

test("a page exports with what it shows and reads, and no secret", () => {
  const page = config.profiles![0]!.pages![1]!;
  assert.deepEqual(picturesOf(page), [SHA]);
  assert.deepEqual(sourcesOf(page), ["btc", "ha"]);
  const bundle = exportPage(config.profiles![0]!.pages![1], 4, yours)!;
  assert.equal(bundle.fddeck, 1);
  assert.deepEqual(bundle.sources?.map((s) => s.id), ["btc", "ha"]);
  assert.ok(!JSON.stringify(bundle).includes("should-not-leave"));
  assert.equal(exportPage(undefined, 4, yours), null);
});

test("an import is reviewed: what it runs, what it lacks, what it renames", () => {
  const bundle = parseBundle(JSON.stringify(exportPage(config.profiles![0]!.pages![1], 4, yours)));
  const review = reviewBundle(bundle, [], yours.slice(0, 1));
  assert.equal(review.keys, 1);
  assert.deepEqual(review.missing, ["obs"]);
  assert.ok(review.concerns.some((c) => c.severe && c.what === "runs a program: obs.exe"));
  assert.ok(review.concerns.some((c) => !c.severe && c.what.includes("obs plugin")));
  assert.ok(review.concerns.some((c) => c.severe && c.what.includes("homeassistant.local:8123") && c.what.includes("network")));
  assert.deepEqual(review.sources.map((s) => s.renamed ?? null), ["btc-2", null]);

  assert.throws(() => parseBundle("{"), /not JSON/);
  assert.throws(() => parseBundle(JSON.stringify({ hello: 1 })), /not a FlexDeck page/);
  assert.throws(() => parseBundle(JSON.stringify({ fddeck: 9, page: {} })), /newer/);
});

test("an import that points a source at an address of its own says where", () => {
  const bundle = parseBundle(
    JSON.stringify({
      fddeck: 1,
      schema: 4,
      page: {
        id: "page-1",
        name: "Lights",
        widgets: [
          {
            id: "widget-1",
            type: "WIDGET_TYPE_BUTTON",
            col: 0,
            row: 0,
            w: 1,
            h: 1,
            props: { label: "Desk" },
            sources: [{ source: "home-assistant", args: { url: "https://ha.attacker.example/", entity: "light.desk" } }],
          },
        ],
      },
    }),
  );
  const review = reviewBundle(bundle, [], []);
  assert.deepEqual(review.concerns, [{ where: "Desk", what: "points home-assistant at ha.attacker.example", severe: true }]);
});

test("an imported page gets fresh ids, its sources renamed where taken, its pictures re-pointed", () => {
  const bundle = exportPage(config.profiles![0]!.pages![1], 4, yours)!;
  const kept = `img:${"cd".repeat(32)}`;
  const { config: next, sources, pageId } = importBundle(config, "p", bundle, yours.slice(0, 1), { [SHA]: kept });
  assert.equal(pageId, "page-3");
  const page = next.profiles![0]!.pages![2]!;
  assert.equal(page.widgets![0]!.id, "widget-3");
  assert.equal(page.background?.image, kept);
  assert.equal(page.widgets![0]!.props?.labelTemplate, "{{src.btc-2.price}}");
  assert.deepEqual(sources.map((s) => s.id), ["btc", "btc-2", "ha"]);
  assert.throws(() => importBundle(config, "nobody", bundle, yours), /no such profile/);
});
