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
