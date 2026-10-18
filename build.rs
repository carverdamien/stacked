//! With the `web` feature: the browser's half of `stacked web`,
//! web/client.rs, compiled to wasm for the server to hand out. It has no
//! dependencies: rustc alone builds it, for Rust's wasm32-unknown-unknown
//! target.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=web/client.rs");
    if std::env::var_os("CARGO_FEATURE_WEB").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("client.wasm");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "cdylib", "--crate-name", "client"])
        .args(["--target", "wasm32-unknown-unknown", "-C", "opt-level=s", "-C", "strip=symbols", "-o"])
        .arg(&out)
        .arg("web/client.rs")
        .status()
        .expect("rustc runs");
    if !status.success() {
        panic!(
            "the web feature compiles web/client.rs to wasm, with Rust's wasm32-unknown-unknown target: \
             `rustup target add wasm32-unknown-unknown`"
        );
    }
}
