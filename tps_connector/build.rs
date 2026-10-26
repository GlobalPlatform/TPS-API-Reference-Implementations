/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/*
 * Build script to run cbindgen over the crate to generate tpsc_connector_api.h.
 *
 * cbindgen is used as a library rather than by spawning the `cbindgen` binary. Shelling out would
 * require every consumer of this crate to have run `cargo install cbindgen` first, which is not
 * true of docs.rs or of a plain `cargo build` on a fresh machine, and the failure mode is a build
 * script panic rather than a resolvable dependency error. The `cbindgen` build-dependency in
 * Cargo.toml is what makes the library available, so the version used is pinned by Cargo.lock
 * instead of being whatever happens to be on PATH.
 */

fn main() {
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src");

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set");

    let config = cbindgen::Config::from_file("cbindgen.toml")
        .expect("failed to read tps_connector/cbindgen.toml");

    // `write_to_file` is a no-op when the generated text matches what is already on disk, so this
    // does not re-dirty the committed header or trigger a rebuild loop.
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed to generate tpsc_connector_api.h")
        .write_to_file("tpsc_connector_api.h");
}
