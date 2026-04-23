//! Include-guard detection.
//!
//! Recognises the canonical `#ifndef X / #define X / ... / #endif`
//! idiom that wraps an entire header. When a file matches, subsequent
//! `#include`s of it can be elided without re-tokenising — Clang calls
//! this the *multiple-include optimization*. Only the *shape* of the
//! guard is validated here; body-level semantics (is `X` already
//! `#define`d? has it since been `#undef`ed?) are decided at the
//! inclusion site by the preprocessor.
//!
//! ### What counts as a guard
//!
//! The guard must cover the **entire** translation unit of the
//! header:
//!
//! - The first non-blank logical line is `#ifndef NAME`.
//! - The second non-blank logical line is `#define NAME ...`
//!   (replacement list is allowed but irrelevant).
//! - The last non-blank logical line is `#endif`.
//!
//! Any stray token before `#ifndef`, between `#endif` and EOF, or a
//! mismatched guard name defeats the optimisation — the header is
//! then processed normally on every inclusion.
//!
//! Blank lines (newlines with no intervening tokens) are ignored:
//! `rcc_lexer::tokenize` collapses horizontal whitespace and comments
//! in its default mode, so "blank" here means a token run between
//! two consecutive `Newline`s is empty.

use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{Interner, Symbol};

/// Return the guard macro name if `tokens` (a header's full pp-token
/// stream, as produced by `rcc_lexer::tokenize`) matches the canonical
/// `#ifndef / #define / #endif` wrapper. `src` is the source text the
/// tokens were lexed from; `interner` receives the guard identifier.
///
/// See the [module docs](self) for the exact acceptance rules.
pub fn detect_guard(tokens: &[PpToken], src: &str, interner: &mut Interner) -> Option<Symbol> {
    let lines = logical_lines(tokens);
    if lines.len() < 3 {
        return None;
    }

    let first = lines[0];
    let second = lines[1];
    let last = *lines.last().expect("lines.len() >= 3");

    // `#ifndef NAME` — the opening guard must be exactly three tokens:
    // the hash, the `ifndef` identifier, and the guard name. Extra
    // trailing tokens on the same line (e.g. `#ifndef NAME foo`) are
    // syntactically ill-formed and would be caught by the directive
    // parser; they also disqualify the guard shape here.
    if first.len() != 3 || !is_hash(&first[0]) || !ident_is(&first[1], src, "ifndef") {
        return None;
    }
    if first[2].kind != PpTokenKind::Ident {
        return None;
    }
    let guard_text = token_text(&first[2], src);

    // `#define NAME [replacement-list]` — the macro name must match
    // the `#ifndef` name. A non-empty replacement list is permitted
    // (the value is discarded for guard purposes).
    if second.len() < 3 || !is_hash(&second[0]) || !ident_is(&second[1], src, "define") {
        return None;
    }
    if second[2].kind != PpTokenKind::Ident || token_text(&second[2], src) != guard_text {
        return None;
    }

    // Bare `#endif`. Trailing tokens on the `#endif` line (e.g.
    // `#endif /* NAME */` survives only because the comment was
    // collapsed by the lexer) are rejected — if anything is left
    // after the `endif` identifier, we bail.
    if last.len() != 2 || !is_hash(&last[0]) || !ident_is(&last[1], src, "endif") {
        return None;
    }

    Some(interner.intern(guard_text))
}

/// Split `tokens` at `Newline` boundaries and drop blank lines. The
/// terminating `Newline`s themselves are discarded; a trailing
/// unterminated line (no final `Newline`) is still returned if it
/// carries any tokens. This mirrors the semantics of
/// [`crate::line_stream::LineStream`] but works directly on a slice
/// instead of an iterator, which is what `detect_guard` needs.
fn logical_lines(tokens: &[PpToken]) -> Vec<&[PpToken]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind == PpTokenKind::Newline {
            if start < i {
                out.push(&tokens[start..i]);
            }
            start = i + 1;
        }
    }
    if start < tokens.len() {
        out.push(&tokens[start..]);
    }
    out
}

fn is_hash(tok: &PpToken) -> bool {
    // A `#` only introduces a directive when it starts the logical
    // line (C99 §6.10 p1). Mid-line `#`s are rejected so that source
    // like `x #ifndef G` cannot be mistaken for a guard.
    tok.kind == PpTokenKind::Punct(Punct::Hash) && tok.at_line_start
}

fn ident_is(tok: &PpToken, src: &str, want: &str) -> bool {
    tok.kind == PpTokenKind::Ident && token_text(tok, src) == want
}

fn token_text<'a>(tok: &PpToken, src: &'a str) -> &'a str {
    &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_lexer::tokenize;
    use rcc_span::FileId;

    fn tokens_of(src: &str) -> Vec<PpToken> {
        tokenize(FileId(0), src).collect()
    }

    /// `ok.h` — the canonical header shape every C project uses. Must
    /// be detected and produce the correct guard symbol.
    #[test]
    fn ok_h_canonical_guard() {
        let src = "#ifndef OK_H\n#define OK_H\nint ok;\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        let sym = detect_guard(&toks, src, &mut interner);
        assert_eq!(sym, Some(interner.intern("OK_H")));
    }

    /// `bad.h` — stray token before `#ifndef` disqualifies the guard.
    /// The file must still be considered includable (the caller falls
    /// back to full processing), so `detect_guard` returns `None`.
    #[test]
    fn bad_h_stray_token_before_ifndef() {
        let src = "int stray;\n#ifndef BAD_H\n#define BAD_H\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn guard_with_define_replacement_list_still_detected() {
        // `#define G 1` is a perfectly valid guard — the replacement
        // list is irrelevant to the skip optimisation.
        let src = "#ifndef G\n#define G 1\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert_eq!(detect_guard(&toks, src, &mut interner), Some(interner.intern("G")));
    }

    #[test]
    fn blank_lines_around_directives_are_ignored() {
        let src = "\n\n#ifndef G\n\n#define G\n\nint x;\n\n#endif\n\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert_eq!(detect_guard(&toks, src, &mut interner), Some(interner.intern("G")));
    }

    #[test]
    fn mismatched_guard_names_rejected() {
        // `#ifndef A` followed by `#define B` is not a guard — neither
        // clang nor gcc treat it as one.
        let src = "#ifndef A\n#define B\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn stray_token_after_endif_rejected() {
        // Anything on the far side of `#endif` breaks the
        // "covers the entire file" requirement.
        let src = "#ifndef G\n#define G\n#endif\nint trail;\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn missing_define_rejected() {
        let src = "#ifndef G\nint x;\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn missing_endif_rejected() {
        let src = "#ifndef G\n#define G\nint x;\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn empty_file_rejected() {
        let toks = tokens_of("");
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, "", &mut interner).is_none());
    }

    #[test]
    fn hash_not_at_line_start_rejected() {
        // `x #ifndef G` keeps the `#` as a punctuator but with
        // `at_line_start = false`; a stray leading token also
        // disqualifies the guard independently.
        let src = "x #ifndef G\n#define G\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn endif_with_trailing_token_rejected() {
        // `#endif G` (extra ident on the endif line) is malformed by
        // C99 §6.10.1 and also forbidden under our guard shape.
        let src = "#ifndef G\n#define G\n#endif G\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }
}
