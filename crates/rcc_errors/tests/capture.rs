use rcc_errors::{CaptureEmitter, Handler, Level};
use rcc_span::{BytePos, FileId, Span};

#[test]
fn capture_emitter_records_builder_output() {
    let cap = CaptureEmitter::new();
    let mut h = Handler::with_emitter(Box::new(cap.clone()));

    let sp = Span::new(FileId(0), BytePos(0), BytePos(3));
    h.struct_err(sp, "unexpected token").code("E0001").note("try `;` at end").emit();

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, Level::Error);
    assert_eq!(diags[0].code, Some("E0001"));
    assert!(h.has_errors());
    assert_eq!(h.error_count(), 1);
}
