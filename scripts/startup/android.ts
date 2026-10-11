/**
 * Development: the Android client.
 *
 * Sets up the JDK and the Android SDK, then builds and installs the debug APK on
 * the attached tablet over adb.
 *
 * The default task installs. To build without a device attached:
 *
 *   pnpm dev:android -- :app:assembleDebug
 *
 * Gradle is reached through `scripts/gradle.mjs` rather than directly, so the
 * wrapper choice (`gradlew.bat` or `gradlew`) and the readable missing-SDK
 * message stay in one place instead of being duplicated here.
 */
import { androidEnvironment, run } from "./env.ts";

// `installDebug` needs a device, so it is only the default and not the only
// option: checking that the app still compiles on a machine with no tablet
// attached is just as much a `dev:android`.
const tasks = process.argv.slice(2);
if (tasks.length === 0) tasks.push(":app:installDebug");

run(process.execPath, ["scripts/gradle.mjs", ...tasks], { env: androidEnvironment() });
