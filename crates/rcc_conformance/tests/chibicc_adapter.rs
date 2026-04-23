//! Integration tests for `ChibiccAdapter`.

use std::path::{Path, PathBuf};

use rcc_conformance::adapters::ChibiccAdapter;
use rcc_conformance::{Adapter, Outcome};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("chibicc-mini")
}

// ── discover ────────────────────────────────────────────────────────

#[test]
fn discover_finds_fixture_files() {
    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(&fixtures_root()).unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].id, "chibicc::arith");
    assert_eq!(cases[1].id, "chibicc::control");
}

#[test]
fn discover_excludes_non_c_files() {
    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    assert!(!ids.iter().any(|id: &&str| id.contains("common")), "common must be excluded");
    assert!(!ids.iter().any(|id: &&str| id.contains("test.h")), "headers must be excluded");
}

#[test]
fn discover_cases_sorted_by_id() {
    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn discover_real_suite_count() {
    let suite_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("third_party")
        .join("testsuites")
        .join("chibicc");
    if !suite_root.join("test").is_dir() {
        eprintln!("skipping: real chibicc tests not vendored");
        return;
    }
    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(&suite_root).unwrap();
    assert_eq!(
        cases.len(),
        41,
        "expected 41 discovered tests (all .c except common), got {}",
        cases.len(),
    );
}

#[test]
fn discover_error_on_missing_dir() {
    let adapter = ChibiccAdapter::compile();
    let result = adapter.discover(Path::new("/nonexistent/path"));
    assert!(result.is_err());
}

// ── preprocess mode ─────────────────────────────────────────────────

#[test]
fn discover_preprocess_mode_filters_to_preprocessor_fixtures() {
    // Synthetic fixture tree with one in-bucket file (`macro.c`) and
    // one out-of-bucket file (`arith.c`) plus the required
    // `common` support file. Preprocess mode must keep only
    // `macro.c`; compile mode must keep both.
    let tmp = tempfile::tempdir().unwrap();
    let test_dir = tmp.path().join("test");
    std::fs::create_dir_all(&test_dir).unwrap();
    std::fs::write(test_dir.join("common"), "// support\n").unwrap();
    std::fs::write(test_dir.join("macro.c"), "int main() { return 0; }\n").unwrap();
    std::fs::write(test_dir.join("arith.c"), "int main() { return 0; }\n").unwrap();

    let compile = ChibiccAdapter::compile();
    let compile_cases = compile.discover(tmp.path()).unwrap();
    let compile_ids: Vec<&str> = compile_cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(compile_ids, vec!["chibicc::arith", "chibicc::macro"]);

    let preprocess = ChibiccAdapter::preprocess();
    let pp_cases = preprocess.discover(tmp.path()).unwrap();
    let pp_ids: Vec<&str> = pp_cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(pp_ids, vec!["chibicc::macro"]);
}

#[test]
fn run_preprocess_mode_fails_with_missing_rcc() {
    // Nothing else to assert on a bogus binary — we just want to
    // confirm the preprocess branch returns a graceful `Fail`
    // rather than an Err/panic.
    let tmp = tempfile::tempdir().unwrap();
    let test_dir = tmp.path().join("test");
    std::fs::create_dir_all(&test_dir).unwrap();
    std::fs::write(test_dir.join("macro.c"), "int x = 0;\n").unwrap();

    let adapter = ChibiccAdapter::preprocess();
    let cases = adapter.discover(tmp.path()).unwrap();
    assert_eq!(cases.len(), 1);
    let outcome = adapter.run(Path::new("nonexistent-rcc-xyzzy"), &cases[0]).unwrap();
    assert!(matches!(outcome, Outcome::Fail { .. }), "expected Fail, got {outcome:?}");
}

// ── run: failure paths ──────────────────────────────────────────────

#[test]
fn run_fail_when_rcc_not_found() {
    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let case = cases.iter().find(|c| c.id == "chibicc::arith").unwrap();
    let outcome = adapter.run(Path::new("nonexistent-rcc-binary-xyzzy"), case).unwrap();
    assert!(matches!(outcome, Outcome::Fail { .. }), "expected Fail, got {outcome:?}");
}

#[test]
fn run_skip_when_common_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let test_dir = tmp.path().join("test");
    std::fs::create_dir_all(&test_dir).unwrap();
    std::fs::write(test_dir.join("solo.c"), "int main() { return 0; }\n").unwrap();

    let adapter = ChibiccAdapter::compile();
    let cases = adapter.discover(tmp.path()).unwrap();
    assert_eq!(cases.len(), 1);
    let outcome = adapter.run(Path::new("nonexistent-rcc"), &cases[0]).unwrap();
    assert!(
        matches!(outcome, Outcome::Skip { ref reason } if reason.contains("common")),
        "expected Skip mentioning common, got {outcome:?}",
    );
}
