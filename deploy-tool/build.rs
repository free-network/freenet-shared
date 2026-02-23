use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../web-container-contract/src/");
    println!("cargo:rerun-if-changed=../web-container-contract/Cargo.toml");
    println!("cargo:rerun-if-changed=../web-container-tool/src/");
    println!("cargo:rerun-if-changed=../web-container-tool/Cargo.toml");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Build web-container-contract for wasm32-unknown-unknown
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--package",
            "web-container-contract",
        ])
        .status()
        .expect("Failed to build web-container-contract");

    if !status.success() {
        panic!("Failed to build web-container-contract");
    }

    // Copy the wasm to OUT_DIR so include_bytes! can find it
    let wasm_src = PathBuf::from("../target/wasm32-unknown-unknown/release/web_container_contract.wasm");
    let wasm_dst = out_dir.join("web_container_contract.wasm");
    fs::copy(&wasm_src, &wasm_dst).expect("Failed to copy wasm to OUT_DIR");
    println!("cargo:rustc-env=BUNDLED_CONTRACT_PATH={}", wasm_dst.display());

    // Build web-container-tool for native target
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--package",
            "web-container-tool",
        ])
        .status()
        .expect("Failed to build web-container-tool");

    if !status.success() {
        panic!("Failed to build web-container-tool");
    }

    // Copy the tool binary to OUT_DIR
    let tool_name = if cfg!(windows) {
        "web-container-tool.exe"
    } else {
        "web-container-tool"
    };
    let tool_src = PathBuf::from("../target/release").join(tool_name);
    let tool_dst = out_dir.join(tool_name);
    fs::copy(&tool_src, &tool_dst).expect("Failed to copy web-container-tool to OUT_DIR");
    println!("cargo:rustc-env=BUNDLED_TOOL_PATH={}", tool_dst.display());
}
