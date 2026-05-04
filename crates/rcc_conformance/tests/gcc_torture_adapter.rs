use std::fs;

use rcc_conformance::adapters::GccTortureAdapter;
use rcc_conformance::Adapter;

#[test]
fn discover_uses_smoke_subset_ordered_by_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let first = root.join("gcc/testsuite/gcc.c-torture/execute/20000205-1.c");
    let second = root.join("gcc/testsuite/gcc.c-torture/execute/20000113-1.c");
    fs::create_dir_all(first.parent().unwrap()).expect("create tree");
    fs::write(&first, "int main(void){return 0;}").expect("write first");
    fs::write(&second, "int main(void){return 0;}").expect("write second");
    fs::write(
        root.join(GccTortureAdapter::SMOKE_SUBSET),
        "\n# comment\ngcc/testsuite/gcc.c-torture/execute/20000205-1.c\n\
         gcc/testsuite/gcc.c-torture/execute/20000113-1.c\n",
    )
    .expect("write subset");

    let cases = GccTortureAdapter::smoke().discover(root).expect("discover");

    let ids = cases.iter().map(|case| case.id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["gcc-torture::execute::20000113-1", "gcc-torture::execute::20000205-1"]);
}

#[test]
fn discover_rejects_parent_dir_subset_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join(GccTortureAdapter::SMOKE_SUBSET), "../outside.c\n")
        .expect("write subset");

    let err = GccTortureAdapter::smoke().discover(tmp.path()).unwrap_err();

    assert!(err.to_string().contains("clean relative paths"), "{err}");
}

#[test]
fn full_execute_discovers_direct_execute_c_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let execute = root.join("gcc/testsuite/gcc.c-torture/execute");
    fs::create_dir_all(&execute).expect("create execute dir");
    fs::write(execute.join("b.c"), "int main(void){return 0;}").expect("write b");
    fs::write(execute.join("a.c"), "int main(void){return 0;}").expect("write a");
    fs::write(execute.join("not-c.txt"), "ignored").expect("write ignored");

    let cases = GccTortureAdapter::full_execute().discover(root).expect("discover");

    let ids = cases.iter().map(|case| case.id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["gcc-torture::execute::a", "gcc-torture::execute::b"]);
}
