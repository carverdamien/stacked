//! With the `web` feature, on by default: the browser's half of `stacked
//! web`, web/client.rs, compiled to wasm for the server to hand out. It has
//! no dependencies: rustc alone builds it, for Rust's wasm32-unknown-unknown
//! target. Where Rust lacks that target, the build stops and says how to add
//! it, or how to build stacked without `web`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The target the browser's half is compiled for.
const WASM: &str = "wasm32-unknown-unknown";

fn main() {
    println!("cargo::rerun-if-changed=web/client.rs");
    if std::env::var_os("CARGO_FEATURE_WEB").is_none() {
        return;
    }
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    if !has_target(&rustc) {
        // Cargo says each line as an error and stops the build: neither a
        // panic nor rustc's own errors (it cannot find `std`) around them.
        let by_default = if std::env::var_os("CARGO_FEATURE_DEFAULT").is_some() { ", on by default," } else { "" };
        println!(
            "cargo::error=the web feature{by_default} compiles web/client.rs to wasm with Rust's {WASM} target, which this Rust does not have"
        );
        println!(
            "cargo::error=add the target with `rustup target add {WASM}`, or build stacked without web, as {} does",
            without_web()
        );
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("client.wasm");
    let status = Command::new(&rustc)
        .args(["--edition", "2024", "--crate-type", "cdylib", "--crate-name", "client"])
        .args(["--target", WASM, "-C", "opt-level=s", "-C", "strip=symbols", "-o"])
        .arg(&out)
        .arg("web/client.rs")
        .status()
        .expect("rustc runs");
    if !status.success() {
        panic!("web/client.rs does not compile to wasm, as rustc says above: {} builds stacked without web", without_web());
    }
}

/// Whether this Rust has the wasm target's standard library, in the folder
/// rustc names for it. A rustc that cannot name one is not stopped here: the
/// build goes on, and rustc says what it lacks.
fn has_target(rustc: &OsStr) -> bool {
    match Command::new(rustc).args(["--print", "target-libdir", "--target", WASM]).output() {
        Ok(out) if out.status.success() => Path::new(String::from_utf8_lossy(&out.stdout).trim()).is_dir(),
        _ => true,
    }
}

/// The flags that build stacked as this build does, but without `web`:
/// `--no-default-features`, then the other features this build has.
fn without_web() -> String {
    let mut kept: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| Some(name.to_str()?.strip_prefix("CARGO_FEATURE_")?.to_lowercase().replace('_', "-")))
        .filter(|f| f != "web" && f != "default")
        .collect();
    kept.sort();
    if kept.is_empty() {
        "`--no-default-features`".to_string()
    } else {
        format!("`--no-default-features --features {}`", kept.join(","))
    }
}
