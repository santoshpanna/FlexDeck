<p align="center">
  <a href="https://flex-deck.com"><img alt="flexdeck" src="https://raw.githubusercontent.com/santoshpanna/FlexDeck/main/.github/assets/logo.svg" width="72"></a>
</p>

<h1 align="center">flexdeck-plugin-sdk</h1>

<p align="center">
  Write native <a href="https://flex-deck.com">FlexDeck</a> plugins in Rust.<br>
  Implement <code>Sidecar</code>, hand it to <code>serve</code>, and the SDK speaks the protocol over stdin and stdout.
</p>

<p align="center">
  <a href="https://crates.io/crates/flexdeck-plugin-sdk"><img alt="crates.io" src="https://img.shields.io/crates/v/flexdeck-plugin-sdk?style=flat-square&color=ff5a1f&labelColor=111113&logo=rust&logoColor=white"></a>
  <a href="https://docs.rs/flexdeck-plugin-sdk"><img alt="docs.rs" src="https://img.shields.io/docsrs/flexdeck-plugin-sdk?style=flat-square&labelColor=111113&logo=docsdotrs&logoColor=white"></a>
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md"><img alt="Plugin guide" src="https://img.shields.io/badge/docs-plugin%20guide-ecebe6?style=flat-square&labelColor=111113"></a>
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/crates/flexdeck-plugin-sdk/LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

## A whole plugin

```rust,no_run
use flexdeck_plugin_sdk::{abi, Host, Sidecar};

struct Hello;

impl Sidecar for Hello {
    fn manifest(&self, abi: u32) -> abi::ManifestDto {
        flexdeck_plugin_sdk::manifest(abi, "hello", "Hello", "1.0.0")
    }

    fn invoke(&self, request: &abi::InvokeRequest, host: &Host) -> abi::InvokeResponse {
        host.log("info", &format!("ran {}", request.action));
        flexdeck_plugin_sdk::ok()
    }
}

fn main() {
    flexdeck_plugin_sdk::serve(Hello);
}
```

`manifest` and `invoke` are all you must write. Everything else on `Sidecar` (`start`, `check`, `choices`, `sample`, `settings_changed`, `watch_feed`, `shutdown` and the rest) has a default you override when you need it.

## Talking to the host

`Host` is how the plugin reaches back to FlexDeck:

| Method | Does |
| ------ | ---- |
| `var`, `vars_set`, `vars_declare` | Publish live values keys can react to |
| `emit` | Fire a declared event |
| `set_series` | Feed a chart |
| `put_image` | Hand over an image, named by its sha256 |
| `log`, `show` | Write to the log, or show a notice |
| `store_settings` | Save the plugin's settings |
| `open_url` | Open a link on the PC |

`ok()`, `ok_state(..)` and `failed(..)` build invoke responses, `value::{number, boolean, text}` build values, and `test_host()` gives unit tests a host to call.

## Ship it

1. Declare the binary in `plugin.json` with `"runtime": "native"`.
2. Pack it with `npx -p @flexdeck/plugin-sdk flexdeck-plugin pack`.
3. Check it with `npx -p @flexdeck/plugin-sdk flexdeck-plugin verify`, which runs it through FlexDeck exactly as an install would.

The [soundboard](https://github.com/Questory-Labs/FlexDeck-Plugins/tree/main/plugins/soundboard) in the official plugins repository is a full example.

## See also

- [Plugin guide](https://github.com/santoshpanna/FlexDeck/blob/main/docs/plugins.md): the full reference
- [`flexdeck-plugin-abi`](https://crates.io/crates/flexdeck-plugin-abi): the contract, re-exported here as `flexdeck_plugin_sdk::abi`
- [`@flexdeck/plugin-sdk`](https://www.npmjs.com/package/@flexdeck/plugin-sdk) for TypeScript · [`flexdeck`](https://github.com/santoshpanna/FlexDeck/tree/main/packages/plugin-sdk-python) for Python

<br>

<p align="center"><sub>MIT licensed, so your plugins can use any license.</sub></p>
