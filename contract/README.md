# flexdeck contract

Single source of truth shared by the agent, the web configurator and the
Android client.

## Why proto3 with a JSON wire?

The PRD called for a JSON Schema contract with JSON over WebSocket and
JSON-Patch deltas. We keep the *wire behaviour* exactly as the PRD specified,
but derive the *types* from `.proto` instead of hand-written schema:

| Concern | Decision |
| --- | --- |
| Type contract | `proto3`, compiled to TS, Rust and Kotlin |
| Wire encoding | JSON, using the **proto3 JSON mapping** |
| Delta sync | RFC 6902 JSON-Patch, applied to that JSON |
| Config at rest | SQLite (the agent's store), not the wire shape |

The important property is that the **proto3 JSON mapping is a specification**,
not a convention each language invents. Field `gap_x` becomes `gapX` in every
language, `uint64` always serialises as a string, unset fields are omitted.
That consistency is what makes one contract work across three renderers.

> **Watch out — defaults are omitted, not zeroed.** proto3 JSON drops *default*
> values. An empty repeated field does not arrive as `[]`; it does not arrive at
> all. A missing `pages` key means "no pages", and a client that reads it
> without a default will break. This exact behaviour is pinned by
> `crates/flexdeck-protocol/tests/proto3_json.rs`.

## Codegen

```sh
# TS (packages/protocol) + Rust (crates/flexdeck-protocol)
buf generate

# Kotlin (android/core/protocol) — via Gradle, not buf
cd android && ./gradlew :core:protocol:generateWireProtos
```

Requires `buf` (`npm i -g @bufbuild/buf`) plus the plugins named in
`buf.gen.yaml` on `PATH`.

## Grid fixtures

`fixtures/grid/*.json` are the parity contract for the layout algorithm. Each
fixture is a spec plus widget spans. The matching `*.expected.json` holds the
integer rectangles every renderer must produce. They are generated from the
TypeScript reference implementation:

```sh
pnpm gen:expected
```

If a change to the grid algorithm alters output, this command rewrites the
expected files and `git diff` shows exactly what moved. That diff is the point:
it makes an accidental parity break visible instead of silent.

Never hand-edit an `*.expected.json`.
