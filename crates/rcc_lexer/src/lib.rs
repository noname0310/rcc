//! `rcc_lexer`: C preprocessing-token (pp-token) lexer.
//!
//! Analogous to `rustc_lexer`. Produces the **pp-token** stream defined by
//! C99 §6.4; `rcc_preprocess` consumes it and `rcc_parse` converts it into
//! full C tokens at phase 7.
//!
//! This is an *allocation-free* streaming lexer: token text is not copied,
//! only `Span`s are produced. Interning is a later concern of the parser.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_span::{BytePos, FileId, Span};

mod cursor;
mod kinds;

pub use cursor::Cursor;
pub use kinds::{PpNumberKind, PpTokenKind, Punct, StringEncoding};

/// A single preprocessing token.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PpToken {
    /// Token kind.
    pub kind: PpTokenKind,
    /// Source span.
    pub span: Span,
    /// Whether the token is preceded by whitespace (needed for `#` / `##`).
    pub leading_ws: bool,
    /// Whether the token sits at the beginning of a logical line (for `#`).
    pub at_line_start: bool,
}

/// Tokenise an entire file into pp-tokens. The returned iterator yields
/// tokens lazily without allocation.
pub fn tokenize<'a>(file: FileId, src: &'a str) -> impl Iterator<Item = PpToken> + 'a {
    Tokenizer::new(file, src)
}

/// Streaming tokenizer. Internal; exposed for `cargo fuzz` targets.
pub struct Tokenizer<'a> {
    file: FileId,
    src: &'a str,
    cursor: Cursor<'a>,
    at_line_start: bool,
}

impl<'a> Tokenizer<'a> {
    /// Build a new tokenizer.
    pub fn new(file: FileId, src: &'a str) -> Self {
        Self { file, src, cursor: Cursor::new(src), at_line_start: true }
    }

    /// Current byte position.
    fn pos(&self) -> BytePos {
        BytePos(self.cursor.offset() as u32)
    }

    fn make_span(&self, lo: BytePos) -> Span {
        Span::new(self.file, lo, self.pos())
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = PpToken;

    fn next(&mut self) -> Option<PpToken> {
        // M1 scope note: this MVP emits EOF-only. A real implementation
        // recognises identifiers, pp-numbers, char/string literals, punctuators,
        // whitespace, newlines, line-splicing, and header-names. Kept as a
        // well-specified stub so downstream crates can be wired up now.
        let _ = (&self.src, &mut self.at_line_start);
        let lo = self.pos();
        if self.cursor.is_eof() {
            return None;
        }
        // Consume one byte and emit an `Unknown` token to avoid infinite loops
        // in any caller that runs before the real impl is in place.
        self.cursor.bump();
        Some(PpToken {
            kind: PpTokenKind::Unknown,
            span: self.make_span(lo),
            leading_ws: false,
            at_line_start: self.at_line_start,
        })
    }
}
