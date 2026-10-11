# flexdeck

A programmable button-deck control surface for old Android tablets.

A **desktop app** owns the configuration and executes actions on the PC. A
**native Android client** renders the deck and sends presses. A **web portal**
configures the desktop from a browser — with **no account and no sign-in**.

```
 Browser on the same PC ──http://127.0.0.1──────────────────────► Desktop app ◄──LAN WSS── Android client(s)
 Browser elsewhere ──HTTPS──► Cloud (relay) ◄──OUTBOUND WSS──────►    owns config, runs actions
                              no accounts, holds no credentials
```

* **On the same computer** the portal talks straight to the desktop app on
  loopback, the way a keyboard vendor's web configurator talks to the keyboard on
  your desk. It connects by itself.
* **From another device**, the desktop app shows a code under *Browsers →
  Connect a browser*; typing it into the portal pairs that browser. The desktop
  issues and checks the browser's credential; the cloud only forwards traffic.

The desktop dials **out** to the cloud and nothing dials in. That is why there is
no inbound port, no NAT rule and no port forward — and why the cloud never needs
the desktop's local address.

---

## Status

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

The plugin system is built — see [docs/plugins.md](docs/plugins.md) for using, writing and
publishing plugins. The desktop window is built — see *The desktop window* below.

### Commands

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

### The desktop window

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

---

## Security notes

The agent serves **HTTPS**, using the self-signed identity it already had for
pairing. This is not decoration: every API call carries `Authorization: Bearer
<token>` in a header, and over plain HTTP that token was readable by anything on
the network — which would have undone the authentication below entirely.

Clients pin the certificate by fingerprint. A browser will warn once about the
self-signed certificate; the Android client records the fingerprint on first
contact and refuses a *changed* one, which is what makes the pin worth having.

Every `/api` route requires a credential, except the pairing redeem routes —
redeeming a code is *how* a client obtains one.

**Where a credential came in decides what it may do.** The agent tags every
request with its *channel* — a typed request extension set by the listener or
the tunnel forwarder, never a header — and each credential is honoured on one
channel only:

| Credential | Accepted on | May |
| --- | --- | --- |
| tablet (PIN/QR pairing) | LAN HTTPS + `/ws` | read the deck, press buttons |
| browser on this PC (auto, on loopback) | `127.0.0.1:<port+1>` only | configure everything |
| browser elsewhere (code pairing) | the cloud relay only | configure; plugin installs and tablet pairing windows need **approval in the desktop window** |
| admin token | LAN + loopback, **never the relay** | everything (CLI) |

The cloud keeps no accounts and holds nothing that can act on a desktop: it
forwards a paired browser's own token, and the agent decides. A desktop proves
its identity to the cloud by **signing a fresh challenge** with its certificate
key, so knowing a desktop's fingerprint is not enough to take over its tunnel.
The loopback listener answers only `Host: 127.0.0.1/localhost` (DNS rebinding)
and only the portal's own origin (CORS, including Chrome's private-network
preflight). Safari may refuse an HTTPS page calling `http://127.0.0.1`; pairing
with a code works there instead. Opening a pairing window requires
the **admin** credential specifically, because its response contains the PIN and
the one-time secret: a paired tablet must not be able to mint windows of its own.
The same goes for anything that changes what code runs: rewriting the layout
(`PUT /api/config`, undo, redo), revoking devices, every plugin write, and the
web-access toggle. A device token can read the deck and press buttons — nothing
more — because a layout write could bind the launcher to any program.

This was not always true. An early probe of a *running* agent found the whole API
unauthenticated, which made the entire pairing ceremony bypassable in two
requests:

```
pairing/begin  -> PIN=955345  secret=9679ebf614bd...
pairing/redeem -> ok=True  tokenLength=64
PUT config     -> SUCCEEDED with no credentials
```

The lock was good; the key was hanging on the outside. Every unit test started
from a client that was already trusted, so none of them could see it.

---

## Repository layout

```
contract/            Single source of truth
  proto/               proto3 schema shared by all three renderers
  fixtures/grid/       Golden grid fixtures — the parity contract
apps/
  agent/               The agent binary (`flexdeck`): loads settings and serves
  desktop/             The desktop app: a Tauri window over the same agent
  portal/              The web configurator (Vite + React), served by the cloud
crates/
  flexdeck-actions/    Action engine: hotkey parsing, desktop execution
  flexdeck-cloud/      Cloud service: portal serving, relay, browser-code routing
  flexdeck-grid/       Grid algorithm + parity test (Rust)
  flexdeck-pairing/    Pairing: TLS identity, PIN/QR windows, device tokens
  flexdeck-plugin/     Plugin registry and the built-in plugins
  flexdeck-plugin-abi/ The plugin contract: manifest, messages, plugin.json
  flexdeck-plugin-host/ Runs an installed plugin as a supervised process
  flexdeck-protocol/   Generated proto3 types + proto3 JSON mapping
  flexdeck-server/     Agent server: sync core, REST API, WebSocket hub
  flexdeck-store/      SQLite config store: revision log, undo/redo, patch diff
  flexdeck-tunnel/     Outbound tunnel: frame codec + request correlation
packages/
  grid/                Grid algorithm + tests (TypeScript reference impl)
  plugin-sdk/          Write plugins in TypeScript: SDK, test harness, packer
plugins/
  template/            The copy-me plugin (`pnpm create-plugin <id>`)
  obs/                 OBS Studio
docs/
  plugins.md           Using, writing and publishing plugins
android/
  app/                 The tablet client: pairing, deck renderer, TLS pinning
  core/grid/           Grid algorithm + parity test (Kotlin, pure JVM)
  core/client/         Wire frames, JSON Patch, session, reconnect (pure JVM)
  gradlew              Gradle wrapper (no global Gradle needed)
.github/workflows/     CI
```

Client-side proto codegen is still absent: only the Rust side is generated (see
`crates/flexdeck-protocol/build.rs`). `buf` is not installed, so `contract/buf.gen.yaml`
is not the working codegen path.

---

## The parity problem

The web configurator draws a live preview; the tablet draws the real thing.
Two renderers, two languages, one layout. If they disagree by a single pixel,
**the preview lies** — the user positions a button against a preview that does
not match the device.

So the grid algorithm is written once as a spec and implemented three times,
and the implementations are held together by golden fixtures:

* `contract/fixtures/grid/*.json` — inputs (grid spec + widget spans)
* `contract/fixtures/grid/*.expected.json` — the exact integer rectangles
  every renderer must produce

The expectations are generated from the TypeScript reference implementation and
then asserted by the Rust and Kotlin suites. Three CI jobs, one contract.

**Rules the algorithm depends on** (documented in `packages/grid/src/index.ts`):

1. Integer arithmetic only — no floats, ever.
2. The gap borders both outer edges *and* sits between cells: `n` cells have
   `n + 1` gaps.
3. Remainder pixels go to the first `remainder` cells, left to right.
4. A spanning widget covers the gaps inside it.

---

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

---

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Type contract | `proto3`, compiled to TS/Rust/Kotlin | One schema; proto3's **JSON mapping is specified**, so field naming stays consistent across languages for free |
| Wire format | JSON + RFC 6902 JSON-Patch | Keeps the PRD's debuggable JSON wire and cheap delta sync; the `.proto` supplies only the types |
| Grid | Integer cells, explicit per-profile layouts | Identical rects are computable in every language; no free-form CSS to drift |
| Android | Native Kotlin, classic Views, minSdk 21 | Old tablets have outdated WebViews and little RAM |
| Grid module | Pure Kotlin/JVM, not Android library | The maths is verifiable in CI without an Android SDK |

---

## Roadmap

* **Phase B** — parity spike: render a fixture in the web preview and on a real
  tablet, compare output.
* **Phase C** *(in progress)* — agent core. Done: SQLite config store (revision
  log, undo/redo, JSON-Patch diff), proto codegen with the proto3 JSON mapping
  pinned by tests, the action engine (hotkey parsing + `enigo` execution),
  pairing (self-signed identity + fingerprint, PIN/QR windows, token hashing),
  the sync core (catch-up policy + patch wire round trip), the REST API (config,
  undo/redo, devices, pairing), and the WebSocket hub (auth handshake,
  welcome/patch streaming, press dispatch), the desktop window, and the plugin
  system (built-in and installed plugins, repositories, sliders, hold gestures,
  live icons; Discord, Streamer.bot, Soundboard, Spotify and OBS plugins).
* **Phase D** — web editor: grid canvas, drag/resize, property panel, live push.
* **Phase E** — Android client: pinned WebSocket, QR/PIN pairing, `DeckView`
  renderer, offline config cache.
* **Phase F** — end-to-end validation and performance budget on 2015-era hardware.

Out of scope for the first slice: mDNS discovery, multi-page, Wayland input
injection.
