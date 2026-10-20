# Plugins

A plugin adds things a flexdeck deck can do (**actions**), live values it can show
and react to (**variables**), and things that happen that an automation can start
on (**events**). OBS Studio, for example, adds actions like *switch scene*, variables
like `obs.recording`, and events like `scene_changed`.

There are two kinds of plugin, and nothing above the registry can tell them apart:

| | Built in | Installed |
| --- | --- | --- |
| Lives in | `crates/flexdeck-plugin/src/builtin/` | `<data>/plugins/<id>/<version>-<sha>/` |
| Runs as | Rust, inside the agent | its own process (Node or a native binary) |
| Examples | `core`, `system`, `media`, `launcher`, `notifications` | `obs` |
| Added by | a release | the marketplace (a repository), a `.fdpkg` file, or a linked folder |

## Using plugins

**From the window (desktop app).** The *Marketplace* lists every plugin from every
repository this computer reads, as tiles: icon, name, author, version, when it last
changed, where it comes from, and what it may do. Search and the *Official ·
Community · Custom · Updates* chips filter the list. *Other ways to install* takes a
`.fdpkg` file or links a folder you are writing. Each installed plugin has
*Settings*, *Log*, *Restart*, *Turn off* and *Remove*.

**From the portal.** The *Plugins* tab shows the same marketplace and repositories.
Both come from the desktop, and installing goes through the desktop: it downloads
the package itself and checks it against the checksum the repository's index pins.

### Repositories

Plugins come from **repositories**: a `repository.min.json` file anyone can host,
listing each plugin's latest version and where its package is. There are three kinds:

| Kind | What it is | Can be |
| --- | --- | --- |
| Official | flexdeck's own repository, [Questory-Labs/FlexDeck-Plugins](https://github.com/Questory-Labs/FlexDeck-Plugins) | always on |
| Community | third-party repositories flexdeck has reviewed, from a list published next to the official one | turned off |
| Custom | any repository you add by address | removed |

Add one under *Repositories* → *Add a repository by address*. `example.com`,
`example.com/plugins` and a full `…/repository.min.json` address all work; a bare
host or folder means its `repository.min.json`. The desktop reads it first and shows
its name and plugins before anything is saved. A custom repository is marked as not
reviewed, here and again when you install from it.

Only `https://` is accepted, except to this computer (`http://localhost`,
`http://127.0.0.1`). The desktop re-reads repositories every six hours, or when you
press *Refresh*, and keeps the last copy of each, so the marketplace still lists
what it knew while offline. A plugin is only offered updates from the repository it
was installed from.

Nothing installs without its permissions being shown and accepted. The agent
enforces this, not just the UI: an install whose permissions were not all accepted
is refused with `409`, and the refusal lists what to ask for.

| Permission | Means |
| --- | --- |
| `network` | connect to other programs and the internet |
| `notifications` | show desktop notifications |
| `processes` | start other programs |
| `filesystem` | read and write files outside its own folder |

> **Plugins are programs.** An installed plugin runs as a process with your
> account's rights; permissions are a declaration you consent to, not a sandbox.
> What the agent *does* enforce is everything that passes through it: a plugin
> can only publish variables under its own `<id>.` namespace, only emit events it
> declared, only show notifications if it asked, never claim a permission its
> package did not declare, and every call it is asked to make has a deadline.

### Binding a button to a live value

Any button can follow a plugin's variables (portal → select the widget → *Looks*):

* **Light up while** a bool is true — `obs.recording` makes a red record light.
* **Label shows** a template — `REC {{obs.record_time}}`, `{{obs.scene}}`,
  `Vol {{system.audio.volume}}%`.
* **Icon follows** a text variable holding a bundled icon's name or a picture a
  plugin handed over (`img:<sha256>`) — `{{media.current.art}}` shows album art.
* **Colours** set the key's face and text; a lit key keeps the deck's glow.

The built-in `time` plugin makes a clock out of a label: `{{time.hhmm}}`,
`{{time.h12mm}} {{time.ampm}}`, `{{time.weekday}}`, `{{time.date}}`. Its values
are in the desktop's time zone.

The tablet updates the moment the value changes. Values are only kept fresh while
a tablet is connected, so an idle agent polls nothing.

### Automations

Two triggers come from plugins:

* **A plugin announces** — e.g. *OBS: the live scene changed*, optionally only
  when `scene` is `Live`.
* **On a schedule** — every N minutes, or at a time of day on chosen days.

## Writing a plugin

```sh
pnpm create-plugin weather "Weather"     # copies plugins/template
pnpm install
pnpm --filter @flexdeck/plugin-weather test
pnpm --filter @flexdeck/plugin-weather run package   # -> weather-0.1.0.fdpkg
```

`plugins/template/src/index.ts` uses every part of the contract once, with a note
on each. The whole plugin is one `definePlugin` call:

```ts
import { definePlugin } from "@flexdeck/plugin-sdk";

export default definePlugin({
  id: "weather",
  name: "Weather",
  version: "0.1.0",
  permissions: ["network"],
  settings: { city: { kind: "text", default: "London" } },
  variables: { "weather.temperature": { kind: "number", unit: "°C" } },
  events: { rain_started: { fields: ["city"] } },
  actions: {
    refresh: {
      description: "Fetch the weather now",
      run: async (_args, ctx) => {
        const temperature = await fetchTemperature(ctx.settings.city);
        ctx.vars.set({ "weather.temperature": temperature });
      },
    },
  },
  // Asked only for variables something is watching.
  sample: async (wanted, ctx) => ({ "weather.temperature": await fetchTemperature(ctx.settings.city) }),
  start: (ctx) => ctx.log.info("ready"),
});
```

What `ctx` gives an action: `ctx.settings` (with defaults), `ctx.vars.set/declare/get`,
`ctx.emit(event, payload)`, `ctx.notify(title, body)` and `ctx.log.info/warn/error`.
`console.log` is redirected to the plugin's log, so it can never corrupt the
protocol.

### The authoring loop

Link the folder (window → *Get plugins* → *Writing a plugin?*, or start the agent
with `FLEXDECK_DEV_PLUGINS=<dir>`). The agent runs it straight from the folder and
restarts it on every save; build with `pnpm run build` (or a watch) and the deck
picks up the change in about a second.

### Testing without the agent

`@flexdeck/plugin-sdk/testing` drives a plugin exactly as the agent does:

```ts
import { checkDefinition, harness } from "@flexdeck/plugin-sdk/testing";

await checkDefinition(plugin, new URL("../plugin.json", import.meta.url)); // what the agent will accept
const host = await harness(plugin, { settings: { city: "Paris" } });
await host.invoke("refresh");
assert.equal(typeof host.variable("weather.temperature"), "number");
```

The OBS plugin's tests ([`plugins/obs/test/`](https://github.com/Questory-Labs/FlexDeck-Plugins/tree/main/plugins/obs/test)) run the whole plugin against an
in-memory OBS, with no network — a good model for a plugin that talks to another
program.

### Packaging

A `.fdpkg` is a zip with `plugin.json` at its root:

```json
{
  "abi": 3,
  "id": "weather",
  "name": "Weather",
  "version": "0.1.0",
  "runtime": "node",
  "entry": "dist/index.js",
  "permissions": ["network"]
}
```

`flexdeck-plugin pack` bundles `src/index.ts` (SDK included, so no `node_modules`
ships), checks that the code and `plugin.json` agree on id, version and
permissions, and writes a byte-for-byte reproducible zip — the same files always
give the same SHA-256. For a plugin written in another language, use
`"runtime": "native"` with `"entries": {"windows": "bin/weather.exe", ...}` and
speak the protocol below.

### Host your own repository

A repository is a static folder: `repository.min.json`, the packages, and any icons.
GitHub Pages, a raw GitHub URL, or any web server will do.

```sh
npm run package                                  # -> weather-0.1.0.fdpkg
mkdir -p site/packages && cp *.fdpkg site/packages/
npx flexdeck-plugin repo site/packages --out site --name "My plugins" --icons site/icons
```

`flexdeck-plugin repo` reads each package's `plugin.json`, keeps the highest version
of each plugin, and writes `repository.min.json` (and a readable
`repository.json`). Addresses in it are relative to the index, so the folder can
move. Run it again after adding a package: `releaseDate` stays the day a plugin
first appeared, `latestUpdateDate` moves only when its version does, and a version
that comes back with different bytes is refused. A published version never
changes; publish a new one instead. [`docs/examples/plugin-repo.yml`](examples/plugin-repo.yml)
is a GitHub Actions workflow that does all of this on every push and publishes to
GitHub Pages.

The format (schema 1):

```json
{
  "schema": 1,
  "repo": { "name": "My plugins", "description": "…", "website": "https://…" },
  "plugins": [{
    "id": "weather", "name": "Weather", "version": "0.1.0", "abi": 3,
    "description": "…", "author": "…", "homepage": "https://…",
    "permissions": ["network"],
    "releaseDate": "2026-03-01T00:00:00Z",
    "latestUpdateDate": "2026-09-20T00:00:00Z",
    "icon": "icons/weather.png",
    "icons": { "64": "icons/weather-64.png", "256": "icons/weather-256.png" },
    "package": { "url": "packages/weather-0.1.0.fdpkg", "sha256": "…", "size": 12345 }
  }],
  "iconPacks": [{
    "id": "my-icons", "name": "My icons", "version": "1.0.0",
    "description": "…", "author": "…", "count": 120,
    "package": { "url": "packs/my-icons.zip", "sha256": "…", "size": 54321 }
  }]
}
```

`iconPacks` is optional. Pass `--icon-packs <dir>` to `flexdeck-plugin repo` to list
every `<id>.zip` in that folder; each zip needs a `manifest.json` (`Name`, `Author`,
`Version`), at its root or in an `<id>/` folder, and an `icons.json` beside it gives
the count. The icon picker offers the packs from every enabled repository and
refuses a zip whose SHA-256 differs from its entry. A bad or repeated pack entry is
hidden; it never hides the plugins.

| Field | Notes |
| --- | --- |
| `package.sha256` | The SHA-256 of the `.fdpkg`. The desktop refuses a package whose bytes differ. |
| `package.url`, `icon`, `icons` | Absolute, or relative to the index. |
| `icon`, `icons` | PNG, JPEG or WebP, under 128 KB. SVG is not shown. |
| `releaseDate`, `latestUpdateDate` | RFC 3339. |

The package's own `plugin.json` must agree with its entry on `id` and `version`. An
entry that cannot be read is hidden, and the rest of the repository still lists;
the same `id` twice refuses the whole repository. Fields the desktop does not know
are ignored, so a repository can carry more than this.

**Getting listed as a community repository** means asking for your repository's
address to be added to `repositories.min.json` in the official repository. The
checksum proves a package is the one your index lists, not that the index is
honest, so a review looks at who runs the repository as well as at the plugins.

### Verify before you share

`npm run verify` (TypeScript) or `flexdeck-plugin verify` (Python) packs the plugin and asks the FlexDeck on this computer whether it will work: `flexdeck verify-plugin <folder | .fdpkg>`. The plugin runs as an install would run it, with the same host, deadlines and rules, and each step is checked:
- the package and catalogue are ones the agent accepts;
- every gallery key could be saved in a deck;
- every argument has an example of its kind, and the plugin accepts every action's examples;
- it answers when its values are wanted;
- its feeds start and stop for a key and leave nothing behind;
- it carries on after a settings change;
- it stops when asked.

Throughout, anything the host refuses fails the step it happened in: a value outside its namespace, an event it didn't declare, a notification without the permission, more messages than its budget, or text on stdout that isn't the protocol. `--run-actions` also runs every action with its examples; it's off by default because actions do real things. A recipe is checked the same way: its sources and presets as the agent reads them, the permission it needs, and no secret with a value in the package. Because the SDKs only find FlexDeck and show its answer, the rules can't drift between the app and the SDKs. Set `FLEXDECK_BIN` if FlexDeck isn't installed in the usual place.

### Python plugins

`pip install flexdeck`, then `flexdeck-plugin new <id> "<Name>"`. The SDK uses only the standard library. A packed plugin (`"runtime": "python"`, ABI 4) carries the SDK beside its own files, so it needs only Python 3.10+ on the computer. The agent runs it with `-u -E -s`: unbuffered, with no `PYTHON*` variables and no user site-packages. Set `FLEXDECK_PYTHON` to choose the interpreter.

```python
from flexdeck_plugin import Plugin, arg

plugin = Plugin(id="lights", name="Desk lights", version="0.1.0", permissions=["network"])
plugin.variable("lights.on", "bool", "Whether the lights are on")

@plugin.action("toggle", "Turn the lights on or off", {"room": arg("text", "desk")})
def toggle(args, ctx):
    ctx.vars.set({"lights.on": not ctx.vars.get("lights.on")})

if __name__ == "__main__":
    plugin.serve()
```

`flexdeck_plugin.testing` has `Harness` (the host's side, in process) and `check_definition`.

### What one plugin can't do to another

Everything a plugin sends passes through the host, which enforces these rules:
- **Values:** it may only publish or declare values under its own `<id>.`, including the per-key `<id>.@<key>.` values of its feeds, which are dropped when the key goes.
- **Events and notifications:** it may only emit events it declared, and only show notifications with the permission.
- **Settings:** it may only store settings it declares, and it's only ever given its own settings and secrets.
- **Asking the host:** it can't ask the host for anything. There's no request to read another plugin's values, settings or files.
- **Environment:** it starts with none of the agent's environment except what any program needs (`PATH`, its temp and home folders, proxy settings), so nothing the agent was started with reaches it.
- **Data folder:** it gets a data folder of its own.

These are tested in `crates/flexdeck-plugin-host/tests/host.rs`. The limits are what a process is: a node, native or Python plugin runs as you, so the operating system lets it read what you can, including FlexDeck's data folder. Recipes run no code, so they have no such reach.

## The protocol

An installed plugin is a child process speaking JSON-RPC 2.0, one message per line,
on stdin/stdout. Stderr is its log. The contract is `crates/flexdeck-plugin-abi`;
the SDK's types mirror it, and the two are pinned to each other by the fixtures in
`contract/fixtures/plugin/` (the SDK writes them, the Rust tests parse them with
unknown fields refused).

**The agent calls:**

| Method | Params → result |
| --- | --- |
| `initialize` | `{abi, settings, dataDir}` → `{manifest, variables}` — always first |
| `check` | `{action, args}` → `{ok, error?}` — refuse a value before anything runs |
| `invoke` | `{action, args}` → `{ok, error?, state?}` — `state` (ABI 3) names the look a widget should show |
| `choices` | `{action, args}` → `{arguments: {name: [{value, label?}]}, error?}` — optional; the editor's suggestions |
| `sample` | `{wanted: [names]}` → `{values: {name: {kind, value}}}` |
| `settings/changed` | `{settings}` → `{}` |
| `shutdown` | `{}` → `{}`, then exit |

**The plugin may send at any time:** `vars/set {values}`, `vars/declare {variables,
replace}`, `event/emit {event, payload}`, `log {level, message}`, `notify {title, body}`
— and, under ABI 3:

| Notification | What it does |
| --- | --- |
| `settings/store {values}` | Keep values for settings the manifest declares — a refresh token just issued. Only declared keys; not echoed back as `settings/changed`. |
| `host/open_url {url}` | Open an http(s) address in the person's browser, for a sign-in page. Needs the `network` permission. |
| `images/put {sha256, mime, data}` | Hand over a picture (PNG, JPEG or WebP, base64, at most 256 KiB, checked against its hash). A widget shows it when an icon template names `img:<sha256>`. |

### ABI versions

A package names its ABI in `plugin.json`, and the agent speaks that one: it
accepts **2, 3 and 4**. Everything ABI 3 added is something a plugin *may* send —
slider metadata on an action (`slider: {argument, min, max, step, feedback?}`),
`state` in an `invoke` answer, and the three notifications above — so an ABI 2
plugin keeps working unchanged. The TypeScript SDK answers in whichever the agent
asks for, leaving newer fields out under older ABIs. A repository entry for an ABI this
agent does not speak is shown as needing a newer flexdeck and is never offered
as an update.

### ABI 4: capabilities, feeds, presets, series

From ABI 4, new features arrive as **capabilities** rather than new ABIs. The
host lists what it offers in `initialize` (`hostCapabilities`), the plugin lists
what it uses in its manifest (`uses`), and something only one side names is
never used — so either side can be older. ABI 4 brings three, and later agents add more:

| Capability | What it adds |
| --- | --- |
| `series` | `series/set {name, points}` — a variable's whole recent history (at most 600 points, oldest first) for a graph: a day of prices, a forecast's hours. |
| `presets` | Manifest `presets` — ready-made keys for the editor's gallery: `{id, name, category, description, w, h, widget}`, where `widget` is a widget in the configuration's JSON (its face, flows, states). |
| `feeds` | Manifest `feeds` and `feed/watch {instance, feed, args}` / `feed/unwatch {instance}` — values *per key*, with that key's own arguments. Publish them as `<id>.@<instance>.<field>`; the key reads them as `{{this.<field>}}`. |
| `toggles` | An action's `active` may name a fixed value (`is`) rather than an argument, and its `variable` may hold `{{argument}}` placeholders: a toggle lit while `discord.voice.mute` is true. |
| `setup` | A setting may be `internal` (kept by the plugin, cleared by signing out) or carry `help`, and the manifest may link a setup `guide`. |
| `conditions` | An action's `active` may be a condition alone: `active: { when: "obs.studio_mode and obs.preview_scene != obs.scene" }`. It is an expression over any live values, at most 2000 characters. An `{{argument}}` placeholder stands for the argument's value, as part of a name or as a whole operand, never inside quotes. A name with `-`, `/` or spaces is written in backticks: `` `my-plugin.on` ``. The SDKs leave the light out, and log a line, when the agent does not offer it. |

In TypeScript, `presets: [...]` and `feeds: {...}` go in `definePlugin`, and the
context gains `ctx.series.set(name, points)` and `ctx.feed(instance).set({...})`,
which declares and publishes a key's values. The test harness has `watch`,
`unwatch`, `feed(instance)` and `series(name)`. In Rust, `Sidecar` has
`watch_feed` and `unwatch_feed`, and `Host` has `set_series`.
[`examples/weather-node`](https://github.com/Questory-Labs/FlexDeck-Plugins/tree/main/examples/weather-node) in the plugins
repository uses all three and is the reference.

Every notification is rate-limited per plugin (`vars/set` 50 a second, `log` 20,
`images/put` 2, `notify` one every two seconds, …); what is over is dropped,
with one line in the plugin's log.

## Recipes: plugins without code

A **recipe** is a package with `"runtime": "recipe"` and a `recipe.json` instead
of a program. It holds web **data sources** and gallery **presets**, and the
agent runs it itself — no process, no Node. A recipe may only *read the web*:
it may not run a program, it must declare the `network` permission (asked for on
install, like any plugin's), and its sources are named after it (`weather`,
`weather-hourly`). Removing it removes its sources.

```json
{
  "sources": [{
    "id": "crypto",
    "everySeconds": 120,
    "args": [{ "name": "coin", "default": "bitcoin" }],
    "http": { "url": "https://api.coingecko.com/api/v3/simple/price?ids={{args.coin}}&vs_currencies=usd" },
    "fields": [{ "name": "price", "value": "json.{{args.coin}}.usd" }]
  }],
  "presets": [{
    "id": "ticker", "name": "Ticker", "w": 2, "h": 1,
    "widget": { "sources": [{ "source": "crypto" }],
                "face": { "elements": [{ "template": "{{ this.price | currency }}" }] } }
  }]
}
```

`recipes/` holds four that ship: weather, crypto, calendar (any iCal address)
and Home Assistant. Pack one with `flexdeck-plugin pack <folder>`; the agent's
tests load every one as an install would.

### Data sources

The same sources can be made in the editor, under **Data**, without a package:
a URL or a program, how often, settings a key can change (`{{args.city}}`), and
**fields** — each an expression over what came back (`json.current.temperature_2m`,
`text`, `status`, `now`), becoming `src.<source>.<field>`. *Test it now* shows
the answer; clicking a value in it adds it as a field. An iCal answer reads as
`json.events` (upcoming, sorted). Each source also publishes `.ok`, `.error`
and `.updated`; a key reading a failing one shows an amber dot.

Fetches are fenced: http(s) only, 1 MB, 10 s, at most three redirects, and every
address checked — on the first hop and each redirect — so a source reaches this
computer or your network only when it says `lan` ("May reach my network").

A token or API key is a **secret** setting: `{ "name": "token", "secret": true }`.
Its value never goes into the deck's config, so tablets, undo history and
anything shared never carry it. The agent keeps it on this computer, puts it into
the request when the source runs, and blots it out of error messages and test
answers. The first token any key gives also serves the source's other keys until
they have their own. A field may not read a secret (`args.token` in a field is
refused).

## Sharing a page

A page can be saved as a **`.fddeck`** file (page settings → *Share*). The file is JSON holding the page, the pictures it shows, and the Data-tab sources its keys read. It never holds a token or a key: the deck's config doesn't contain them, and a source's secret settings are left blank. Opening one shows what it would do before anything is added:
- in red, anything that runs a program, opens an app or a page, presses keys or types text, runs a command source, or reaches your network;
- then the plugins it uses that this desktop doesn't have.

Keys get fresh ids. A source whose id you already use is renamed, and the keys that read it are updated to match.

## Faces, templates and rules

A key's look is a **face**: a layout (icon + label, value, picture, split, dial),
elements in its places (text, icon, bar, gauge, graph, clock), a background
(colour, fade, picture, a graph behind) and **rules** that recolour it while a
value holds (`system.cpu.usage > 80`). Templates are expressions with filters:

```
{{ system.cpu.usage | round }}%        {{ btc.price | currency("EUR") }}
{{ 20.4 | temp }}  (°C or °F, the desktop's)     {{ media.current.position | duration }}
{{ x | map(0: "sun", 3: "cloud", _: "fog") }}   {{ if(obs.recording, "REC", "idle") }}
```

Filters: `round`, `number`, `pct`, `currency`, `temp`, `abs`, `floor`, `ceil`,
`sign`, `clamp`, `min`, `max`, `upper`, `lower`, `trim`, `truncate`, `default`,
`map`, `time`, `duration`, `bytes`. A template that does not parse is refused on
save with the reason. Numbers, money, times and temperatures follow the desktop's
**Formats** (System tab). The same templates work in an action's text arguments:
`"Temp: {{vars.temp}}"`.

An element can sit **anywhere** instead of in its layout's place. `"box": {"x": 0.1, "y": 0.2, "w": 0.5, "h": 0.3}` gives fractions of the face, which is the key above any strip of zones. Its `slot` still decides its text's size and alignment. In the editor, drag any piece on the *Arrange* canvas, or set its Place to *Anywhere*.

A long title can **scroll** instead of being cut short: `"text": {"marquee": true}`.

**Timers** are built in. The `timer` actions are start, toggle, pause, resume, reset and add, each naming a timer (`focus`, `tea`). A timer's values are `timer.<name>.running`, `.left`, `.ends`, `.length` and `.done`. When its time is up it announces `timer/finished` with its `name`, which a flow or an automation can start on. A `ELEMENT_KIND_TIMER` element with `"variable": "focus"` shows the countdown. The tablet counts it down itself from the seconds the agent last sent, so no two clocks need to agree. The *Pomodoro* and *Tea timer* presets use one.

A face may have **zones**: up to five buttons in a strip along the key's bottom,
each with its own flows. The rest of the face sits above the strip. A flow with
`"zone": "next"` runs when that button is tapped; a press above the strip runs
the key's own flows. A zone's icon can follow a value (`when` + `iconOn`), such
as play turning into pause. The *Now playing + controls* preset is a 3×1 example.

```json
"face": { "zones": [
  { "name": "prev", "icon": "skip-back" },
  { "name": "play", "icon": "play", "when": "media.current.playing", "iconOn": "pause" },
  { "name": "next", "icon": "skip-forward" }
]},
"flows": [{ "trigger": { "gesture": "GESTURE_SHORT_PRESS" }, "zone": "next", "blocks": [ … ] }]
```

### Sliders

An action with `slider` metadata can be driven by a slider widget: the slider's
value is written into `argument`, at most once per 50 ms while it is dragged
(the last value always runs), and `feedback` — a variable name with
`{{argument}}` placeholders filled from the action's other arguments — is what
the thumb follows when the value changes elsewhere:

```ts
set_volume: {
  arguments: { input: { kind: "text", example: "Mic" }, percent: { kind: "number", example: "50" } },
  slider: { argument: "percent", feedback: "obs.input.{{input}}.volume" },
  run: …,
}
```

### Native plugins

A package can be a native executable (`"runtime": "native"`, with `entries` per
platform). `crates/flexdeck-plugin-sdk` speaks the protocol for one written in
Rust — implement `Sidecar`, call `serve` — and the
[soundboard](https://github.com/Questory-Labs/FlexDeck-Plugins/tree/main/plugins/soundboard) is one.
`Host::put_image(bytes, mime)` hands the agent a picture and returns the
`img:<sha256>` to put in a text variable, as `ctx.images.put` does in TypeScript.

Deadlines: 10 s for `initialize` and `invoke`, 3 s for `check`, `sample` and
`settings/changed`. A plugin that misses one is restarted. A plugin that crashes is
restarted after 1, 2, 4 … 30 seconds; one that crashes more than five times in a
minute is stopped, with that reason shown, until someone presses *Restart*.

## Where things live

| Path | What |
| --- | --- |
| `crates/flexdeck-plugin-abi` | The contract: manifest, messages, `plugin.json`, ABI version |
| `crates/flexdeck-plugin` | The registry and the built-in plugins |
| `crates/flexdeck-plugin-host` | Runs an installed plugin: process, protocol, deadlines, restarts |
| `crates/flexdeck-server/src/plugins.rs` | Install, update, uninstall, enable, link, reload |
| `crates/flexdeck-server/src/widgets.rs` | What each widget does and shows: flows, looks, live labels and icons, sliders |
| `crates/flexdeck-server/src/images.rs` | Pictures plugins hand over, served at `/api/images/<sha256>` |
| `crates/flexdeck-plugin-sdk` | The Rust SDK, for native plugins |
| `crates/flexdeck-server/src/repos.rs` | Repositories: reading, caching, the marketplace list, updates |
| `crates/flexdeck-plugin-abi/src/repo.rs` | The repository format |
| `packages/plugin-sdk` | The TypeScript SDK, test harness, packer and repository builder |
| `plugins/template` | The copy-me plugin |
| `recipes/` | Recipes the agent's tests load; published from the plugins repository |

The official plugins and recipes (OBS Studio, Streamer.bot, Discord, Spotify, the
soundboard, weather, calendar, crypto, Home Assistant) live in their own
repository, [Questory-Labs/FlexDeck-Plugins](https://github.com/Questory-Labs/FlexDeck-Plugins). It is built with the
published SDKs, and its `repository.min.json` is the official repository the
app reads.
