#[path = "build_support.rs"]
mod build_support;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed={}", build_support::LLVM_PREFIX_ENV);

    if env::var_os("CARGO_FEATURE_LLVM_WINDOWS_LLVM_C").is_none() {
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        panic!(
            "feature `llvm-windows-llvm-c` is only supported for windows-msvc targets.\n{}",
            build_support::expected_layout()
        );
    }

    let Some(prefix) = env::var_os(build_support::LLVM_PREFIX_ENV) else {
        panic!(
            "missing environment variable `{}`.\n{}",
            build_support::LLVM_PREFIX_ENV,
            build_support::expected_layout()
        );
    };

    let prefix = PathBuf::from(prefix);
    let layout = build_support::validate_llvm_c_prefix(&prefix)
        .unwrap_or_else(|message| panic!("{message}"));

    println!("cargo:rustc-link-search=native={}", layout.lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=LLVM-C");
}
