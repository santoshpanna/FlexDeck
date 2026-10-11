/**
 * Parity test: the TypeScript output must match the committed golden files
 * exactly.
 *
 * This does not check the algorithm is *correct* (test/grid.test.ts does
 * that). It checks the reference implementation has not drifted from the
 * committed expectations — which is what makes the golden files meaningful as
 * a cross-language contract for Rust and Kotlin.
 */
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { computeGrid, layoutPage, orientPage, type GridSpec, type WidgetSpan } from "../src/index.ts";

const FIXTURES_DIR = fileURLToPath(new URL("../../../contract/fixtures/grid/", import.meta.url));

interface Fixture {
  name: string;
  spec: GridSpec;
  widgets: WidgetSpan[];
}

interface Expected {
  name: string;
  cells: { x: number[]; w: number[]; y: number[]; h: number[] };
  rects: Record<string, { x: number; y: number; w: number; h: number }>;
}

function fixtureFiles(): string[] {
  return readdirSync(FIXTURES_DIR)
    .filter((name) => name.endsWith(".json") && !name.endsWith(".expected.json"))
    .sort();
}

function readJson<T>(file: string): T {
  return JSON.parse(readFileSync(join(FIXTURES_DIR, file), "utf8")) as T;
}

test("there is at least one grid fixture", () => {
  assert.ok(fixtureFiles().length > 0, `no fixtures found in ${FIXTURES_DIR}`);
});

/**
 * Check one oriented view of a fixture against its golden.
 *
 * Factored out because landscape and portrait differ ONLY in which transform is
 * applied first, and a second hand-written copy of these five assertions is
 * exactly how a parity test quietly stops covering one of the two.
 */
function expectParity(label: string, spec: GridSpec, widgets: readonly WidgetSpan[], expected: Expected): void {
  const geometry = computeGrid(spec);
  assert.deepEqual(geometry.cols.offsets, expected.cells.x, `${label}: column offsets`);
  assert.deepEqual(geometry.cols.sizes, expected.cells.w, `${label}: column sizes`);
  assert.deepEqual(geometry.rows.offsets, expected.cells.y, `${label}: row offsets`);
  assert.deepEqual(geometry.rows.sizes, expected.cells.h, `${label}: row sizes`);
  assert.deepEqual(layoutPage(spec, widgets), expected.rects, `${label}: widget rects`);
}

for (const file of fixtureFiles()) {
  test(`parity: ${file}`, () => {
    const fixture = readJson<Fixture>(file);
    const expected = readJson<Expected>(file.replace(/\.json$/, ".expected.json"));
    expectParity(fixture.name, fixture.spec, fixture.widgets, expected);
  });

  // The same fixture, shown on a panel held the other way up: every one of these
  // goldens moves if the transpose is changed, so the transform cannot drift
  // between the preview and the tablet.
  test(`parity: ${file} (portrait)`, () => {
    const fixture = readJson<Fixture>(file);
    const expected = readJson<Expected>(file.replace(/\.json$/, ".portrait.expected.json"));
    const portrait = orientPage(fixture.spec, fixture.widgets, "portrait");
    expectParity(`${fixture.name} (portrait)`, portrait.spec, portrait.widgets, expected);
  });
}
