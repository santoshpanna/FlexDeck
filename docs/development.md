# Development

> Part of the [flexdeck docs](README.md). How the pieces fit is in
> [architecture.md](architecture.md); this page is for building and testing them.

## Requirements

| Tool | Version | Needed for |
| --- | --- | --- |
| Node | ≥ 22 (24 recommended) | the portal, the desktop window's UI, the plugin SDK |
| pnpm | 10 (pinned via `packageManager`) | the workspace |
| Rust | ≥ 1.88 | the agent, the desktop app, the cloud relay |
| JDK | 17 | the Android app and the Kotlin suites (Gradle itself comes from the wrapper) |
| Android SDK | platform 34, build-tools 34 | `verify:android` only |

```sh
git clone https://github.com/santoshpanna/FlexDeck.git flexdeck
cd flexdeck
pnpm install
pnpm dev:desktop     # the desktop app, with its window
```

## Commands

```
pnpm start                          # build the portal, then run the cloud service
pnpm verify                         # every gate: Node, Rust, Android
pnpm verify:node                    # typecheck + tests for the TypeScript packages
pnpm verify:rust                    # fmt, clippy, tests for the Rust workspace
pnpm verify:android                 # the app plus the pure-JVM Kotlin tests
pnpm dev:web                        # the portal UI AND the cloud API behind it
pnpm dev:desktop                     # the desktop app (opens the window)
pnpm dev:agent                       # the headless agent, no window
pnpm desktop:dev                    # just the window's UI, on 127.0.0.1:5174
pnpm desktop:app                    # just the window (UI must already be running)
pnpm desktop:build                  # build the window's UI bundle
pnpm brand:icons                    # regenerate every icon and favicon from the [f] mark
pnpm verify:plugins                 # the plugin SDK, template, OBS and the host
pnpm create-plugin <id> "<Name>"    # start a plugin from the template
pnpm plugins:package                # build every plugin into a .fdpkg
```

Run `pnpm install` once from the root. Skipping it is not obvious: the TypeScript
tests pass without it, because Node strips types natively — only `typecheck`
needs the real `tsc`, so the failure looks like a missing gate rather than a
missing install.

`verify:android` needs `ANDROID_HOME`, because the SDK is deliberately not
committed. Fetch it once with:

```
sdkmanager --sdk_root=.tools/android-sdk "platform-tools" "platforms;android-34" "build-tools;34.0.0"
```

The `core/grid` and `core/client` modules are pure Kotlin/JVM precisely so the
logic they hold stays testable without the SDK installed.

## Verifying

Requirements: **Node ≥ 22** (24 recommended), **Rust ≥ 1.88**. A **JDK 17** is
needed only for the Kotlin suite — the Gradle wrapper fetches Gradle itself.

```sh
# TypeScript — correctness + parity
node --test packages/grid/test/grid.test.ts packages/grid/test/parity.test.ts

# Rust — format, lints, tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Kotlin
cd android && ./gradlew :core:grid:test
```

### Changing the layout algorithm

If you change the grid algorithm on purpose:

```sh
pnpm gen:expected      # regenerate the golden fixtures
git diff contract/fixtures/grid
```

The diff **is** the point — it shows exactly which rectangles moved, so an
accidental parity break becomes a visible, reviewable change instead of silent
one-pixel drift between the preview and the tablet.

Never hand-edit an `*.expected.json`.

## The desktop window

The agent still runs, exactly as before — the window is a *view* of it, not a
second program. `flexdeck-desktop` assembles the same agent (the sequence lives
in `flexdeck-agent`, so the two cannot drift), serves it, and adds a small window
and a tray icon. Closing the window hides it; the agent keeps serving tablets.

It shows four things: whether this computer is reachable (the server, web access,
and the failures that only appear from a tablet), a pairing code with a countdown,
the paired tablets with a live connected/not-connected state, and the actions this
build can run.

The window reads state over Tauri's own IPC rather than the agent's HTTPS API.
That is deliberate: the API needs a bearer token and presents a self-signed
certificate, and neither belongs in a webview — the window is the machine's own
interface, so it holds the agent handle directly. It is also why the window can
report *enabled but not connected* honestly, which the agent's own code already
insists on.

`pnpm dev:desktop` is the one command: it starts the window's UI, waits for it to
answer, opens the window, and stops the dev server when you quit. The UI is on
port **5174**, not the portal's 5173, so both can run at once.

The Tauri CLI is not required. A debug build loads `devUrl`, so `cargo run` is
enough; importing `@tauri-apps/cli` would only rename these steps to `tauri dev`.

One agent per port: `dev:desktop` and `dev:agent` both serve the tablets, so run
one or the other. `dev:desktop` checks the port first and says so rather than
opening a window that cannot reach anything.

Two checks the ordinary suites cannot make:

* **The portal and the agent are only joined by hand.**
  `apps/portal/scripts/roundtrip.check.ts` drives a *running* agent, applies edits
  through the portal's editing model, and asserts the widget, its label, its
  position and the revision all survive — plus undo and redo. Run it after
  changing either side: `pnpm --filter @flexdeck/portal check:roundtrip
  <base-url> <identity> <desktop-id>`. It needs a live cloud, desktop and tunnel.
* **The agent binary needs rebuilding before a manual test.** `cargo test` does
  not refresh `target/debug`, and a failed build leaves the previous binary in
  place, so a smoke test can silently exercise stale code.

Startup is: load `Settings` from the store, then call
`Agent::startup_warnings(settings.bind())`. Between them they cover the failures
that all look fine from the agent's own machine — a QR containing a loopback
address, an agent advertising an address it is not listening on, and presses that
would do nothing because no live executor is installed. They are warnings rather
than errors, because same-machine testing is legitimate, so the caller decides
what to show.

## Releasing and deploying

Everything after a push to `main` is one workflow,
`.github/workflows/pipeline.yml`, which runs the changelog first and then calls
one workflow per target. Each target workflow can also be run by hand from the
Actions tab.

| Stage | Workflow | Runs | Goes to |
| --- | --- | --- | --- |
| Changelog | `pipeline.yml` | every push | `CHANGELOG.md` + `releases.json`, committed back; release tags |
| Web | `web.yml` | every push | Vercel: landing, docs, changelog, download at `flex-deck.com` |
| Server | `server.yml` | when cloud or portal code changed, and on releases | Docker Hub image → Easypanel: editor + API at `app.flex-deck.com` |
| Desktop | `desktop.yml` | release commits | Windows installer (NSIS) |
| Android | `android.yml` | release commits | signed release APK |
| Publish | `pipeline.yml` | release commits | GitHub release with both files, `SHA256SUMS.txt` and notes |

The public pages and the editor are deployed apart because the editor and the
cloud API must share an origin (see `crates/flexdeck-cloud/src/portal.rs`). The
Vercel build is told the editor's address (`VITE_EDITOR_URL`), and
`flex-deck.com/config` redirects there. The cloud image is built with
`VITE_EDITOR_ONLY=true`, so `app.flex-deck.com/` is the editor itself.

### The changelog comes from commits

Every `feat`, `fix`, `perf` or `sec` line in a commit, subject or body, is a
public changelog entry, so write those lines for users. The scope picks the part
of the product: `desktop`, `android`, `portal`, `ui`, `plugin`, or a plugin's id.
`chore`, `ci`, `test`, `docs` and `refactor` lines stay off the page. The mapping
lives in `scripts/changelog/changelog.lib.ts`.

A release is cut by a commit with a `release:` line. Its subject is the release
title, and its untyped lines are the blurb:

```text
First public release
release: v0.1.0
The tablet in your drawer, turned into a control surface for your PC.
feat(desktop): pair with six digits
```

Pushing that builds both apps, tags the commit and publishes the release. If a
build fails, fix it and run *Pipeline* again by hand: a version that has a tag
but no GitHub release is built again. Run `pnpm changelog` to preview locally.
A pre-release suffix such as `release: v0.1.0-rc.1` works for test releases; the
Windows installer carries the plain number, since NSIS rejects the suffix.

### One-time setup

| Where | Name | What |
| --- | --- | --- |
| Repository secret | `VERCEL_TOKEN`, `VERCEL_ORG_ID`, `VERCEL_PROJECT_ID` | A Vercel token and the project's ids (`vercel link` writes them to `.vercel/project.json`) |
| Repository variable | `EDITOR_URL` | The cloud's origin; defaults to `https://app.flex-deck.com` |
| Repository variable | `DOCKERHUB_USERNAME` | The Docker Hub account the image is pushed to |
| Repository secret | `DOCKERHUB_TOKEN` | A Docker Hub access token with read and write access |
| Repository secret | `EASYPANEL_DEPLOY_WEBHOOK` | The Easypanel service's deploy webhook URL |
| Repository secret | `ANDROID_KEYSTORE_BASE64` | The release keystore, `base64 -w0 release.jks` |
| Repository secret | `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` | Its passwords and alias |

The Easypanel service uses the Docker image `<DOCKERHUB_USERNAME>/flexdeck-cloud:latest`
on port `8787`, with a volume at `/data` and the domain `app.flex-deck.com`. The
Docker Hub repository can be public (the source is public anyway). If you make
it private, give Easypanel a read-only Docker Hub access token.

Keep the Android keystore safe forever. Every update must be signed with the same
key, or Android refuses to install it over the old version.

## Build status

**Phase C complete — the agent builds, runs, pairs, and serves.**

Built and verified:

| Piece | State |
| --- | --- |
| Monorepo (pnpm workspaces + Cargo workspace + Gradle wrapper) | ✅ |
| `proto3` contract for config, events and pairing | ✅ |
| Proto codegen → Rust types + proto3 JSON mapping | ✅ 8 tests |
| Shared grid algorithm — TypeScript reference | ✅ 10 unit tests |
| Shared grid algorithm — Rust | ✅ 9 unit tests |
| Shared grid algorithm — Kotlin | ✅ 2 tests |
| Golden fixtures + cross-renderer parity tests | ✅ 5 fixtures, 3 languages |
| Agent config store (SQLite: revision log, undo/redo, JSON-Patch diff) | ✅ 10 tests |
| Action engine (hotkey parsing + desktop execution) | ✅ 37 tests |
| Pairing (self-signed identity, PIN/QR windows, device tokens) | ✅ 28 tests |
| Agent sync core (catch-up policy + patch wire round trip) | ✅ 11 tests |
| Agent REST API (config, undo/redo, devices, pairing) | ✅ 15 tests |
| Agent WebSocket hub + end-to-end socket tests | ✅ 23 tests |
| LAN address discovery + startup checks | ✅ 24 tests |
| Agent settings persistence (port, bind address, host override) | ✅ 10 tests |
| Agent binary (`flexdeck`) — runs, seeds, pairs, persists its identity | ✅ 7 tests |
| REST authentication (admin token + device tokens, per route) | ✅ 5 tests |
| Portal static serving (SPA fallback + an explanatory page when unbuilt) | ✅ 5 tests |
| Portal app (React: same-PC connect, code pairing, status, devices, grid preview) | ✅ |
| Portal editor (move, resize, add, delete, label, undo/redo) | ✅ 20 tests |
| Action catalogue, served by the agent and driving the portal's form | ✅ 8 tests |
| Portal action editing (attach / change / clear a press action) | ✅ 6 tests |
| Android client core (JSON Patch, wire frames, session, reconnect) | ✅ 51 tests |
| Android app (pairing, canvas deck renderer, TLS pinning) | ✅ builds, 1.6 MB APK |
| Desktop window (Tauri: status, pairing, live device presence, plugins) | ✅ builds, 131 + 12 Rust, 26 TS tests |
| Plugin registry + built-ins (core, system, media, launcher, notifications) | ✅ 21 tests, live Windows audio/CPU backend |
| Plugin contract (ABI 2: manifest, JSON-RPC messages, `plugin.json`) | ✅ 15 tests, pinned to the SDK by shared fixtures |
| Plugin host (sidecar processes: deadlines, crash restarts, namespace enforcement) | ✅ 18 tests |
| Install / update / uninstall / hot reload, without restarting the agent | ✅ 13 end-to-end tests, incl. real Node |
| Plugin repositories (official, vetted, custom; pinned checksums, offline cache) | ✅ |
| Plugin SDK + template (`pnpm create-plugin`) | ✅ 9 + 3 tests |
| OBS Studio plugin (scenes, record, stream, mute, sources; live state) | ✅ 14 tests against an in-memory OBS |
| Plugin-event and schedule triggers; buttons bound to live values | ✅ agent, portal and tablet |
| CI (format, lint, tests, fixture-drift check) | ✅ |

The plugin system is built — see [plugins.md](plugins.md) for using, writing and
publishing plugins. The desktop window is built — see *The desktop window* below.
