//! Generates Rust types and proto3-JSON serde impls from the shared contract.
//!
//! `protoc` is deliberately **not** required on the machine:
//! `protoc-bin-vendored` ships a `protoc` binary, so a clean checkout builds
//! without a system Protobuf install. That matters because CI and a fresh
//! clone should not need a setup step beyond `cargo`.
//!
//! Two outputs are produced from the same descriptor set:
//!
//! 1. `prost` -> the Rust structs (`flexdeck.v1.rs`).
//! 2. `pbjson` -> `serde` impls that follow the proto3 JSON mapping
//!    (`flexdeck.v1.serde.rs`).
//!
//! Both are emitted into `OUT_DIR` and stitched into the crate by `src/lib.rs`.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_root = manifest_dir.join("../../contract/proto");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let descriptor_path = out_dir.join("flexdeck_descriptor.bin");

    // Re-run when the contract changes. Watching the whole tree is fine at this
    // size and avoids a subtle bug where editing only an imported file does not
    // trigger a rebuild.
    println!("cargo:rerun-if-changed={}", proto_root.display());

    let protos = [
        proto_root.join("flexdeck/v1/config.proto"),
        proto_root.join("flexdeck/v1/events.proto"),
        proto_root.join("flexdeck/v1/pairing.proto"),
        proto_root.join("flexdeck/v1/tunnel.proto"),
    ];

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    // The descriptor set is the hand-off to pbjson. Without it, pbjson would
    // have no idea what the messages are.
    config.file_descriptor_set_path(&descriptor_path);
    // `google.protobuf.ListValue` (a flow's blocks) as pbjson's types, whose
    // serde is plain JSON — the generated prost ones would have none.
    config.compile_well_known_types();
    config.extern_path(".google.protobuf", "::pbjson_types");
    config.compile_protos(protos.as_slice(), &[proto_root.as_path()])?;

    let descriptor_bytes = std::fs::read(&descriptor_path)?;
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_bytes)?
        .build(&[".flexdeck.v1"])?;

    Ok(())
}
