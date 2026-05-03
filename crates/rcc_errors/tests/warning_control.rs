use rcc_errors::{codes, CaptureEmitter, Handler, Level, WarningConfig};
use rcc_span::{BytePos, FileId, Span};

fn span() -> Span {
    Span::new(FileId(0), BytePos(0), BytePos(1))
}

#[test]
fn werror_promotes_warning_to_error_count() {
    let cap = CaptureEmitter::new();
    let mut handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut config = WarningConfig::default();
    config.set_warnings_as_errors(true);
    handler.set_warning_config(config);

    handler.struct_warn(span(), "GNU extension").code(codes::W0013).emit();

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].level, Level::Error);
    assert_eq!(diags[0].code, Some(codes::W0013));
    assert_eq!(handler.error_count(), 1);
    assert_eq!(handler.warning_count(), 0);
    assert!(handler.has_errors());
}

#[test]
fn suppress_all_drops_warnings() {
    let cap = CaptureEmitter::new();
    let mut handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut config = WarningConfig::default();
    config.suppress_all();
    handler.set_warning_config(config);

    handler.struct_warn(span(), "GNU extension").code(codes::W0013).emit();

    assert!(cap.diagnostics().is_empty());
    assert_eq!(handler.error_count(), 0);
    assert_eq!(handler.warning_count(), 0);
}

#[test]
fn named_suppression_matches_warning_alias() {
    let cap = CaptureEmitter::new();
    let mut handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut config = WarningConfig::default();
    config.disable_warning("gnu-statement-expression");
    handler.set_warning_config(config);

    handler.struct_warn(span(), "GNU extension").code(codes::W0013).emit();

    assert!(cap.diagnostics().is_empty());
    assert_eq!(handler.warning_count(), 0);
}
