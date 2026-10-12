# Releasing

Every release is cut by a line in a commit message on `main`. Pushing the commit
is the whole procedure: `.github/workflows/pipeline.yml` reads the history,
builds what the line names, publishes it and tags the commit. There are no
version bumps to commit by hand. The version comes from the line and is written
into each build as it is made.

| Line | Ships | Tag |
| --- | --- | --- |
| `release: v0.4.0` | Desktop, Android and server, plus a GitHub release | `v0.4.0` |
| `release(desktop): v0.4.1` | The desktop app only | `v0.4.1` |
| `release(android): v0.4.1` | The Android app only | `v0.4.1` |
| `release(server): v0.4.1` | The server only | `v0.4.1` |
| `release(desktop, server): v0.4.1` | Any mix of the three apps | `v0.4.1` |
| `release(js-sdk): v0.2.0` | `@flexdeck/plugin-sdk` → npm | `js-sdk-v0.2.0` |
| `release(create-plugin): v0.2.0` | `create-flexdeck-plugin` → npm | `create-plugin-v0.2.0` |
| `release(rust-abi): v0.2.0` | `flexdeck-plugin-abi` → crates.io | `rust-abi-v0.2.0` |
| `release(rust-sdk): v0.2.0` | `flexdeck-plugin-sdk` → crates.io | `rust-sdk-v0.2.0` |
| `release(python-sdk): v0.2.0` | `flexdeck` → PyPI | `python-sdk-v0.2.0` |
| `release(js-sdk, create-plugin): v0.2.0` | Several SDK packages at one version | one tag per package |

The line must be exactly `release` or `release(<targets>)`, a colon, and a
version `vMAJOR.MINOR.PATCH` with an optional pre-release suffix (`v0.4.0-rc.1`).
It can sit anywhere in the message, subject or body. A target the pipeline
doesn't know fails the run.

## How a commit message is read

Every line of every commit is read on its own:

- **`feat`, `fix`, `perf`, `sec` lines** are public changelog entries
  (Added, Fixed, Improved, Security). Write them for users. The scope picks
  the area: `desktop`, `android`, `portal`, `ui`, `plugin`, a plugin's id, and
  so on; the full mapping is in `scripts/changelog/changelog.lib.ts`.
- **`chore`, `ci`, `test`, `docs`, `refactor` lines** never reach the changelog.
- **A `release` line** cuts a release, as above.
- **Untyped lines** in a release commit, other than trailers like
  `Co-Authored-By:`, become the release's blurb.

A typical commit carries a plain subject and one typed line per change:

```text
Ship faders on Android
feat(android): faders that follow live values
fix(desktop): reconnect after the PC wakes from sleep
test(android): cover fader drag
```

## App releases

An app release takes every changelog line since the previous app release,
including its own commit's. The commit's subject is the release title and its
untyped lines are the blurb:

```text
Faders and live values
release: v0.4.0
Keys and faders now react to what is happening on your PC.
feat(android): faders that follow live values
```

The release commit can carry nothing but the release line. It then releases
the changes in the commits before it:

```text
Release 0.4.1
release(desktop): v0.4.1
```

What the pipeline does with it:

1. **Changelog.** Regenerates `CHANGELOG.md` and the `/changelog` page
   (`apps/portal/src/modules/changelog/releases.json`), commits them back, and
   tags the release commit `v0.4.0`.
2. **Builds** what the line names:
   - **desktop:** Windows installer, Linux AppImage and `.deb` (`desktop.yml`)
   - **android:** a signed universal APK plus one APK per CPU family:
     arm64-v8a, armeabi-v7a, x86_64, x86 (`android.yml`)
   - **server:** the editor and API image to Docker Hub, tagged with the
     version, then deployed to Easypanel (`server.yml`)
3. **GitHub release.** Publishes `v0.4.0` with the files, `SHA256SUMS.txt`, and
   the notes and title from the changelog. An app the release skipped keeps
   its files from the previous release, so every download link on the site
   keeps working.

Not releases, for reference: the public site (`web.yml`) deploys on every
push, and the server also redeploys on any push that changes its code, without
a new version.

## SDK releases

Each SDK package has its own version line, apart from the apps and from each
other. An SDK release is not an app release: it doesn't cut the changelog, and
it doesn't tag `v…` or make a GitHub release. Its `feat`/`fix` lines still go
into the app changelog, under the next app release.

```text
Ship test helpers in the SDK
release(js-sdk): v0.2.0
feat(sdk): an in-process harness for plugin tests
```

What the pipeline does with it (`sdk.yml`, once per package):

1. Writes the version into that package's manifest (`scripts/release/sdk-version.ts`).
   The versions committed in the manifests are placeholders and never need editing.
   - **js-sdk:** `packages/plugin-sdk/package.json`.
   - **create-plugin:** `packages/create-flexdeck-plugin/package.json`.
   - **rust-abi:** `crates/flexdeck-plugin-abi/Cargo.toml`.
   - **rust-sdk:** `crates/flexdeck-plugin-sdk/Cargo.toml`.
   - **python-sdk:** `packages/plugin-sdk-python/pyproject.toml`.
2. Tests that package, then publishes it with trusted publishing (OIDC), so no
   token is stored anywhere.
3. Tags the commit, e.g. `js-sdk-v0.2.0`.

### Packages that depend on another

Two packages are built against another one:

| Package | Depends on | Because |
| --- | --- | --- |
| create-plugin | js-sdk | The plugin it scaffolds depends on `@flexdeck/plugin-sdk` at that version |
| rust-sdk | rust-abi | Its `flexdeck-plugin-abi` dependency is that version, and crates.io refuses to publish it until that version exists there |

Each takes the newest release of its dependency as of its own release commit,
counting one in the same commit. So `release(create-plugin): v0.3.0` after
`release(js-sdk): v0.2.0` scaffolds plugins on `^0.2.0`, without releasing
js-sdk again. The pipeline publishes in two waves: js-sdk, rust-abi and
python-sdk first, then create-plugin and rust-sdk. A new ABI and the SDK that
uses it can go out in one commit:

```text
Typed plugin settings
release(rust-abi): v0.3.0
release(rust-sdk): v0.4.0
```

A dependent needs at least one `release(...)` line for its dependency before
or alongside it. Without one, its publish fails and says so; nothing else is
held up.

A pre-release (`v0.2.0-rc.1`) goes to npm under the `next` dist-tag, so
`npm install` still gets the latest stable version. On PyPI it becomes
`0.2.0rc1`, which `pip` skips unless asked for it.

## Mixing releases in one commit

A commit can carry one app release line and SDK release lines, each on its own
line:

```text
Plugin settings
release(desktop): v0.5.0
release(js-sdk): v0.3.0
feat(desktop): settings pages for plugins
feat(sdk): declare plugin settings
```

One line can't name an app and an SDK together (`release(desktop, js-sdk)`),
because they never share a version. The pipeline refuses it.

## Versions

- **Pre-releases.** Use a suffix for test builds: `v0.4.0-rc.1`. Every build
  carries the full version, so the installers and the APK say `0.4.0-rc.1`.
- **No repeats.** Each version is released once: a second `release: v0.4.0`,
  or a second `release(js-sdk): v0.2.0`, fails the changelog step.
- **Android limit.** The Android versionCode is derived from the version
  (major·10000 + minor·100 + patch), so minor and patch stay under 100 for app
  releases.

## When something fails

Fix the cause, push the fix, then run **Actions → Pipeline → Run workflow**.

- **Apps:** the newest app release that has a tag but no GitHub release is
  built again.
- **SDKs:** the newest release of each SDK package without its `<sdk>-v…` tag
  is published again. A version already published is skipped, so a
  half-finished release completes.

Each stage also runs on its own from the Actions tab, without releasing
anything. *Desktop* and *Android* build with a version you give them (to test a
build), and *Server* redeploys the commit it runs on. *Plugin SDK* does publish:
give it a package, a version and a commit, plus the dependency's version for
create-plugin or rust-sdk.

## Previewing

`pnpm changelog` regenerates the changelog locally from your history.
`node scripts/changelog/changelog.ts --notes v0.4.0` prints one release's notes,
and `--sdk-pending` lists the SDK releases the next run would publish.

## Plugins

The official plugins are released from their own repository,
[Questory-Labs/FlexDeck-Plugins](https://github.com/Questory-Labs/FlexDeck-Plugins).
There it is a manual run (**Actions → Release plugin**) that commits
`release(<id>): v<version>` itself, then builds the plugin and publishes it to
the plugin index. Its README covers the details.
