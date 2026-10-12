<p align="center">
  <a href="https://flex-deck.com"><img alt="flexdeck" src="https://raw.githubusercontent.com/santoshpanna/FlexDeck/main/.github/assets/logo.svg" width="72"></a>
</p>

<h1 align="center">flexdeck-plugin-abi</h1>

<p align="center">
  The contract between the <a href="https://flex-deck.com">FlexDeck</a> agent and its plugins.
</p>

<p align="center">
  <a href="https://crates.io/crates/flexdeck-plugin-abi"><img alt="crates.io" src="https://img.shields.io/crates/v/flexdeck-plugin-abi?style=flat-square&color=ff5a1f&labelColor=111113&logo=rust&logoColor=white"></a>
  <a href="https://docs.rs/flexdeck-plugin-abi"><img alt="docs.rs" src="https://img.shields.io/docsrs/flexdeck-plugin-abi?style=flat-square&labelColor=111113&logo=docsdotrs&logoColor=white"></a>
  <img alt="Rust 1.89+" src="https://img.shields.io/badge/rust-1.89%2B-ecebe6?style=flat-square&labelColor=111113">
  <a href="https://github.com/santoshpanna/FlexDeck/blob/main/crates/flexdeck-plugin-abi/LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-ecebe6?style=flat-square&labelColor=111113"></a>
</p>

<br>

## What's in it

| | |
| - | - |
| **Protocol** | The JSON-RPC messages the agent and a plugin exchange |
| **Manifest** | `plugin.json`, the package manifest |
| **Repository index** | `repository.min.json`, which plugin repositories publish |
| **Packages** | Reading `.fdpkg` files, behind the `package` feature |

```toml
[dependencies]
flexdeck-plugin-abi = { version = "0.1", features = ["package"] }
```

## Writing a plugin?

You probably want [`flexdeck-plugin-sdk`](https://crates.io/crates/flexdeck-plugin-sdk) instead. It re-exports this crate as `flexdeck_plugin_sdk::abi` and handles the protocol for you.

It has a version of its own, apart from the SDK's and from FlexDeck's, and moves when the contract does.

<br>

<p align="center"><sub>MIT licensed.</sub></p>
