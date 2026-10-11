# Contributing to flexdeck

Thanks for helping. Bug reports, plugins, recipes and pull requests are all
welcome.

## Before you start

* **Bugs**: open an issue with what you did, what you expected, and what happened.
  Say which desktop and Android versions you used, and which tablet.
* **Features**: open an issue first, so we can agree on the shape before you
  write the code.
* **Plugins**: a plugin does not need to live in this repository. Publish it from
  your own [repository](docs/plugins.md#host-your-own-repository) and ask for it
  to be reviewed for the community list.
* **Security issues**: follow [SECURITY.md](SECURITY.md), not the issue tracker.

## Sign the CLA

Before your first pull request can be merged, you sign the
[Contributor License Agreement](CLA.md). The CLA bot comments on your pull request
with instructions: you reply with one line, once, and it covers every future
contribution.

You keep the copyright in your code. The CLA gives the project the rights it
needs to maintain and license flexdeck over the long term.

## Making a change

```sh
pnpm install
pnpm dev:desktop        # the desktop app, with its window
pnpm dev:web            # the web editor and the cloud relay behind it
```

[docs/development.md](docs/development.md) has every command and the toolchain
versions.

Before you open a pull request, run the checks for what you touched:

```sh
pnpm verify:node        # TypeScript: typecheck + tests
pnpm verify:rust        # Rust: fmt, clippy, tests
pnpm verify:plugins     # the plugin SDKs, first-party plugins, and the host
pnpm verify:android     # the Android app + Kotlin tests (needs ANDROID_HOME)
```

## Ground rules

* **The grid has one contract.** The layout algorithm is implemented in
  TypeScript, Rust and Kotlin and held together by golden fixtures. Change it on
  purpose with `pnpm gen:expected`, and never hand-edit an `*.expected.json`.
  [Architecture](docs/architecture.md#the-parity-problem) explains why.
* **No accounts.** flexdeck has no sign-in by design. A change that needs one is
  a change to discuss first.
* **Push, don't poll.** Live state reaches the portal and the tablet over
  WebSockets.
* **Icons come from one source.** The `[f]` mark is defined in
  `packages/ui/src/brand/mark.ts`. Run `pnpm brand:icons` to regenerate every app
  icon, favicon and README image from it.

## Licensing

The project is [AGPL-3.0](LICENSE). The plugin kits (the SDKs, the plugin ABI,
`contract/`, `create-flexdeck-plugin` and the plugin template) are MIT, so plugins
can use any license. Each of those folders has its own `LICENSE`. Don't move code
from an AGPL folder into an MIT one.
