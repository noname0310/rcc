use std::fs;

use rcc_conformance::adapters::LlvmTestSuiteAdapter;
use rcc_conformance::{Adapter, Outcome};

#[test]
fn discover_uses_curated_single_source_subset() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("SingleSource/UnitTests");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("2002-04-17-PrintfChar.c"), "int main(void){return 0;}").unwrap();
    fs::write(dir.join("2002-04-17-PrintfChar.reference_output"), "exit 0\n").unwrap();
    fs::write(dir.join("not-in-subset.c"), "int main(void){return 0;}").unwrap();
    fs::write(dir.join("not-in-subset.reference_output"), "exit 0\n").unwrap();

    let cases = LlvmTestSuiteAdapter.discover(tmp.path()).unwrap();

    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].id, "llvm-test-suite::2002-04-17-PrintfChar");
}

#[test]
fn compare_appends_exit_code_to_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let expected = tmp.path().join("case.reference_output");
    fs::write(&expected, b"hello\nexit 0\n").unwrap();

    assert!(matches!(
        LlvmTestSuiteAdapter::compare_outcome(b"hello\n", Some(0), &expected),
        Outcome::Pass
    ));
    assert!(matches!(
        LlvmTestSuiteAdapter::compare_outcome(b"hello\n", Some(1), &expected),
        Outcome::Fail { .. }
    ));
}
