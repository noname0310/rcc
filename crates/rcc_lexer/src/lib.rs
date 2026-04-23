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

use rcc_errors::{
    codes::{E0003, E0004},
    Handler,
};
use rcc_span::{BytePos, FileId, Span};

mod cursor;
mod kinds;
mod line_splice;

pub use cursor::Cursor;
pub use kinds::{PpNumberKind, PpTokenKind, Punct, StringEncoding};
pub use line_splice::LineSpliceCursor;

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
/// tokens lazily without allocation. Whitespace is collapsed; use
/// [`Tokenizer::preserve_whitespace`] for debug / `--emit=tokens` mode.
pub fn tokenize<'a>(file: FileId, src: &'a str) -> impl Iterator<Item = PpToken> + 'a {
    Tokenizer::new(file, src)
}

/// Streaming tokenizer. Internal; exposed for `cargo fuzz` targets and
/// for the driver's `--emit=tokens` debug mode.
pub struct Tokenizer<'a> {
    file: FileId,
    cursor: LineSpliceCursor<'a>,
    at_line_start: bool,
    leading_ws: bool,
    preserve_whitespace: bool,
    handler: Option<&'a mut Handler>,
}

impl<'a> Tokenizer<'a> {
    /// Build a new tokenizer with whitespace collapsing enabled and no
    /// diagnostic handler attached.
    pub fn new(file: FileId, src: &'a str) -> Self {
        Self {
            file,
            cursor: LineSpliceCursor::new(src),
            at_line_start: true,
            leading_ws: false,
            preserve_whitespace: false,
            handler: None,
        }
    }

    /// Attach a diagnostic handler; needed to emit E0003/E0004 and any
    /// future lexer-level errors.
    pub fn with_handler(mut self, handler: &'a mut Handler) -> Self {
        self.handler = Some(handler);
        self
    }

    /// Enable or disable whitespace preservation. When enabled, runs of
    /// horizontal whitespace and comments are emitted as single
    /// [`PpTokenKind::Whitespace`] tokens spanning the full run; when
    /// disabled (default) they are silently consumed. Newlines are
    /// always emitted regardless.
    pub fn preserve_whitespace(mut self, yes: bool) -> Self {
        self.preserve_whitespace = yes;
        self
    }

    /// Current physical byte position.
    fn pos(&self) -> BytePos {
        BytePos(self.cursor.offset() as u32)
    }

    fn span_from(&self, lo: BytePos) -> Span {
        Span::new(self.file, lo, self.pos())
    }

    fn make_token(&mut self, kind: PpTokenKind, lo: BytePos) -> PpToken {
        let leading_ws = self.leading_ws;
        let at_line_start = self.at_line_start;
        // After emitting any non-whitespace/non-newline token the next
        // token is *not* at the start of a logical line, and has no
        // leading whitespace by default.
        match kind {
            PpTokenKind::Whitespace | PpTokenKind::Newline => {}
            _ => {
                self.at_line_start = false;
            }
        }
        self.leading_ws = false;
        PpToken { kind, span: self.span_from(lo), leading_ws, at_line_start }
    }
}

/// Classification of the outcome of scanning a `/* ... */` block.
enum BlockOutcome {
    /// Matching `*/` consumed.
    Closed,
    /// End of file hit before `*/`; E0004 already emitted.
    Eof,
}

impl Iterator for Tokenizer<'_> {
    type Item = PpToken;

    fn next(&mut self) -> Option<PpToken> {
        loop {
            let lo = self.pos();
            let first = self.cursor.first()?;

            match first {
                // ── Physical newlines ─────────────────────────────────
                '\n' => {
                    self.cursor.bump();
                    let tok = self.make_token(PpTokenKind::Newline, lo);
                    self.at_line_start = true;
                    self.leading_ws = false;
                    return Some(tok);
                }
                '\r' => {
                    self.cursor.bump();
                    if self.cursor.first() == Some('\n') {
                        self.cursor.bump();
                    }
                    let tok = self.make_token(PpTokenKind::Newline, lo);
                    self.at_line_start = true;
                    self.leading_ws = false;
                    return Some(tok);
                }

                // ── Horizontal whitespace run ─────────────────────────
                c if is_horizontal_ws(c) => {
                    self.cursor.eat_while(is_horizontal_ws);
                    self.leading_ws = true;
                    if self.preserve_whitespace {
                        return Some(self.make_token(PpTokenKind::Whitespace, lo));
                    }
                    continue;
                }

                // ── `//` line comment ─────────────────────────────────
                '/' if self.cursor.second() == Some('/') => {
                    self.cursor.bump(); // '/'
                    self.cursor.bump(); // '/'
                                        // Eat until (but NOT including) the next physical newline.
                    self.cursor.eat_while(|c| c != '\n' && c != '\r');
                    self.leading_ws = true;
                    if self.preserve_whitespace {
                        return Some(self.make_token(PpTokenKind::Whitespace, lo));
                    }
                    continue;
                }

                // ── `/* ... */` block comment ─────────────────────────
                '/' if self.cursor.second() == Some('*') => {
                    self.cursor.bump(); // '/'
                    self.cursor.bump(); // '*'
                    let opening = Span::new(self.file, lo, self.pos());
                    match self.scan_block_comment(opening) {
                        BlockOutcome::Closed | BlockOutcome::Eof => {}
                    }
                    self.leading_ws = true;
                    if self.preserve_whitespace {
                        return Some(self.make_token(PpTokenKind::Whitespace, lo));
                    }
                    // On EOF within an unterminated comment we still
                    // need to drop out of the loop cleanly — the next
                    // iteration's `cursor.first()?` will do that.
                    continue;
                }

                // ── Fallback: single-char Unknown. ────────────────────
                // Real recognisers (ident, pp-number, punct, literals)
                // land in tasks 03-lex/04..08.
                _ => {
                    self.cursor.bump();
                    return Some(self.make_token(PpTokenKind::Unknown, lo));
                }
            }
        }
    }
}

impl Tokenizer<'_> {
    /// Scan the body of a `/* ... */` block comment (the opening `/*`
    /// has already been consumed). Emits E0003 at most once per comment
    /// for any nested `/*`, and E0004 if EOF is reached first.
    fn scan_block_comment(&mut self, opening: Span) -> BlockOutcome {
        let mut nested_reported = false;
        loop {
            let Some(c) = self.cursor.first() else {
                // EOF inside a block comment.
                if let Some(h) = self.handler.as_deref_mut() {
                    h.struct_err(opening, "unterminated block comment")
                        .code(E0004)
                        .primary(opening, "this `/*` was never closed")
                        .help("insert `*/` before the end of the file")
                        .emit();
                }
                return BlockOutcome::Eof;
            };

            // Closing `*/`.
            if c == '*' && self.cursor.second() == Some('/') {
                self.cursor.bump(); // '*'
                self.cursor.bump(); // '/'
                return BlockOutcome::Closed;
            }

            // Nested `/*`: record its span and emit E0003 once.
            if c == '/' && self.cursor.second() == Some('*') && !nested_reported {
                let nested_lo = self.pos();
                self.cursor.bump(); // '/'
                self.cursor.bump(); // '*'
                let nested_span = Span::new(self.file, nested_lo, self.pos());
                if let Some(h) = self.handler.as_deref_mut() {
                    h.struct_err(nested_span, "`/*` within block comment")
                        .code(E0003)
                        .primary(nested_span, "nested `/*` starts here")
                        .label(opening, "outer comment opened here")
                        .note("C99 block comments do not nest (§6.4.9)")
                        .emit();
                }
                nested_reported = true;
                continue;
            }

            // Plain content — just advance. Note: newlines inside a
            // block comment are *not* emitted as `Newline` tokens; the
            // entire comment reduces to a single space per C99
            // §5.1.1.2 phase 3, so directive boundaries ignore them.
            self.cursor.bump();
        }
    }
}

/// C99 horizontal whitespace: space, horizontal tab, vertical tab,
/// form feed. `\n` / `\r` are handled separately as newlines.
fn is_horizontal_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\x0B' | '\x0C')
}
