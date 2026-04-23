//! Logical-line grouping over the pp-token stream.
//!
//! C99 §6.10 preamble: the preprocessor operates on *logical* source lines
//! (after translation phase 2 line-splicing). [`LineStream`] wraps a raw
//! pp-token iterator and exposes a [`next_line`](LineStream::next_line)
//! method that returns the tokens making up one logical line, terminated
//! by a [`PpTokenKind::Newline`] token.
//!
//! Line splicing (`\<newline>`) is already performed upstream by
//! [`rcc_lexer::LineSpliceCursor`], so every `Newline` the lexer emits is
//! by definition *not* preceded by a backslash. This iterator therefore
//! simply cuts the stream at each `Newline`.
//!
//! The terminator `Newline` token is **not** included in the returned
//! line; clients that need its span can rely on the next token's
//! [`PpToken::at_line_start`] flag being true instead.
//!
//! ### Blank line preservation
//!
//! Every physical `Newline` produces exactly one line emission, even if
//! the resulting line is empty. This preserves `__LINE__` bookkeeping and
//! gives diagnostics accurate line numbers. Examples:
//!
//! - `"\n\n\n"` yields three empty lines.
//! - `"a\n\nb\n"` yields `[a]`, `[]`, `[b]`.
//! - `"   \n"` yields `[]` (the lexer collapses horizontal whitespace).
//!
//! ### End-of-file
//!
//! If the source ends without a trailing newline, any buffered tokens
//! are returned as a final unterminated line, after which `next_line`
//! yields `None` forever. An entirely empty input yields `None` on the
//! first call.

use rcc_lexer::{PpToken, PpTokenKind};

/// Iterator adapter that groups a pp-token stream into logical source
/// lines. See the [module docs](self) for semantics.
pub struct LineStream<I: Iterator<Item = PpToken>> {
    iter: I,
    /// Set once the underlying iterator has returned `None` and any
    /// trailing unterminated line has been flushed. Subsequent calls
    /// to [`next_line`](Self::next_line) then return `None`.
    done: bool,
}

impl<I: Iterator<Item = PpToken>> LineStream<I> {
    /// Wrap an existing pp-token iterator.
    ///
    /// The typical call site is:
    ///
    /// ```ignore
    /// use rcc_lexer::tokenize;
    /// use rcc_preprocess::line_stream::LineStream;
    ///
    /// let mut ls = LineStream::new(tokenize(file_id, src));
    /// while let Some(line) = ls.next_line() {
    ///     // ...
    /// }
    /// ```
    pub fn new(iter: I) -> Self {
        Self { iter, done: false }
    }

    /// Return the next logical line, or `None` once the stream is
    /// fully consumed.
    ///
    /// The returned `Vec` contains every pp-token up to (but excluding)
    /// the terminating `Newline`. An empty `Vec` means a blank line.
    /// After EOF, any unterminated tail is flushed as a final line, and
    /// all subsequent calls return `None`.
    pub fn next_line(&mut self) -> Option<Vec<PpToken>> {
        if self.done {
            return None;
        }
        let mut line = Vec::new();
        loop {
            match self.iter.next() {
                Some(tok) if tok.kind == PpTokenKind::Newline => {
                    return Some(line);
                }
                Some(tok) => {
                    line.push(tok);
                }
                None => {
                    self.done = true;
                    // Flush an unterminated trailing line only if it
                    // actually held tokens; an empty buffer at EOF is
                    // just the end of the stream (not a phantom blank
                    // line).
                    return if line.is_empty() { None } else { Some(line) };
                }
            }
        }
    }
}

impl<I: Iterator<Item = PpToken>> Iterator for LineStream<I> {
    type Item = Vec<PpToken>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_line()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_lexer::{tokenize, Punct, StringEncoding};
    use rcc_span::FileId;

    /// Helper: build a `LineStream` directly from source text.
    fn line_stream(src: &str) -> LineStream<impl Iterator<Item = PpToken> + '_> {
        LineStream::new(tokenize(FileId(0), src))
    }

    /// Kinds-only view of a line, for easy assertions that don't care
    /// about spans or whitespace flags.
    fn kinds(line: &[PpToken]) -> Vec<PpTokenKind> {
        line.iter().map(|t| t.kind).collect()
    }

    #[test]
    fn include_angle_header() {
        // `#include <x>` — the `<x>` is NOT a HeaderName here because
        // the lexer only produces HeaderName on explicit `lex_header_name`
        // call. That happens in task 04-03; at this stage we just see
        // Hash, Ident, Lt, Ident, Gt on a single line.
        let mut ls = line_stream("#include <x>\n");
        let line = ls.next_line().expect("one line");
        assert_eq!(
            kinds(&line),
            vec![
                PpTokenKind::Punct(Punct::Hash),
                PpTokenKind::Ident,
                PpTokenKind::Punct(Punct::Lt),
                PpTokenKind::Ident,
                PpTokenKind::Punct(Punct::Gt),
            ]
        );
        // `#` must carry `at_line_start = true`; rest must not.
        assert!(line[0].at_line_start, "`#` should be at_line_start");
        for tok in &line[1..] {
            assert!(!tok.at_line_start, "non-leading tokens must not be at_line_start");
        }
        assert!(ls.next_line().is_none(), "no more lines after trailing newline");
    }

    #[test]
    fn simple_assignment_line() {
        // `a = b ;\n`
        let mut ls = line_stream("a = b ;\n");
        let line = ls.next_line().expect("one line");
        assert_eq!(
            kinds(&line),
            vec![
                PpTokenKind::Ident,
                PpTokenKind::Punct(Punct::Eq),
                PpTokenKind::Ident,
                PpTokenKind::Punct(Punct::Semi),
            ]
        );
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn blank_lines_preserved() {
        // `\n\n\n` — three terminators → three empty lines.
        let mut ls = line_stream("\n\n\n");
        for _ in 0..3 {
            let line = ls.next_line().expect("blank line");
            assert!(line.is_empty(), "blank line must have no tokens");
        }
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn whitespace_only_line_is_empty() {
        // Horizontal whitespace is collapsed by the lexer, so a
        // `   \n` line surfaces as an empty token vec — exactly what
        // `__LINE__` bookkeeping requires.
        let mut ls = line_stream("   \t  \n");
        let line = ls.next_line().expect("whitespace-only line");
        assert!(line.is_empty());
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn blank_line_between_code_lines() {
        // `a\n\nb\n` → [a], [], [b], None.
        let mut ls = line_stream("a\n\nb\n");
        assert_eq!(kinds(&ls.next_line().unwrap()), vec![PpTokenKind::Ident]);
        assert!(ls.next_line().unwrap().is_empty());
        assert_eq!(kinds(&ls.next_line().unwrap()), vec![PpTokenKind::Ident]);
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn empty_input_yields_none() {
        let mut ls = line_stream("");
        assert!(ls.next_line().is_none());
        // Idempotent.
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn unterminated_trailing_line_is_flushed() {
        // No trailing newline — the final line must still surface.
        let mut ls = line_stream("x");
        let line = ls.next_line().expect("unterminated final line");
        assert_eq!(kinds(&line), vec![PpTokenKind::Ident]);
        assert!(ls.next_line().is_none());
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn backslash_newline_is_line_continuation() {
        // Line splicing is done upstream at the character level in
        // `LineSpliceCursor`, so `a =\<newline> b<newline>` surfaces
        // as a single logical line carrying `a`, `=`, `b` with no
        // intervening `Newline` token.
        let mut ls = line_stream("a =\\\n b\n");
        let line = ls.next_line().expect("spliced line");
        assert_eq!(
            kinds(&line),
            vec![PpTokenKind::Ident, PpTokenKind::Punct(Punct::Eq), PpTokenKind::Ident]
        );
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn hash_only_at_line_start_on_first_token() {
        // `#` after other tokens on the same line is still a Punct::Hash
        // token, but its `at_line_start` flag is false — that is what
        // the directive parser uses to refuse to treat it as a
        // directive introducer.
        let mut ls = line_stream("a # b\n");
        let line = ls.next_line().expect("one line");
        assert_eq!(kinds(&line).len(), 3);
        assert!(!line[1].at_line_start, "mid-line `#` is not a directive");
        assert!(ls.next_line().is_none());
    }

    #[test]
    fn iterator_impl_agrees_with_next_line() {
        // Using `for line in LineStream::new(..)` must behave identically
        // to repeated `next_line` calls.
        let lines: Vec<_> = line_stream("a\n\nb\n").collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(kinds(&lines[0]), vec![PpTokenKind::Ident]);
        assert!(lines[1].is_empty());
        assert_eq!(kinds(&lines[2]), vec![PpTokenKind::Ident]);
    }

    #[test]
    fn string_literal_inside_line() {
        // Sanity: a string literal is one token and sits on a single
        // logical line alongside its neighbours.
        let mut ls = line_stream("return \"hi\";\n");
        let line = ls.next_line().expect("one line");
        assert_eq!(
            kinds(&line),
            vec![
                PpTokenKind::Ident,
                PpTokenKind::StringLit { enc: StringEncoding::None },
                PpTokenKind::Punct(Punct::Semi),
            ]
        );
        assert!(ls.next_line().is_none());
    }
}
