# Changelog

Generated from commit history by `pnpm changelog`. Do not edit by hand.

## v0.1.0-rc.6 — 2026-10-14

**Retry taken ports, reuse one window, and gate command sources**

### Added

- **desktop** Reuse the open window when flexdeck is launched again.

### Fixed

- **general** Cla workflow.
- **desktop** Retry a taken port every 5s and as soon as the window is focused.
- **desktop** Move the default agent port to 23539 and same-PC door to 23540.
- **android** Default to port 23539 to match the desktop agent.
- **editor** Reach the same-PC agent on port 23540.
- **desktop** Require a yes at the desktop for command sources from elsewhere.
- **desktop** Keep a key's address from borrowing another key's shared token.
- **editor** Flag imported sources that send requests to another host.

## v0.1.0-rc.5 — 2026-10-13

**Publish the plugin SDKs and move official plugins to their own repo**

### Added

- **plugins** Publish @flexdeck/plugin-sdk and create-flexdeck-plugin on npm, flexdeck-plugin-abi and flexdeck-plugin-sdk on crates.io, and flexdeck on PyPI.
- **desktop** A "Minimize to tray" setting that hides the minimized window from the taskbar.

### Fixed

- **plugins** Load the official and vetted plugin lists from Questory-Labs/FlexDeck-Plugins.

## v0.1.0-rc.4 — 2026-10-12

**Allocate widget ids across the whole configuration**

### Fixed

- **editor** Derive next widget id from every profile and page.

## v0.1.0-rc.3 — 2026-10-12

**Replace re-paired devices and ship window prefs with side-by-side dev**

### Added

- **desktop** Add optional client_id so re-pair replaces the old device row.
- **desktop** Persist device.client and revoke-all with keep-self.
- **desktop** Add forget-me and revoke-all; mark kind and current on devices.
- **editor** Forward clientId on browser pair; serve api-only portal stub.
- **android** Send install clientId, forget-me on disconnect, .dev package.
- **desktop** Window prefs for autostart, start minimized, and close to tray.
- **desktop** Paginate devices with remove-all and kind pills.
- **editor** JsonTree raw config, paged devices, revoke-all others.
- **editor** Add Pager, Pill, paginate, and useSteadyHeight.

### Fixed

- **editor** Proxy /api WebSockets and harden unreachable/cut-off errors.

## v0.1.0-rc.2 — 2026-10-11

**Ship scoped releases with Linux packages and per-ABI Android APKs**

### Added

- **editor** Add platforms to the Release type used by downloads.

### Fixed

- **general** Keep the full pre-release version in the Tauri config.

## v0.1.0-rc.1 — 2026-10-11

**Test release candidate**

A first test build of the desktop and Android apps.

### Added

- **desktop** Add configurator live feed at /api/live with cloud watch leases.
- **editor** Relay live frames through /api/live after desktop watch grants.
- **editor** Consume live socket for revision, state, page, devices, sources.
- **editor** Ship FillPicker with linear/radial gradients and angle dial.
- **editor** Add ColorArea, HueStrip, AngleDial, Popover, and Endpoint.
- **plugins** Raise ABI 4 with action.active for lit keys while a pick holds.
- **plugins** OBS Studio: bump to 0.4 with scene/record/stream presets and set_scene active.
- **android** Add connection panel, ASCII link view, and pairing polish.
- **desktop** Surface endpoint address/port on home and pair screens.
- **editor** Add download, docs, and changelog routes with site nav.
- **editor** Split public site vs editor-only builds via VITE flags.

### Fixed

- **desktop** Run pnpm dev/build from app cwd in Tauri hooks.
