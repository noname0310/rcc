//! Shared helpers for the per-category integration tests (task 03-lex/10).
//!
//! Deliberately duplicated from `src/test_util.rs`: `#[cfg(test)]` items
//! in the library crate are invisible to integration test binaries,
//! which build against the non-test rlib. Keeping the mirror tiny keeps
//! the duplication cost negligible.
//!
//! Cargo treats subdirectories under `tests/` as non-targets, so this
//! file is *not* compiled as its own test binary — each integration
//! test brings it in with `mod common;`.

#![allow(dead_code)] // not every category uses every helper

use rcc_errors::Diagnostic;
use rcc_lexer::{PpToken, PpTokenKind, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

/// Mirror of `rcc_lexer::test_util::lex_all`: tokenise `src` with the
/// default-configured tokenizer and pair every emitted token with the
/// source slice its `span` refers to.
pub fn lex_all(src: &str) -> Vec<(PpTokenKind, &str)> {
    Tokenizer::new(FileId(0), src)
        .map(|t| (t.kind, &src[t.span.lo.0 as usize..t.span.hi.0 as usize]))
        .collect()
}

/// Same as [`lex_all`] but returns the full `PpToken` so spans are
/// available to callers that need them.
pub fn lex_tokens(src: &str) -> Vec<PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

/// Drop `Whitespace` and `Newline` tokens from a `lex_all` result, so
/// positive-case tables can focus on the grammar under test.
pub fn non_trivia(v: Vec<(PpTokenKind, &str)>) -> Vec<(PpTokenKind, &str)> {
    v.into_iter()
        .filter(|(k, _)| !matches!(k, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect()
}

/// Run `src` with a capturing diagnostic handler and return the raw
/// diagnostics. Callers typically project `.code` out for negative-case
/// tables.
pub fn run_diags(src: &str) -> Vec<Diagnostic> {
    let (mut sess, cap) = Session::for_test();
    let _: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();
    cap.diagnostics()
}

/// Project the diagnostic-code field from [`run_diags`] into a Vec of
/// `Option<&str>`-unwrapped codes. Makes it ergonomic to assert with
/// `assert!(codes.contains(&E0007))`.
pub fn diag_codes(src: &str) -> Vec<&'static str> {
    run_diags(src).into_iter().filter_map(|d| d.code).collect()
}
