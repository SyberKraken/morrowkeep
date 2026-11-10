//! Builds the phone's copy of Morrowkeep's rules: morrowkeep-core compiled to WebAssembly, which
//! the phone server sends as /core.wasm. Needs the target once: rustup target add wasm32-unknown-unknown
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=core/src");
    println!("cargo:rerun-if-changed=core/Cargo.toml");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // its own target folder (so it doesn't wait on the outer build's lock), kept short: build
    // paths under OUT_DIR can pass Windows' 260-character limit
    let target_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("target").join("wasm-core");
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["build", "-p", "morrowkeep-core", "--target", "wasm32-unknown-unknown", "--release", "--target-dir"]).arg(&target_dir);
    for var in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "CARGO_MAKEFLAGS"] {
        cmd.env_remove(var);
    }
    let status = cmd.status().expect("couldn't run cargo to build the phone's WebAssembly");
    if !status.success() {
        panic!("building morrowkeep-core for the phone failed; if the target is missing run: rustup target add wasm32-unknown-unknown");
    }
    std::fs::copy(target_dir.join("wasm32-unknown-unknown/release/morrowkeep_core.wasm"), out.join("core.wasm")).expect("copy core.wasm");
}
