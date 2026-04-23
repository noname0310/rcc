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
    codes::{E0003, E0004, E0005},
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

                // ── Identifier (ASCII start) ──────────────────────────
                c if is_ident_start_ascii(c) => {
                    self.cursor.bump();
                    self.scan_ident_body();
                    return Some(self.make_token(PpTokenKind::Ident, lo));
                }

                // ── Identifier (UCN start) ────────────────────────────
                // `\uXXXX` / `\UXXXXXXXX` can legally start a C99
                // identifier (C99 §6.4.2.1). We scan the UCN, validate
                // the code point, and then continue the identifier
                // body loop as usual.
                '\\' if matches!(self.cursor.second(), Some('u' | 'U')) => {
                    let u = self.scan_ucn();
                    self.validate_ident_ucn(&u);
                    self.scan_ident_body();
                    return Some(self.make_token(PpTokenKind::Ident, lo));
                }

                // ── Fallback: single-char Unknown. ────────────────────
                // Real recognisers (pp-number, punct, literals) land
                // in tasks 03-lex/05..08.
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

/// Outcome of [`Tokenizer::scan_ucn`]: the literal bytes consumed, the
/// span they cover, and the decoded code point (if 4/8 hex digits were
/// actually read). A `None` code means the UCN was truncated by
/// non-hex input or EOF.
struct ScannedUcn {
    /// Source-text bytes consumed, always starting with `\u` or `\U`.
    bytes: String,
    /// Span of those bytes in the original source.
    span: Span,
    /// Decoded code point, or `None` if fewer than the required
    /// 4 / 8 hex digits were read.
    code: Option<u32>,
}

impl Tokenizer<'_> {
    /// Consume the greedy body of an identifier after its first
    /// character has already been consumed. Terminates on the first
    /// non-ident character. Embedded UCNs are scanned and validated
    /// inline; they count as part of the identifier even if they are
    /// ill-formed (the error-recovery strategy is to diagnose but not
    /// truncate the token).
    fn scan_ident_body(&mut self) {
        loop {
            match self.cursor.first() {
                Some(c) if is_ident_continue_ascii(c) => {
                    self.cursor.bump();
                }
                Some('\\') if matches!(self.cursor.second(), Some('u' | 'U')) => {
                    let u = self.scan_ucn();
                    self.validate_ident_ucn(&u);
                }
                _ => break,
            }
        }
    }

    /// Scan a single `\uXXXX` or `\UXXXXXXXX` escape. The cursor must
    /// be positioned on the leading backslash; on return it is past
    /// whatever hex digits were consumed. No diagnostic is emitted
    /// here — [`validate_ident_ucn`] is the caller's responsibility.
    ///
    /// [`validate_ident_ucn`]: Self::validate_ident_ucn
    fn scan_ucn(&mut self) -> ScannedUcn {
        let lo = self.pos();
        let mut bytes = String::new();
        let bs = self.cursor.bump().expect("UCN scan called without `\\`");
        bytes.push(bs);
        let u_or_big_u = self.cursor.bump().expect("UCN scan called without `u`/`U`");
        bytes.push(u_or_big_u);
        let need = if u_or_big_u == 'u' { 4 } else { 8 };

        let mut code: u32 = 0;
        let mut got = 0;
        while got < need {
            match self.cursor.first().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    let ch = self.cursor.bump().unwrap();
                    bytes.push(ch);
                    code = (code << 4) | d;
                    got += 1;
                }
                None => break,
            }
        }

        let span = self.span_from(lo);
        let code = if got == need { Some(code) } else { None };
        ScannedUcn { bytes, span, code }
    }

    /// Emit E0005 if a scanned UCN is ill-formed (wrong digit count)
    /// or if its code point is disallowed for the identifier position
    /// by C99 §6.4.3 (constraint list) and §6.4.2.1 (Annex D).
    fn validate_ident_ucn(&mut self, u: &ScannedUcn) {
        match u.code {
            None => self.emit_ucn_error(u, "malformed universal character name"),
            Some(cp) if !is_allowed_ucn_in_ident(cp) => {
                self.emit_ucn_error(u, "universal character name not allowed in identifier")
            }
            Some(_) => {}
        }
    }

    fn emit_ucn_error(&mut self, u: &ScannedUcn, message: &str) {
        if let Some(h) = self.handler.as_deref_mut() {
            h.struct_err(u.span, message)
                .code(E0005)
                .primary(u.span, "this is not a well-formed universal character name")
                .help(format!(
                    "universal character names must be `\\uXXXX` with 4 hex digits or \
                     `\\UXXXXXXXX` with 8 hex digits; got `{}`",
                    u.bytes
                ))
                .note(
                    "C99 §6.4.3 constrains UCN code points (no surrogates, no \
                     short-identifier < 0x00A0 except 0x24/0x40/0x60); identifiers \
                     further restrict them to Annex D ranges",
                )
                .emit();
        }
    }
}

/// ASCII subset of C99 identifier-start characters: underscore and
/// letters. UCN starts are handled by the caller.
fn is_ident_start_ascii(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

/// ASCII subset of C99 identifier-continue characters: underscore,
/// letters, and digits. UCN continuations are handled by the caller.
fn is_ident_continue_ascii(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

/// Whether a UCN-decoded Unicode code point is permitted inside a C99
/// identifier.
///
/// This is the intersection of two rules:
/// - C99 §6.4.3 constraint list: no surrogates (D800..=DFFF), and no
///   short identifier < 0x00A0 except the three exceptions 0x0024
///   (`$`), 0x0040 (`@`), 0x0060 (`` ` ``).
/// - C99 §6.4.2.1 identifier rule: each UCN in an identifier must
///   designate a character in one of the Annex D ranges — which
///   *excludes* the three §6.4.3 exceptions.
///
/// For the purposes of the lexer we approximate Annex D by the
/// conservative rule "code point ≥ 0x00A0 and not a surrogate and
/// within Unicode scalar range". This correctly rejects `\u0024` /
/// `\u0040` / `\u0060` in identifier position, as well as all control
/// characters below U+00A0, while accepting the letter-like ranges
/// used in practice. A tighter Annex-D whitelist is deferred to a
/// future identifier-body hardening task.
fn is_allowed_ucn_in_ident(cp: u32) -> bool {
    (0x00A0..=0x10_FFFF).contains(&cp) && !(0xD800..=0xDFFF).contains(&cp)
}
