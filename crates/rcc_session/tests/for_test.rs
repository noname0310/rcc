use std::path::PathBuf;
use std::sync::Arc;

use rcc_errors::Level;
use rcc_session::Session;
use rcc_span::{BytePos, FileId, Span};

#[test]
fn for_test_plumbs_capture_emitter() {
    let (mut sess, cap) = Session::for_test();

    let sp = Span::new(FileId(0), BytePos(0), BytePos(3));
    sess.handler.struct_err(sp, "test error").code("E0001").emit();

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, Level::Error);
    assert_eq!(diags[0].code, Some("E0001"));
    assert!(sess.handler.has_errors());
    assert_eq!(sess.handler.error_count(), 1);
}

#[test]
fn for_test_starts_with_no_errors() {
    let (sess, cap) = Session::for_test();
    assert!(!sess.handler.has_errors());
    assert_eq!(sess.handler.error_count(), 0);
    assert!(cap.diagnostics().is_empty());
}

#[test]
fn virtual_files_load_through_source_map() {
    let (sess, _cap) = Session::for_test();
    let path = PathBuf::from("__rcc_vfs__/header.h");
    sess.add_virtual_file(path.clone(), Arc::from("int from_virtual;\n"));

    assert!(sess.has_virtual_file(&path));
    let file = sess.load_source_file(&path).expect("load virtual file");
    let sm = sess.source_map.read().unwrap();
    let registered = sm.file(file);
    assert_eq!(registered.name, path);
    assert_eq!(&*registered.src, "int from_virtual;\n");
}
