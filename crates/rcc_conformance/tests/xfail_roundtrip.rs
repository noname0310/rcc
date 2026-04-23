//! Regression tests for `rcc_conformance::xfail::load`.

use std::path::PathBuf;

use rcc_conformance::xfail;

fn suites_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/testsuites")
        .canonicalize()
        .expect("third_party/testsuites should exist")
}

/// All six seed `xfail.toml` files parse to `XFailFile::default()` (empty list).
#[test]
fn empty_xfail_files_load_cleanly() {
    let root = suites_root();
    let suites =
        ["c-testsuite", "chibicc", "gcc-torture", "tcc-tests2", "llvm-test-suite", "csmith"];

    for suite in &suites {
        let path = root.join(suite).join("xfail.toml");
        assert!(path.exists(), "xfail.toml missing for suite `{suite}`");

        let xf = xfail::load(&path).unwrap_or_else(|e| {
            panic!("xfail::load failed for `{suite}`: {e}");
        });
        assert!(
            xf.xfail.is_empty(),
            "expected empty xfail list for `{suite}`, got {} entries",
            xf.xfail.len()
        );
    }
}

/// `xfail::load` on a nonexistent path returns `XFailFile::default()`.
#[test]
fn missing_file_returns_default() {
    let path = suites_root().join("nonexistent-suite/xfail.toml");
    let xf = xfail::load(&path).expect("load should succeed for missing file");
    assert!(xf.xfail.is_empty());
}
