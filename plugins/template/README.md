# Plugin template

The starting point for a new flexdeck plugin, in TypeScript. Every part of the
plugin contract is used once in [`src/index.ts`](src/index.ts), with a note on
why it looks the way it does; delete what you do not need.

## Start a plugin

From the repository root:

```sh
pnpm create-plugin weather "Weather"
```

That copies this folder to `plugins/weather` with the id replaced everywhere.
Then, in the new folder:

```sh
pnpm test            # every action runs with its examples, without the agent
pnpm run package     # writes weather-<version>.fdpkg, ready to install
```

While you work, link the folder from the flexdeck window (**Plugins → Link a
folder**) and each save reloads the plugin in the running agent.

## What goes where

| In `src/index.ts` | For |
| --- | --- |
| `id` | The namespace for everything the plugin owns. Must match `plugin.json`; never change it once people use it. |
| `permissions` | What the person is asked to allow: `network`, `notifications`, `processes`, `filesystem`. |
| `settings` | What the person fills in once. Mark passwords and tokens `secret`. |
| `variables` | Live values a key can show (`{{weather.temp}}`) and automations can react to. |
| `events` | Things that happen, for an automation to trigger on. |
| `actions` | What a key can do. Every argument needs an `example` that works. |
| `sample` | Answers for the variables someone is watching, and only those. |
| `start` | Connect, subscribe, start timers. |

`ctx.images.put(bytes, "image/png")` hands the agent a picture and returns
`img:<sha256>`; put that in a text variable and a key's icon can follow it.

The full guide is [`docs/plugins.md`](../../docs/plugins.md).
