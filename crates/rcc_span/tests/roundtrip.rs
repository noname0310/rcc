use rcc_span::{BytePos, Interner, LineCol, SourceMap};
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
