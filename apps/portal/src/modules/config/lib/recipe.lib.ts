import type { DataSource } from "../../../services/api/api.ts";

/**
 * A data source of your own, packed as a recipe: a plugin with no code, the
 * same kind as the weather and Home Assistant ones, that anyone can install
 * from the Plugins tab. Each key that uses it runs its own copy, with its own
 * settings; secret settings travel blank.
 */

export interface RecipeMeta {
  /** The plugin's id: lowercase letters, digits, - and _, starting with a letter. */
  id: string;
  name: string;
  version?: string;
  description?: string;
  author?: string;
}

/** An id for a recipe made from `source`: its own, made valid. */
export function recipeIdFor(source: DataSource): string {
  const id = source.id
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^[^a-z]+/, "")
    .slice(0, 64);
  return id || "recipe";
}

/** Why `source` cannot become a recipe, or `null` when it can. */
export function recipeProblem(source: DataSource, meta: RecipeMeta): string | null {
  if ((source.kind ?? "http") !== "http") return "Only a source that reads the web can be shared: one that runs a program stays on this computer.";
  if (!source.http?.url) return "Give it an address first.";
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(meta.id)) return "Its id: lowercase letters, digits, - and _, starting with a letter.";
  if (!meta.name.trim()) return "Give it a name.";
  if (meta.version && !/^\d[0-9A-Za-z.+-]*$/.test(meta.version)) return "Its version: like 1.0.0.";
  if ((source.fields ?? []).length === 0) return "Give it at least one field, or a key has nothing to show.";
  return null;
}

/** The two files of the package: `plugin.json` and `recipe.json`, as objects. */
export function recipeFiles(source: DataSource, meta: RecipeMeta): { plugin: Record<string, unknown>; recipe: Record<string, unknown> } {
  const fields = (source.fields ?? []).filter((field) => !field.series);
  const graph = (source.fields ?? []).find((field) => field.series);
  const shared: DataSource = {
    id: meta.id,
    name: meta.name,
    ...(source.everySeconds ? { everySeconds: source.everySeconds } : {}),
    ...(source.lan ? { lan: true } : {}),
    http: source.http,
    args: source.args?.map((arg) => (arg.secret ? { name: arg.name, secret: true, description: arg.description ?? "" } : arg)),
    fields: source.fields ?? [],
  };
  if (!shared.args?.length) delete shared.args;
  const [first, second] = fields;
  const elements: Record<string, unknown>[] = [{ slot: "SLOT_TOP", template: meta.name }];
  if (first) elements.push({ slot: "SLOT_VALUE", template: `{{this.${first.name}}}` });
  if (second) elements.push({ slot: "SLOT_CAPTION", template: `{{this.${second.name}}}` });
  const preset = {
    id: "value",
    name: meta.name,
    category: "Web",
    description: meta.description || `What ${meta.name} reads. Set its settings under Its own data.`,
    w: 1,
    h: 1,
    widget: {
      sources: [{ source: meta.id }],
      face: {
        layout: "FACE_LAYOUT_VALUE",
        ...(graph ? { background: { graph: `this.${graph.name}` } } : {}),
        elements,
      },
    },
  };
  return {
    plugin: {
      abi: 4,
      id: meta.id,
      name: meta.name.trim(),
      version: meta.version || "1.0.0",
      description: meta.description ?? "",
      author: meta.author ?? "",
      runtime: "recipe",
      recipe: "recipe.json",
      permissions: ["network"],
    },
    recipe: { sources: [shared], presets: [preset] },
  };
}

// ---- A zip, stored (no compression) ---------------------------------------------

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = CRC_TABLE[(crc ^ byte) & 0xff]! ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

/** A zip of `files`, stored as they are: small text files need no squeezing. */
export function zip(files: readonly { name: string; bytes: Uint8Array }[]): Uint8Array {
  const encoder = new TextEncoder();
  const parts: Uint8Array[] = [];
  const central: Uint8Array[] = [];
  let offset = 0;
  for (const file of files) {
    const name = encoder.encode(file.name);
    const crc = crc32(file.bytes);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true); // version needed
    local.setUint16(6, 0x0800, true); // names are UTF-8
    local.setUint16(8, 0, true); // stored
    local.setUint32(14, crc, true);
    local.setUint32(18, file.bytes.length, true);
    local.setUint32(22, file.bytes.length, true);
    local.setUint16(26, name.length, true);
    parts.push(new Uint8Array(local.buffer), name, file.bytes);

    const entry = new DataView(new ArrayBuffer(46));
    entry.setUint32(0, 0x02014b50, true);
    entry.setUint16(4, 20, true);
    entry.setUint16(6, 20, true);
    entry.setUint16(8, 0x0800, true);
    entry.setUint32(16, crc, true);
    entry.setUint32(20, file.bytes.length, true);
    entry.setUint32(24, file.bytes.length, true);
    entry.setUint16(28, name.length, true);
    entry.setUint32(42, offset, true);
    central.push(new Uint8Array(entry.buffer), name);
    offset += 30 + name.length + file.bytes.length;
  }
  const size = central.reduce((n, part) => n + part.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, files.length, true);
  end.setUint16(10, files.length, true);
  end.setUint32(12, size, true);
  end.setUint32(16, offset, true);
  const all = [...parts, ...central, new Uint8Array(end.buffer)];
  const out = new Uint8Array(all.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of all) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

/** The installable package (`.fdpkg`) for `source` as a recipe. */
export function packRecipe(source: DataSource, meta: RecipeMeta): Uint8Array {
  const problem = recipeProblem(source, meta);
  if (problem) throw new Error(problem);
  const { plugin, recipe } = recipeFiles(source, meta);
  const encoder = new TextEncoder();
  return zip([
    { name: "plugin.json", bytes: encoder.encode(`${JSON.stringify(plugin, null, 2)}\n`) },
    { name: "recipe.json", bytes: encoder.encode(`${JSON.stringify(recipe, null, 2)}\n`) },
  ]);
}
