/**
 * Development: the Android client.
 *
 * Sets up the JDK and the Android SDK, then builds and installs the debug APK on
 * the attached tablet over adb, and opens it there.
 *
 * The default task installs. To build without a device attached:
 *
 *   pnpm dev:android -- :app:assembleDebug
 *
 * Gradle is reached through `scripts/gradle.mjs` rather than directly, so the
 * wrapper choice (`gradlew.bat` or `gradlew`) and the readable missing-SDK
 * message stay in one place instead of being duplicated here.
 */
import { spawn, spawnSync } from "node:child_process";
import { join } from "node:path";

import { androidEnvironment, fail, repoRoot } from "./env.ts";

/** The debug build's package (`applicationIdSuffix = ".dev"`) and its launcher activity. */
const DEBUG_ACTIVITY = "dev.flexdeck.dev/dev.flexdeck.app.MainActivity";

// `installDebug` needs a device, so it is only the default and not the only
// option: checking that the app still compiles on a machine with no tablet
// attached is just as much a `dev:android`.
const tasks = process.argv.slice(2);
const installing = tasks.length === 0;
if (installing) tasks.push(":app:installDebug");

const env = androidEnvironment();
const gradle = spawn(process.execPath, ["scripts/gradle.mjs", ...tasks], { cwd: repoRoot, stdio: "inherit", env });

gradle.on("error", (error) => fail(`Could not start Gradle: ${error.message}`));
gradle.on("exit", (code, signal) => {
  // A signal death carries no exit code, and reporting success for it would
  // let a killed build pass a `&&` chain.
  const status = signal === null ? (code ?? 1) : 1;
  if (status === 0 && installing) launch();
  process.exitCode = status;
});

/**
 * Open the freshly installed app on every attached device — `installDebug`
 * installed it on all of them. Best effort: a launch that fails is reported,
 * but the install itself succeeded, so the exit status stays clean.
 */
function launch(): void {
  const adb = join(env.ANDROID_HOME ?? "", "platform-tools", process.platform === "win32" ? "adb.exe" : "adb");
  const listed = spawnSync(adb, ["devices"], { encoding: "utf8" });
  const serials = (listed.stdout ?? "")
    .split(/\r?\n/)
    .slice(1)
    .map((line) => line.trim().split(/\s+/))
    .filter(([serial, state]) => serial && state === "device")
    .map(([serial]) => serial!);

  for (const serial of serials) {
    const started = spawnSync(adb, ["-s", serial, "shell", "am", "start", "-n", DEBUG_ACTIVITY], { encoding: "utf8" });
    const output = `${started.stdout ?? ""}${started.stderr ?? ""}`;
    if (started.status === 0 && !/Error/.test(output)) {
      console.log(`Opened flexdeck dev on ${serial}.`);
    } else {
      console.warn(`Installed, but could not open the app on ${serial}:\n${output.trim()}`);
    }
  }
}
