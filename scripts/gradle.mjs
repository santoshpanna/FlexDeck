#!/usr/bin/env node

// Runs the Android Gradle wrapper, whichever platform this is.
//
// The wrapper is `gradlew.bat` on Windows and `gradlew` everywhere else, and no
// single command works on both. Putting the choice in a package.json script
// would mean it is silently wrong on one of them; putting it here means it is
// wrong nowhere, and the failure says which file was missing.
//
// It also refuses to run without an SDK, because the alternative is a Gradle
// error about a missing `sdk.dir` several screens long that never mentions that
// the SDK is simply not installed.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const android = fileURLToPath(new URL("../android", import.meta.url));
const wrapper = join(android, process.platform === "win32" ? "gradlew.bat" : "gradlew");

if (!existsSync(wrapper)) {
  console.error(`No Gradle wrapper at ${wrapper}`);
  process.exit(1);
}

if (!process.env.ANDROID_HOME && !process.env.ANDROID_SDK_ROOT) {
  console.error(
    [
      "ANDROID_HOME is not set, so the Android module cannot be built.",
      "The SDK is deliberately not committed. See the README for how to fetch it,",
      "or run one of the other verify:* scripts instead.",
    ].join("\n"),
  );
  process.exit(1);
}

const result = spawnSync(wrapper, process.argv.slice(2), {
  cwd: android,
  stdio: "inherit",
  // Windows will not execute a .bat without a shell.
  shell: process.platform === "win32",
});

process.exit(result.status ?? 1);
