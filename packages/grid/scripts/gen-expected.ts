/**
 * Regenerate the golden grid fixtures from the TypeScript reference
 * implementation.
 *
 *   pnpm gen:expected
 *
 * Run this ONLY when you intend to change the layout algorithm. The resulting
 * `git diff` shows exactly which rectangles moved, which is the point: an
 * accidental parity break becomes a visible, reviewable change instead of a
 * silent one-pixel drift between the preview and the tablet.
 */
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { computeGrid, layoutPage, orientPage, type GridSpec, type WidgetSpan } from "../src/index.ts";

const FIXTURES_DIR = fileURLToPath(new URL("../../../contract/fixtures/grid/", import.meta.url));

interface Fixture {
  name: string;
  spec: GridSpec;
  widgets: WidgetSpan[];
}

/// The expectation shape, for any oriented view of a fixture.
function expectationFor(name: string, spec: GridSpec, widgets: WidgetSpan[]) {
  const geometry = computeGrid(spec);
  return {
    name,
    cells: {
      x: geometry.cols.offsets,
      w: geometry.cols.sizes,
      y: geometry.rows.offsets,
      h: geometry.rows.sizes,
    },
    rects: layoutPage(spec, widgets),
  };
}

function write(name: string, expectation: unknown): void {
  writeFileSync(join(FIXTURES_DIR, name), `${JSON.stringify(expectation, null, 2)}\n`, "utf8");
  console.log(`wrote ${name}`);
}

const files = readdirSync(FIXTURES_DIR)
  .filter((name) => name.endsWith(".json") && !name.endsWith(".expected.json"))
  .sort();

if (files.length === 0) {
  throw new Error(`no fixtures found in ${FIXTURES_DIR}`);
}

for (const file of files) {
  const fixture = JSON.parse(readFileSync(join(FIXTURES_DIR, file), "utf8")) as Fixture;
  const stem = file.replace(/\.json$/, "");

  // Landscape is the AUTHORED orientation, so its expectation is the fixture
  // itself laid out directly.
  write(`${stem}.expected.json`, expectationFor(fixture.name, fixture.spec, fixture.widgets));

  // Portrait is DERIVED, so its golden is generated through the transform rather
  // than being written out by hand: it is a record of what `orientPage` produces,
  // and all three implementations are checked against it. Writing it by hand
  // would let a broken transform agree with itself.
  const portrait = orientPage(fixture.spec, fixture.widgets, "portrait");
  write(
    `${stem}.portrait.expected.json`,
    expectationFor(`${fixture.name} (portrait)`, portrait.spec, portrait.widgets),
  );
}

console.log(
  `\n${files.length} fixture(s) regenerated in both orientations from the TypeScript reference implementation.`,
);
