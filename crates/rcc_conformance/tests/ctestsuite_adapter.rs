//! Integration tests for `CTestSuiteAdapter`.

use std::path::{Path, PathBuf};

use rcc_conformance::adapters::CTestSuiteAdapter;
use rcc_conformance::{Adapter, Outcome};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("c-testsuite")
}

// ── discover ────────────────────────────────────────────────────────

#[test]
fn discover_finds_fixture_files() {
    let adapter = CTestSuiteAdapter;
    let cases = adapter.discover(&fixtures_root()).unwrap();
    assert_eq!(cases.len(), 3);
    assert_eq!(cases[0].id, "c-testsuite::00001");
    assert_eq!(cases[1].id, "c-testsuite::00002");
    assert_eq!(cases[2].id, "c-testsuite::00003");
}

#[test]
fn discover_cases_sorted_by_id() {
    let adapter = CTestSuiteAdapter;
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn discover_real_suite_at_least_200() {
    let suite_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("third_party")
        .join("testsuites")
        .join("c-testsuite");
    if !suite_root.join("tests").join("single-exec").is_dir() {
        eprintln!("skipping: real c-testsuite not vendored");
        return;
    }
    let adapter = CTestSuiteAdapter;
    let cases = adapter.discover(&suite_root).unwrap();
    assert!(cases.len() >= 200, "expected >= 200 discovered tests, got {}", cases.len(),);
}

// ── run: skip / fail paths ──────────────────────────────────────────

#[test]
fn run_skip_when_no_expected_file() {
    let adapter = CTestSuiteAdapter;
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let skip_case = cases.iter().find(|c| c.id == "c-testsuite::00003").unwrap();
    let outcome = adapter.run(Path::new("nonexistent-rcc"), skip_case).unwrap();
    assert!(matches!(outcome, Outcome::Skip { .. }), "expected Skip, got {outcome:?}",);
}

#[test]
fn run_fail_when_rcc_not_found() {
    let adapter = CTestSuiteAdapter;
    let cases = adapter.discover(&fixtures_root()).unwrap();
    let case = cases.iter().find(|c| c.id == "c-testsuite::00001").unwrap();
    let outcome = adapter.run(Path::new("nonexistent-rcc-binary-xyzzy"), case).unwrap();
    assert!(matches!(outcome, Outcome::Fail { .. }), "expected Fail, got {outcome:?}",);
}

// ── compare_outcome (pure logic) ────────────────────────────────────

#[test]
fn compare_pass_empty_expected() {
    let expected = fixtures_root().join("tests").join("single-exec").join("00001.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"", Some(0), &expected);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn compare_pass_nonempty_expected() {
    let expected = fixtures_root().join("tests").join("single-exec").join("00002.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"hello\n", Some(0), &expected);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn compare_normalizes_crlf_expected_files() {
    let tmp = tempfile::tempdir().unwrap();
    let expected = tmp.path().join("out.expected");
    std::fs::write(&expected, b"42\r\n64\r\n").unwrap();

    let outcome = CTestSuiteAdapter::compare_outcome(b"42\n64\n", Some(0), &expected);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn compare_fail_stdout_mismatch() {
    let expected = fixtures_root().join("tests").join("single-exec").join("00002.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"wrong\n", Some(0), &expected);
    assert!(matches!(outcome, Outcome::Fail { .. }), "expected Fail, got {outcome:?}",);
}

#[test]
fn compare_fail_nonzero_exit() {
    let expected = fixtures_root().join("tests").join("single-exec").join("00001.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"", Some(1), &expected);
    assert!(matches!(outcome, Outcome::Fail { .. }), "expected Fail, got {outcome:?}",);
}

#[test]
fn compare_fail_signal_killed() {
    let expected = fixtures_root().join("tests").join("single-exec").join("00001.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"", None, &expected);
    assert!(
        matches!(outcome, Outcome::Fail { .. }),
        "expected Fail for signal-killed process, got {outcome:?}",
    );
}

#[test]
fn compare_skip_missing_expected_file() {
    let bogus = fixtures_root().join("tests").join("single-exec").join("no_such_file.c.expected");
    let outcome = CTestSuiteAdapter::compare_outcome(b"", Some(0), &bogus);
    assert!(
        matches!(outcome, Outcome::Skip { .. }),
        "expected Skip when .expected is unreadable, got {outcome:?}",
    );
}
