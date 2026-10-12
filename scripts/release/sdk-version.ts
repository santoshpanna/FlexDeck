/**
 * Write a release version into one plugin SDK package's manifests, at build time.
 *
 *   node scripts/release/sdk-version.ts js-sdk v0.2.0
 *   node scripts/release/sdk-version.ts create-plugin v0.3.0 v0.2.0
 *
 * prints name=0.2.0 and prerelease=false (for $GITHUB_OUTPUT). The versions
 * committed in the manifests are placeholders; the release line is the truth.
 * A package that depends on another takes that one's version as a third
 * argument (the Pipeline passes the newest release of it):
 *
 *   js-sdk         packages/plugin-sdk
 *   create-plugin  packages/create-flexdeck-plugin, pinning new plugins to the
 *                  js-sdk version given
 *   rust-abi       crates/flexdeck-plugin-abi
 *   rust-sdk       crates/flexdeck-plugin-sdk, depending on the rust-abi
 *                  version given
 *   python-sdk     packages/plugin-sdk-python/pyproject.toml
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

export function parseSdkVersion(tag: string): { name: string; prerelease: boolean } {
  const match = /^v?(\d+\.\d+\.\d+)(-[0-9A-Za-z.-]+)?$/.exec(tag);
  if (!match) throw new Error(`not a release version: ${tag}`);
  return { name: tag.replace(/^v/, ""), prerelease: Boolean(match[2]) };
}

/** `text` with the first match of `pattern` (whose group 1 is the version) set to `version`. */
function replaceVersion(path: string, text: string, pattern: RegExp, version: string): string {
  if (!pattern.test(text)) throw new Error(`${path}: no version matching ${pattern}`);
  return text.replace(pattern, (whole, old: string) => whole.replace(old, version));
}

function edit(relative: string, change: (text: string, path: string) => string) {
  const path = join(root, relative);
  writeFileSync(path, change(readFileSync(path, "utf8"), relative));
}

const packageJson = (fields: Record<string, string>) => (text: string) => `${JSON.stringify({ ...JSON.parse(text), ...fields }, null, 2)}\n`;
const ownVersion = /^version = "([^"]+)"/m;
const abiDependency = /^flexdeck-plugin-abi = \{[^}]*version = "([^"]+)"/m;

export function stamp(sdk: string, version: string, requires?: string) {
  const needs = (dependency: string) => {
    if (!requires) throw new Error(`${sdk} needs the ${dependency} version it is built against: no release(${dependency}) line comes before it, so release ${dependency} first (or pass the version by hand)`);
    return parseSdkVersion(requires).name;
  };
  switch (sdk) {
    case "js-sdk":
      edit("packages/plugin-sdk/package.json", packageJson({ version }));
      return;
    case "create-plugin": {
      const sdkVersion = needs("js-sdk");
      // Its prepack copies the SDK package's version into sdkVersion, so both agree.
      edit("packages/plugin-sdk/package.json", packageJson({ version: sdkVersion }));
      edit("packages/create-flexdeck-plugin/package.json", packageJson({ version, sdkVersion }));
      return;
    }
    case "rust-abi":
      edit("crates/flexdeck-plugin-abi/Cargo.toml", (text, path) => replaceVersion(path, text, ownVersion, version));
      return;
    case "rust-sdk": {
      const abi = needs("rust-abi");
      edit("crates/flexdeck-plugin-abi/Cargo.toml", (text, path) => replaceVersion(path, text, ownVersion, abi));
      edit("crates/flexdeck-plugin-sdk/Cargo.toml", (text, path) => replaceVersion(path, replaceVersion(path, text, ownVersion, version), abiDependency, abi));
      return;
    }
    case "python-sdk":
      // setuptools normalises a pre-release (0.2.0-rc.1 → 0.2.0rc1) for PyPI.
      edit("packages/plugin-sdk-python/pyproject.toml", (text, path) => replaceVersion(path, text, ownVersion, version));
      return;
    default:
      throw new Error(`unknown SDK: ${sdk} (js-sdk, create-plugin, rust-abi, rust-sdk or python-sdk)`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [sdk, tag, requires] = process.argv.slice(2);
  if (!sdk || !tag) {
    console.error("usage: node scripts/release/sdk-version.ts <sdk> <vX.Y.Z> [<dependency vX.Y.Z>]");
    process.exit(1);
  }
  const { name, prerelease } = parseSdkVersion(tag);
  stamp(sdk, name, requires || undefined);
  console.log(`name=${name}`);
  console.log(`prerelease=${prerelease}`);
}
