//! Shared lexer-test utilities.
//!
//! Task 03-lex/10 asks for a single entry point the per-category
//! table-driven tests can build on. We want it to stay compile-gated
//! to `cargo test` so it cannot leak into release binaries or into the
//! public API surface seen by downstream crates.
//!
//! Integration tests in `tests/` cannot see `#[cfg(test)]` items of
//! their host library (they build against the non-test rlib), so a
//! near-identical mirror lives at `tests/common/mod.rs` for the six
//! category files to share. Keeping both copies trivially small makes
//! the duplication cost negligible.
#![cfg(test)]
#![allow(missing_docs)]

use crate::{PpTokenKind, Tokenizer};
use rcc_span::FileId;

/// Run `src` through the default-configured [`Tokenizer`] and pair
/// every emitted token with the source slice its `span` refers to.
///
/// This is the shape expected by phase-03 table-driven unit tests:
/// `&[(PpTokenKind, &str)]` rows are easy to write, easy to read, and
/// trivially exhaustive against each category's grammar.
pub(crate) fn lex_all(src: &str) -> Vec<(PpTokenKind, &str)> {
    Tokenizer::new(FileId(0), src)
        .map(|t| (t.kind, &src[t.span.lo.0 as usize..t.span.hi.0 as usize]))
        .collect()
}

#[test]
fn lex_all_returns_kind_and_source_slice() {
    let v = lex_all("foo");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].0, PpTokenKind::Ident);
    assert_eq!(v[0].1, "foo");
}

#[test]
fn lex_all_slice_matches_span_for_each_token() {
    let src = "\"hi\"";
    let v = lex_all(src);
    assert_eq!(v.len(), 1);
    assert!(matches!(v[0].0, PpTokenKind::StringLit { .. }));
    assert_eq!(v[0].1, src);
}

#[test]
fn lex_all_preserves_source_order() {
    let src = "a 1";
    let v = lex_all(src);
    // Two non-whitespace tokens, in source order.
    let non_ws: Vec<_> = v
        .iter()
        .filter(|(k, _)| !matches!(k, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(non_ws.len(), 2);
    assert_eq!(non_ws[0].1, "a");
    assert_eq!(non_ws[1].1, "1");
}
