use std::sync::{Arc, RwLock};

use rcc_errors::{include_chain_notes, Diagnostic, Label, Level, StderrEmitter};
use rcc_span::{BytePos, FileId, SourceMap, Span};

fn make_source_map() -> Arc<RwLock<SourceMap>> {
    let mut sm = SourceMap::new();
    sm.add_file("test.c".into(), Arc::from("int main() {\n    return 0;\n}\n"));
    Arc::new(RwLock::new(sm))
}

#[test]
fn single_file() {
    let sm = make_source_map();
    let emitter = StderrEmitter::new(sm).with_color(false);

    let diag = Diagnostic {
        level: Level::Error,
        code: Some("E0001"),
        message: "unexpected token".into(),
        labels: vec![
            Label {
                span: Span::new(FileId(0), BytePos(4), BytePos(8)),
                message: "expected `;`".into(),
                primary: true,
            },
            Label {
                span: Span::new(FileId(0), BytePos(18), BytePos(24)),
                message: "in this function".into(),
                primary: false,
            },
        ],
        notes: vec!["try adding `;` at end".into()],
        help: vec!["see C99 §6.7".into()],
    };

    let output = emitter.render_to_string(&diag);
    insta::assert_snapshot!("render__single_file", output);
}

#[test]
fn no_color_disables_ansi() {
    let sm = make_source_map();
    let emitter = StderrEmitter::new(sm).with_color(false);

    let diag = Diagnostic {
        level: Level::Error,
        code: None,
        message: "test error".into(),
        labels: vec![Label {
            span: Span::new(FileId(0), BytePos(0), BytePos(3)),
            message: "here".into(),
            primary: true,
        }],
        notes: vec![],
        help: vec![],
    };

    let output = emitter.render_to_string(&diag);
    assert!(
        !output.contains("\x1b["),
        "output must not contain ANSI escape codes when colour is disabled"
    );
}

#[test]
fn multi_file() {
    let mut sm = SourceMap::new();
    // FileId(0): main.c
    sm.add_file(
        "main.c".into(),
        Arc::from("#include \"header.h\"\nint main() { return foo(); }\n"),
    );
    // FileId(1): header.h
    sm.add_file("header.h".into(), Arc::from("int foo(void);\n"));
    let sm = Arc::new(RwLock::new(sm));
    let emitter = StderrEmitter::new(sm).with_color(false);

    let notes = include_chain_notes(&["header.h", "main.c"]);

    let diag = Diagnostic {
        level: Level::Error,
        code: Some("E0002"),
        message: "implicit declaration of function 'foo'".into(),
        labels: vec![
            Label {
                span: Span::new(FileId(0), BytePos(40), BytePos(43)),
                message: "called here".into(),
                primary: true,
            },
            Label {
                span: Span::new(FileId(1), BytePos(4), BytePos(7)),
                message: "declared here".into(),
                primary: false,
            },
        ],
        notes,
        help: vec![],
    };

    let output = emitter.render_to_string(&diag);
    insta::assert_snapshot!("multi_file", output);
}
