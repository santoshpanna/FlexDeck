#!/usr/bin/env node
/**
 * `flexdeck-plugin` — the authoring commands.
 *
 *   flexdeck-plugin build [dir]   bundle src/index.ts into the entry
 *   flexdeck-plugin check [dir]   build, then compare the code with plugin.json
 *   flexdeck-plugin pack  [dir]   build, check, and write <id>-<version>.fdpkg
 *   flexdeck-plugin verify [dir] [--run-actions]
 *                                 pack, then ask FlexDeck itself whether it will
 *                                 work: run as the app runs it, checked at every step
 *   flexdeck-plugin repo  <dir>   write repository.min.json for the .fdpkg files in <dir>
 *       [--out dir] [--name "…"] [--description "…"] [--website url] [--icons dir]
 *       [--icon-packs dir]
 *
 * To run a plugin while writing it, link its folder from the flexdeck window
 * (Plugins → Link a folder) or start the agent with
 * `FLEXDECK_DEV_PLUGINS=<dir>`; it reloads on every save.
 */

import { bundle, pack, readPackage, verify } from "./pack.ts";
import { conform, describe, findFlexdeck, passed } from "./conformance.ts";
import { buildRepository } from "./repo.ts";

/** `--name value` pairs after the positional arguments. */
function flags(args: string[]): Record<string, string> {
  const result: Record<string, string> = {};
  for (let index = 0; index < args.length; index++) {
    const arg = args[index]!;
    if (!arg.startsWith("--")) continue;
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`${arg} needs a value`);
    result[arg.slice(2)] = value;
    index++;
  }
  return result;
}

async function main(): Promise<number> {
  const [command = "help", directory = ".", ...rest] = process.argv.slice(2);
  switch (command) {
    case "build": {
      const manifest = readPackage(directory);
      await bundle(directory, manifest);
      console.log(`built ${manifest.entry}`);
      return 0;
    }
    case "check": {
      const manifest = readPackage(directory);
      await bundle(directory, manifest);
      await verify(directory, manifest);
      console.log(`${manifest.id} ${manifest.version}: plugin.json and the code agree`);
      return 0;
    }
    case "pack": {
      const result = await pack(directory);
      console.log(`packed ${result.file}`);
      console.log(`  ${result.size} bytes`);
      console.log(`  sha256 ${result.sha256}`);
      return 0;
    }
    case "verify": {
      const folder = directory.startsWith("--") ? "." : directory;
      const flexdeck = findFlexdeck(folder);
      if (!flexdeck) {
        console.error("verify asks FlexDeck itself, and none was found. Install the FlexDeck app, or set FLEXDECK_BIN to its executable.");
        return 1;
      }
      const result = await pack(folder);
      const report = conform(result.file, { flexdeck, runActions: process.argv.includes("--run-actions") });
      console.log(describe(report));
      return passed(report) ? 0 : 1;
    }
    case "repo": {
      const options = flags(rest);
      const index = buildRepository(directory, {
        out: options.out,
        name: options.name,
        description: options.description,
        website: options.website,
        icons: options.icons,
        iconPacks: options["icon-packs"],
      });
      console.log(`wrote repository.min.json: ${index.repo.name}, ${index.plugins.length} plugins`);
      for (const plugin of index.plugins) console.log(`  ${plugin.id} ${plugin.version}  sha256 ${plugin.package.sha256}`);
      for (const pack of index.iconPacks ?? []) console.log(`  icon pack ${pack.id} ${pack.version}  sha256 ${pack.package.sha256}`);
      return 0;
    }
    default:
      console.log("usage: flexdeck-plugin <build|check|pack|verify|repo> [dir]");
      return command === "help" ? 0 : 1;
  }
}

main().then(
  (code) => process.exit(code),
  (error: unknown) => {
    console.error(`flexdeck-plugin: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  },
);
