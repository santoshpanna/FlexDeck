//! Rust bindings for the shared flexdeck contract.
//!
//! The types here are **generated** from `contract/proto` by `build.rs`. Do not
//! hand-edit anything in this file's `include!`s — edit the `.proto` instead.
//!
//! # The JSON mapping is the contract
//!
//! The wire format is JSON, but not *arbitrary* JSON: it is the **proto3 JSON
//! mapping**, and that mapping is a specification rather than a convention.
//! That is the entire reason one `.proto` can serve three renderers written in
//! three languages. The rules that actually matter here:
//!
//! | Rule | Example |
//! | --- | --- |
//! | snake_case fields become lowerCamelCase | `gap_x` -> `"gapX"` |
//! | 64-bit integers are encoded as **strings** | `rev: 42` -> `"42"` |
//! | enums are their full names | -> `"WIDGET_TYPE_BUTTON"` |
//! | **default** values are omitted entirely | `rev: 0` -> key absent |
//! | a `oneof` uses the field name as the key | `{"hello": {...}}` |
//!
//! That fourth rule is the one most likely to bite a client author: an empty
//! `pages` list does not arrive as `[]`, it does not arrive at all.
//!
//! Every one of those is pinned by `tests/proto3_json.rs`. If a rule ever
//! breaks, the TypeScript and Kotlin clients would silently disagree with this
//! one, so the tests are the guard rail rather than documentation.

pub mod flexdeck {
    pub mod v1 {
        // Generated code is not ours to lint: prost/pbjson output is mechanical
        // and would otherwise trip style lints we enforce on hand-written code.
        #![allow(clippy::all)]

        include!(concat!(env!("OUT_DIR"), "/flexdeck.v1.rs"));
        include!(concat!(env!("OUT_DIR"), "/flexdeck.v1.serde.rs"));
    }
}

pub use flexdeck::v1::*;
