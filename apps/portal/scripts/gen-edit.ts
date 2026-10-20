/**
 * Write the editing answers the portal gives, where the agent's MCP tests
 * read them:
 *
 *   pnpm gen:edit
 *
 * So a coding agent's edit and the editor's cannot drift apart unnoticed.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { gridSpecOf, withSchemaFor } from "../src/modules/config/lib/config.lib.ts";
import { firstFreeCell, limitsFor, placementProblem, setActiveLight } from "../src/modules/config/lib/editing.lib.ts";
import type { ActionDefinition, WireConfig, WireWidget } from "../src/services/api/api.ts";

export const EDIT_DIR = fileURLToPath(new URL("../../../contract/fixtures/edit/", import.meta.url));

const page = [
  { id: "a", col: 0, row: 0, w: 2, h: 1 },
  { id: "b", col: 3, row: 1, w: 1, h: 2 },
];

export function placementCases() {
  const grid = { cols: 4, rows: 3 };
  const spec = gridSpecOf({ id: "p", grid });
  const candidates: { type?: string; span: { id: string; col: number; row: number; w: number; h: number } }[] = [
    { span: { id: "n", col: 2, row: 0, w: 1, h: 1 } },
    { span: { id: "n", col: 1, row: 0, w: 1, h: 1 } },
    { span: { id: "a", col: 1, row: 0, w: 2, h: 1 } },
    { span: { id: "n", col: 3, row: 0, w: 2, h: 1 } },
    { span: { id: "n", col: -1, row: 0, w: 1, h: 1 } },
    { span: { id: "n", col: 0, row: 2, w: 1, h: 2 } },
    { type: "WIDGET_TYPE_BUTTON", span: { id: "n", col: 0, row: 0, w: 5, h: 1 } },
    { type: "WIDGET_TYPE_SLIDER", span: { id: "n", col: 0, row: 1, w: 1, h: 0 } },
    { type: "WIDGET_TYPE_SLIDER", span: { id: "n", col: 0, row: 0, w: 1, h: 9 } },
    { type: "WIDGET_TYPE_SOMETHING_NEW", span: { id: "n", col: 0, row: 1, w: 3, h: 2 } },
  ];
  const free = [
    [1, 1],
    [2, 1],
    [3, 1],
    [1, 2],
    [4, 1],
    [2, 2],
  ] as const;
  return {
    grid,
    page,
    placements: candidates.map(({ type, span }) => ({
      type: type ?? null,
      span,
      problem: placementProblem(spec, page, span, limitsFor(type)),
    })),
    freeCells: free.map(([w, h]) => ({ w, h, cell: firstFreeCell(spec, page, w, h) })),
  };
}

function deckOf(widgets: WireWidget[], schema = 2): WireConfig {
  return { schema, profiles: [{ id: "p", grid: { cols: 4, rows: 3 }, pages: [{ id: "home", widgets }] }] } as WireConfig;
}

export function schemaCases() {
  const decks: { name: string; deck: WireConfig }[] = [
    { name: "empty", deck: deckOf([]) },
    { name: "plain key", deck: deckOf([{ id: "k", type: "WIDGET_TYPE_BUTTON", col: 0, row: 0, w: 1, h: 1, props: { label: "x" } } as WireWidget]) },
    { name: "hidden dot", deck: deckOf([{ id: "k", props: { hideDot: true } } as WireWidget]) },
    { name: "no auto light", deck: deckOf([{ id: "k", props: { noAutoLight: true } } as WireWidget]) },
    { name: "gesture looks", deck: deckOf([{ id: "k", states: { looks: [{ id: "a" }], gesture: { looks: [] } } } as unknown as WireWidget]) },
    { name: "value rule", deck: deckOf([{ id: "k", states: { looks: [{ id: "a" }], rules: [{ variable: "v", value: "1", state: "a" }] } } as unknown as WireWidget]) },
    { name: "rule with when", deck: deckOf([{ id: "k", states: { looks: [{ id: "a" }], rules: [{ when: "v > 1", state: "a" }] } } as unknown as WireWidget]) },
    { name: "case with when", deck: deckOf([{ id: "k", states: { looks: [{ id: "a" }], variable: { variable: "", cases: [{ when: "v > 1", state: "a" }] } } } as unknown as WireWidget]) },
    { name: "blank when", deck: deckOf([{ id: "k", states: { looks: [{ id: "a" }], variable: { variable: "v", cases: [{ when: "  ", value: "1", state: "a" }] } } } as unknown as WireWidget]) },
    { name: "already newer", deck: deckOf([], 6) },
  ];
  return decks.map(({ name, deck }) => ({ name, deck, schema: withSchemaFor(deck).schema }));
}

const PRESS = "GESTURE_SHORT_PRESS";

function pressing(args: Record<string, unknown>, extra: Partial<WireWidget> = {}): WireWidget {
  return {
    id: "k",
    type: "WIDGET_TYPE_BUTTON",
    col: 0,
    row: 0,
    w: 1,
    h: 1,
    flows: [{ trigger: { gesture: PRESS }, blocks: [{ block: "doAction", plugin: "obs", action: "act", args }] }],
    ...extra,
  } as unknown as WireWidget;
}

export function activeCases() {
  const cases: { name: string; active: ActionDefinition["active"] | null; widget: WireWidget }[] = [
    { name: "variable equals an argument", active: { variable: "obs.scene", equals: "scene" }, widget: pressing({ scene: { text: "Live" } }) },
    { name: "placeholder in the variable", active: { variable: "media.{{app}}.playing" }, widget: pressing({ app: { text: "spotify" } }) },
    { name: "doubled plugin prefix", active: { variable: "vars.{{name}}", is: "on" }, widget: pressing({ name: { text: "vars.focus" } }) },
    { name: "blank argument", active: { variable: "obs.scene", equals: "scene" }, widget: pressing({ scene: { text: "  " } }) },
    { name: "argument read from a variable", active: { variable: "obs.scene", equals: "scene" }, widget: pressing({ scene: { variable: "x.y" } }) },
    { name: "condition", active: { when: "obs.{{input}}.muted == true" }, widget: pressing({ input: { text: "Mic" } }) },
    { name: "condition with a spaced argument", active: { when: "{{input}}.muted" }, widget: pressing({ input: { text: "Mic 2" } }) },
    { name: "condition with a quoted placeholder", active: { when: "'{{input}}' == x" }, widget: pressing({ input: { text: "Mic" } }) },
    {
      name: "looks set by hand",
      active: { variable: "obs.scene", equals: "scene" },
      widget: pressing({ scene: { text: "Live" } }, { states: { looks: [{ id: "a", bgColor: "#f00" }] } } as Partial<WireWidget>),
    },
    {
      name: "lit before, action can no longer tell",
      active: null,
      widget: pressing(
        { scene: { text: "Live" } },
        {
          states: { looks: [{ id: "off" }, { id: "on" }], variable: { variable: "obs.scene", cases: [{ value: "Live", state: "on" }], otherwise: "off" } },
        } as Partial<WireWidget>,
      ),
    },
    {
      name: "no auto light is dropped when lit",
      active: { variable: "obs.recording" },
      widget: pressing({}, { props: { noAutoLight: true } } as Partial<WireWidget>),
    },
  ];
  return cases.map(({ name, active, widget }) => {
    const definition = active ? ({ plugin: "obs", action: "act", description: "", arguments: [], active } as ActionDefinition) : null;
    const press = (widget.flows?.[0]?.blocks?.[0] ?? {}) as { args?: Record<string, Record<string, unknown>> };
    const args = Object.fromEntries(
      Object.entries(press.args ?? {}).map(([key, value]) => [key, "text" in value ? String(value.text) : `{{${String(value.variable)}}}`]),
    );
    const lit = setActiveLight(deckOf([widget]), "k", definition, args);
    return { name, active, widget, lit: lit.profiles?.[0]?.pages?.[0]?.widgets?.[0] ?? null };
  });
}

export function fixtures(): Record<string, unknown> {
  return { "placement.json": placementCases(), "schema.json": schemaCases(), "active.json": activeCases() };
}

export function render(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  mkdirSync(EDIT_DIR, { recursive: true });
  for (const [file, value] of Object.entries(fixtures())) {
    writeFileSync(`${EDIT_DIR}${file}`, render(value));
    console.log(`wrote ${file}`);
  }
}
