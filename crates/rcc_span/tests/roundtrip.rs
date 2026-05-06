use rcc_span::{BytePos, Interner, LineCol, SourceMap};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn interner_identity() {
    let mut i = Interner::new();
    let a = i.intern("foo");
    let b = i.intern("foo");
    let c = i.intern("bar");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(i.get(a), "foo");
    assert_eq!(i.get(c), "bar");
}

#[test]
fn source_map_line_col() {
    let mut sm = SourceMap::new();
    let src: Arc<str> = Arc::from("abc\ndef\n\nghij");
    let id = sm.add_file(PathBuf::from("<test>"), src);
    assert_eq!(sm.lookup_line_col(id, BytePos(0)), LineCol { line: 1, col: 1 });
    assert_eq!(sm.lookup_line_col(id, BytePos(2)), LineCol { line: 1, col: 3 });
    assert_eq!(sm.lookup_line_col(id, BytePos(4)), LineCol { line: 2, col: 1 });
    assert_eq!(sm.lookup_line_col(id, BytePos(8)), LineCol { line: 3, col: 1 });
    assert_eq!(sm.lookup_line_col(id, BytePos(9)), LineCol { line: 4, col: 1 });
}

#[test]
fn load_file_accepts_non_utf8_bytes() {
    let dir = std::env::temp_dir().join(format!("rcc-span-non-utf8-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("generated.h");
    fs::write(&path, b"int x;\n// raw byte: \xff\nint y;\n").expect("write non-utf8 file");

    let mut sm = SourceMap::new();
    let id = sm.load_file(&path).expect("load non-utf8 C source");
    let file = sm.file(id);
    assert!(file.src.contains("// raw byte: \u{ff}"));
    assert!(file.src.contains("int y;"));

    fs::remove_dir_all(&dir).expect("remove temp dir");
}
