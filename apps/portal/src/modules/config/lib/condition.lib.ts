const RESERVED = new Set(["and", "or", "not", "true", "false"]);
const NAME = /^[\p{L}_@$][\p{L}\p{N}_@$.]*$/u;
const HOLE_NAME = /^[\p{L}_][\p{L}\p{N}_.\-@$/]*$/u;
const NAME_CHAR = /[\p{L}\p{N}_.@$]/u;

export function isBareName(text: string, known: readonly string[] = []): boolean {
  const name = text.trim();
  if (!name) return false;
  if (known.includes(name)) return true;
  return !RESERVED.has(name) && !name.endsWith(".") && HOLE_NAME.test(name);
}

export function quoteName(name: string): string {
  return NAME.test(name) && !RESERVED.has(name) && !name.endsWith(".") ? name : `\`${name}\``;
}

export function literal(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

const PLACEHOLDER = /^\{\{\s*([^{}\s]+)\s*\}\}/;

export function fillPlaceholders(when: string, args: Record<string, string>): string | null {
  let out = "";
  let i = 0;
  const valueOf = (name: string): string | null => {
    const value = (args[name] ?? "").trim();
    return value && !value.includes("{{") ? value : null;
  };
  while (i < when.length) {
    const c = when[i]!;
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < when.length && when[j] !== c) j += when[j] === "\\" ? 2 : 1;
      const quoted = when.slice(i, j + 1);
      if (quoted.includes("{{")) return null;
      out += quoted;
      i = j + 1;
    } else if (c === "`") {
      const end = when.indexOf("`", i + 1);
      if (end < 0) return null;
      let failed = false;
      const name = when.slice(i + 1, end).replace(/\{\{\s*([^{}\s]+)\s*\}\}/g, (_, arg: string) => {
        const value = valueOf(arg);
        if (value === null || value.includes("`")) failed = true;
        return value ?? "";
      });
      if (failed) return null;
      out += `\`${name}\``;
      i = end + 1;
    } else if (NAME_CHAR.test(c) || when.startsWith("{{", i)) {
      let name = "";
      let filled = false;
      let parts = 0;
      while (i < when.length) {
        const hole = PLACEHOLDER.exec(when.slice(i));
        if (hole) {
          const value = valueOf(hole[1]!);
          if (value === null) return null;
          name += value;
          filled = true;
          parts += 1;
          i += hole[0].length;
        } else if (NAME_CHAR.test(when[i]!)) {
          name += when[i];
          parts += 1;
          i += 1;
        } else {
          break;
        }
      }
      if (parts === 0) {
        out += c;
        i += 1;
      } else if (!filled) out += name;
      else if (parts === 1) out += literal(name);
      else if (name.includes("`")) return null;
      else out += quoteName(name);
    } else {
      out += c;
      i += 1;
    }
  }
  return out;
}
