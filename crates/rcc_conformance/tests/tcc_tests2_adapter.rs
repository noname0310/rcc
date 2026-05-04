use std::fs;

use rcc_conformance::adapters::TccTests2Adapter;
use rcc_conformance::{Adapter, Outcome, TestCase};

#[test]
fn discover_finds_expect_paired_c_files_sorted_by_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("tests/tests2");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("02_b.c"), "int main(void){return 0;}").unwrap();
    fs::write(dir.join("02_b.expect"), "").unwrap();
    fs::write(dir.join("01_a.c"), "int main(void){return 0;}").unwrap();
    fs::write(dir.join("01_a.expect"), "").unwrap();
    fs::write(dir.join("ignored.c"), "int main(void){return 0;}").unwrap();

    let cases = TccTests2Adapter.discover(tmp.path()).unwrap();
    let ids = cases.into_iter().map(|case| case.id).collect::<Vec<_>>();

    assert_eq!(ids, ["tcc-tests2::01_a", "tcc-tests2::02_b"]);
}

#[test]
fn discover_real_suite_count() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/testsuites/tcc-tests2");
    let cases = TccTests2Adapter.discover(&root).unwrap();

    assert!(cases.len() >= 80, "unexpected tests2 case count: {}", cases.len());
}

#[test]
fn compare_normalizes_crlf_expected_output() {
    let tmp = tempfile::tempdir().unwrap();
    let expected = tmp.path().join("case.expect");
    fs::write(&expected, b"a\r\nb\r\n").unwrap();

    assert!(matches!(TccTests2Adapter::compare_outcome(b"a\nb\n", &expected), Outcome::Pass));
}

#[test]
fn run_fails_when_rcc_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let case_path = tmp.path().join("00_assignment.c");
    fs::write(&case_path, "int main(void){return 0;}").unwrap();
    fs::write(tmp.path().join("00_assignment.expect"), "").unwrap();
    let case = TestCase { id: "tcc-tests2::00_assignment".into(), path: case_path };

    let outcome = TccTests2Adapter.run(std::path::Path::new("definitely-missing-rcc"), &case);

    assert!(matches!(outcome.unwrap(), Outcome::Fail { .. }));
}
