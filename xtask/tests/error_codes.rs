//! Integration test: the error-code registry in `codes.rs` must be
//! consistent with `docs/error-codes.md`. This is the CI gate that
//! fails if a developer adds a code without documenting it.

#[test]
fn error_codes_consistent() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must have a parent")
        .to_path_buf();

    // Reuse the xtask logic — this gives us the same check CI will run.
    xtask::check_error_codes::run(&root).expect("error-code consistency check failed");
}
