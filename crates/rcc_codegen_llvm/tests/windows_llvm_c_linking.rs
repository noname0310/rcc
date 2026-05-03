#[path = "../build_support.rs"]
mod build_support;

use std::fs;

#[test]
fn validates_official_windows_archive_layout() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("lib")).unwrap();
    fs::create_dir(tmp.path().join("bin")).unwrap();
    fs::write(tmp.path().join("lib").join("LLVM-C.lib"), b"import lib").unwrap();
    fs::write(tmp.path().join("bin").join("LLVM-C.dll"), b"dll").unwrap();

    let layout = build_support::validate_llvm_c_prefix(tmp.path()).unwrap();
    assert_eq!(layout.lib_dir, tmp.path().join("lib"));
    assert_eq!(layout.bin_dir, tmp.path().join("bin"));
    assert_eq!(layout.import_lib, tmp.path().join("lib").join("LLVM-C.lib"));
    assert_eq!(layout.runtime_dll, tmp.path().join("bin").join("LLVM-C.dll"));
}

#[test]
fn reports_missing_import_library() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("lib")).unwrap();
    fs::create_dir(tmp.path().join("bin")).unwrap();
    fs::write(tmp.path().join("bin").join("LLVM-C.dll"), b"dll").unwrap();

    let err = build_support::validate_llvm_c_prefix(tmp.path()).unwrap_err();
    assert!(err.contains("LLVM-C.lib"));
    assert!(err.contains(build_support::LLVM_PREFIX_ENV));
}

#[test]
fn reports_missing_runtime_dll() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("lib")).unwrap();
    fs::create_dir(tmp.path().join("bin")).unwrap();
    fs::write(tmp.path().join("lib").join("LLVM-C.lib"), b"import lib").unwrap();

    let err = build_support::validate_llvm_c_prefix(tmp.path()).unwrap_err();
    assert!(err.contains("LLVM-C.dll"));
    assert!(err.contains("PATH"));
}
