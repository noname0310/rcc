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
    codes::{E0001, E0003, E0004, E0005, E0006, E0007, E0008, E0010},
    Handler,
};
use rcc_span::{BytePos, FileId, Span};

mod cursor;
mod kinds;
mod line_splice;
pub mod pretty;
#[cfg(test)]
mod test_util;

pub use cursor::Cursor;
use kinds::Punct as P;
pub use kinds::{PpNumberKind, PpTokenKind, Punct, StringEncoding};
pub use line_splice::{strip_line_splices, LineSpliceCursor};

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

                // ── Character constant, optionally prefixed ───────────
                // C99 §6.4.4.4: `' c-char-sequence '`. C11 adds `u` /
                // `U` / `u8` encoding prefixes which we accept now for
                // forward compatibility. Two-char lookahead (three for
                // `u8'`) disambiguates against identifiers that merely
                // start with `L`, `u`, or `U`.
                '\'' => {
                    return Some(self.scan_char_constant(lo, StringEncoding::None));
                }
                'L' if self.cursor.second() == Some('\'') => {
                    self.cursor.bump(); // 'L'
                    return Some(self.scan_char_constant(lo, StringEncoding::Wide));
                }
                'U' if self.cursor.second() == Some('\'') => {
                    self.cursor.bump(); // 'U'
                    return Some(self.scan_char_constant(lo, StringEncoding::Utf32));
                }
                'u' if self.cursor.second() == Some('8')
                    && self.cursor.peek_at(2) == Some('\'') =>
                {
                    self.cursor.bump(); // 'u'
                    self.cursor.bump(); // '8'
                    return Some(self.scan_char_constant(lo, StringEncoding::Utf8));
                }
                'u' if self.cursor.second() == Some('\'') => {
                    self.cursor.bump(); // 'u'
                    return Some(self.scan_char_constant(lo, StringEncoding::Utf16));
                }

                // ── String literal, optionally prefixed ───────────────
                // C99 §6.4.5: `" s-char-sequence_opt "`. Same encoding
                // prefixes as character constants (plus C11 `u8`),
                // same escape alphabet, same prefix-vs-identifier
                // disambiguation with two-/three-char lookahead.
                // Adjacent-literal concatenation is a phase-05 (parser)
                // concern — each pair of quotes yields its own token.
                '"' => {
                    return Some(self.scan_string_literal(lo, StringEncoding::None));
                }
                'L' if self.cursor.second() == Some('"') => {
                    self.cursor.bump(); // 'L'
                    return Some(self.scan_string_literal(lo, StringEncoding::Wide));
                }
                'U' if self.cursor.second() == Some('"') => {
                    self.cursor.bump(); // 'U'
                    return Some(self.scan_string_literal(lo, StringEncoding::Utf32));
                }
                'u' if self.cursor.second() == Some('8') && self.cursor.peek_at(2) == Some('"') => {
                    self.cursor.bump(); // 'u'
                    self.cursor.bump(); // '8'
                    return Some(self.scan_string_literal(lo, StringEncoding::Utf8));
                }
                'u' if self.cursor.second() == Some('"') => {
                    self.cursor.bump(); // 'u'
                    return Some(self.scan_string_literal(lo, StringEncoding::Utf16));
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

                // ── pp-number, digit-initial form (C99 §6.4.8) ────────
                c if c.is_ascii_digit() => {
                    return Some(self.scan_pp_number(lo));
                }

                // ── pp-number, `.digit` form (C99 §6.4.8) ─────────────
                // Two-char lookahead disambiguates against a bare `.`
                // punctuator and against the `...` ellipsis — both of
                // which fall through to the catch-all arm and will be
                // picked up by the punctuator recogniser (task 03-lex/08).
                '.' if matches!(self.cursor.second(), Some(c) if c.is_ascii_digit()) => {
                    return Some(self.scan_pp_number(lo));
                }

                // ── Punctuator (C99 §6.4.6), max-munch ────────────────
                // All three-char punctuators are tried before any
                // two-char prefix, which are in turn tried before the
                // one-char form. See `scan_punctuator`.
                c if is_punct_start(c) => {
                    if let Some(p) = self.scan_punctuator() {
                        return Some(self.make_token(PpTokenKind::Punct(p), lo));
                    }
                    // `is_punct_start` is only true for bytes that can
                    // begin at least one punctuator, so `scan_punctuator`
                    // must succeed — but we keep a defensive fallback.
                    self.cursor.bump();
                    self.emit_stray_char(lo);
                    return Some(self.make_token(PpTokenKind::Unknown, lo));
                }

                // ── Stray character (E0001) ───────────────────────────
                // Any byte that can begin none of identifier, pp-number,
                // literal, comment, whitespace, or punctuator. The
                // lexer still produces a token (so parser recovery can
                // resynchronise) but also emits a diagnostic.
                _ => {
                    self.cursor.bump();
                    self.emit_stray_char(lo);
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
    /// Recognise a C99 preprocessing-number starting at the current
    /// cursor position, up to the closing byte offset implied by the
    /// pp-number grammar of §6.4.8:
    ///
    /// ```text
    /// pp-number := digit
    ///            | . digit
    ///            | pp-number digit
    ///            | pp-number identifier-nondigit
    ///            | pp-number (e|E|p|P) sign
    ///            | pp-number .
    /// ```
    ///
    /// The recogniser is pure maximal-munch: it never re-examines
    /// validity, so inputs like `0xzzz` or `1.2.3` lex to a single
    /// pp-number whose span covers the whole malformed run. Actual
    /// numeric decoding (and the corresponding diagnostics) live in
    /// the parser (phase 05).
    ///
    /// Classification is recorded in [`PpNumberKind`]:
    /// - `Float` if the token contains `.`, or (outside a `0x`/`0X`
    ///   prefix) `e`/`E`, or (inside such a prefix) `p`/`P`.
    /// - `Integer` otherwise.
    ///
    /// Sign-absorption after `e`/`E`/`p`/`P` applies unconditionally
    /// per the grammar: a trailing `+`/`-` is swallowed into the
    /// pp-number even when the resulting token is Integer-shaped
    /// (e.g. `0x1e+2`).
    fn scan_pp_number(&mut self, lo: BytePos) -> PpToken {
        // Caller guarantees the first char is either an ASCII digit or
        // `.` followed by an ASCII digit.
        let first = self.cursor.bump().expect("pp-number called at EOF");
        let mut has_dot = first == '.';
        let mut has_dec_exp = false;
        let mut has_hex_exp = false;

        // Detect the `0x` / `0X` hex prefix so we can tell apart the
        // decimal exponent (`e`/`E`) from the hex binary exponent
        // (`p`/`P`). Only a leading `0` digit can introduce one — a
        // leading `.` cannot.
        let is_hex = first == '0' && matches!(self.cursor.first(), Some('x' | 'X'));
        if is_hex {
            self.cursor.bump();
        }

        loop {
            match self.cursor.first() {
                Some('.') => {
                    has_dot = true;
                    self.cursor.bump();
                }
                Some('e' | 'E') => {
                    self.cursor.bump();
                    if !is_hex {
                        has_dec_exp = true;
                    }
                    // pp-number grammar: `e sign` / `E sign`. The sign
                    // is absorbed whenever present; if missing, the
                    // `e`/`E` simply acts as identifier-nondigit
                    // continuation (e.g. hex digit in `0xdeadbeef`).
                    if matches!(self.cursor.first(), Some('+' | '-')) {
                        self.cursor.bump();
                    }
                }
                Some('p' | 'P') => {
                    self.cursor.bump();
                    if is_hex {
                        has_hex_exp = true;
                    }
                    if matches!(self.cursor.first(), Some('+' | '-')) {
                        self.cursor.bump();
                    }
                }
                Some(c) if is_ident_continue_ascii(c) => {
                    self.cursor.bump();
                }
                // A UCN is a valid identifier-nondigit continuation.
                // The pp-number grammar is permissive — we do NOT run
                // UCN validation here (malformed / disallowed UCNs are
                // re-checked when the token is decoded in phase 05).
                Some('\\') if matches!(self.cursor.second(), Some('u' | 'U')) => {
                    let _ = self.scan_ucn();
                }
                _ => break,
            }
        }

        let kind = if has_dot || (is_hex && has_hex_exp) || (!is_hex && has_dec_exp) {
            PpNumberKind::Float
        } else {
            PpNumberKind::Integer
        };
        self.make_token(PpTokenKind::PpNumber(kind), lo)
    }

    /// Scan a character constant starting at the opening `'` (the
    /// encoding prefix, if any, has already been consumed by the
    /// caller). The cursor must be positioned on the `'`.
    ///
    /// Produces one [`PpTokenKind::CharConst`] token spanning from
    /// `lo` (inclusive of any prefix) through either the closing `'`
    /// or the first physical newline / EOF encountered — whichever
    /// comes first. In the latter case E0006 is emitted. Unknown
    /// escape letters emit E0007 per escape but do not truncate the
    /// token. Byte-value decoding (octal overflow, hex truncation,
    /// UCN code-point validity) is deferred to phase 05.
    ///
    /// C99 §6.4.4.4 grammar (informative):
    /// ```text
    /// c-char := any source char except ', \ or newline
    ///         | escape-sequence
    /// ```
    fn scan_char_constant(&mut self, lo: BytePos, enc: StringEncoding) -> PpToken {
        let opened = self.cursor.bump();
        debug_assert_eq!(opened, Some('\''), "caller must position cursor on `'`");

        loop {
            match self.cursor.first() {
                // Closing quote — done.
                Some('\'') => {
                    self.cursor.bump();
                    return self.make_token(PpTokenKind::CharConst { enc }, lo);
                }

                // Physical newline — unterminated. The newline itself
                // stays in the stream so directive boundaries survive.
                Some('\n') | Some('\r') => {
                    self.emit_unterminated_char_const(lo);
                    return self.make_token(PpTokenKind::CharConst { enc }, lo);
                }

                // EOF — unterminated.
                None => {
                    self.emit_unterminated_char_const(lo);
                    return self.make_token(PpTokenKind::CharConst { enc }, lo);
                }

                // Backslash: either a UCN (reuse the existing scanner
                // so splice-transparent offsets are maintained) or a
                // simple / octal / hex escape.
                Some('\\') => {
                    if matches!(self.cursor.second(), Some('u') | Some('U')) {
                        // Deliberately do NOT call validate_ident_ucn:
                        // UCN value checking inside a literal is a
                        // phase-05 concern (§6.4.4.4p9 cross-refs
                        // §6.4.3 which is itself only required at
                        // translation phase 5).
                        let _ = self.scan_ucn();
                    } else {
                        self.scan_char_escape();
                    }
                }

                // Any other c-char: just advance. Multi-byte code
                // points pass through as a single `char`.
                Some(_) => {
                    self.cursor.bump();
                }
            }
        }
    }

    fn emit_unterminated_char_const(&mut self, lo: BytePos) {
        let span = self.span_from(lo);
        if let Some(h) = self.handler.as_deref_mut() {
            h.struct_err(span, "unterminated character constant")
                .code(E0006)
                .primary(span, "this `'` is never closed before end of line or file")
                .help(
                    "insert a matching `'`; to embed a newline character \
                     inside the constant use the escape `\\n`",
                )
                .note("C99 §6.4.4.4 forbids a literal newline inside a character constant")
                .emit();
        }
    }

    /// Scan a string literal starting at the opening `"`. The encoding
    /// prefix (if any) has already been consumed by the caller; the
    /// cursor must be positioned on the `"`.
    ///
    /// Produces one [`PpTokenKind::StringLit`] token spanning from
    /// `lo` (inclusive of any prefix) through either the closing `"`
    /// or the first physical newline / EOF encountered — whichever
    /// comes first. In the latter case E0008 is emitted. Unknown
    /// escape letters emit E0007 per escape but do not truncate the
    /// token. Byte-value decoding (octal overflow, hex truncation,
    /// UCN code-point validity) is deferred to phase 05, as is
    /// adjacent-literal concatenation (C99 §5.1.1.2 phase 6).
    ///
    /// C99 §6.4.5 grammar (informative):
    /// ```text
    /// s-char := any source char except ", \ or newline
    ///         | escape-sequence
    /// ```
    fn scan_string_literal(&mut self, lo: BytePos, enc: StringEncoding) -> PpToken {
        let opened = self.cursor.bump();
        debug_assert_eq!(opened, Some('"'), "caller must position cursor on `\"`");

        loop {
            match self.cursor.first() {
                // Closing quote — done.
                Some('"') => {
                    self.cursor.bump();
                    return self.make_token(PpTokenKind::StringLit { enc }, lo);
                }

                // Physical newline — unterminated. The newline itself
                // stays in the stream so directive boundaries survive.
                Some('\n') | Some('\r') => {
                    self.emit_unterminated_string_literal(lo);
                    return self.make_token(PpTokenKind::StringLit { enc }, lo);
                }

                // EOF — unterminated.
                None => {
                    self.emit_unterminated_string_literal(lo);
                    return self.make_token(PpTokenKind::StringLit { enc }, lo);
                }

                // Backslash: either a UCN (reuse the existing scanner
                // so splice-transparent offsets are maintained) or a
                // simple / octal / hex escape. Same alphabet and same
                // deferral rules as a character constant — see
                // `scan_char_constant`.
                Some('\\') => {
                    if matches!(self.cursor.second(), Some('u') | Some('U')) {
                        let _ = self.scan_ucn();
                    } else {
                        self.scan_char_escape();
                    }
                }

                // Any other s-char: advance. Multi-byte UTF-8 code
                // points pass through as a single `char`; byte values
                // are preserved verbatim in the span.
                Some(_) => {
                    self.cursor.bump();
                }
            }
        }
    }

    fn emit_unterminated_string_literal(&mut self, lo: BytePos) {
        let span = self.span_from(lo);
        if let Some(h) = self.handler.as_deref_mut() {
            h.struct_err(span, "unterminated string literal")
                .code(E0008)
                .primary(span, "this `\"` is never closed before end of line or file")
                .help(
                    "insert a matching `\"`; to embed a newline inside the \
                     string use the escape `\\n`, or continue the literal \
                     across a physical line break with backslash-newline",
                )
                .note("C99 §6.4.5 forbids a literal newline inside a string literal")
                .emit();
        }
    }

    /// Scan a non-UCN escape sequence inside a character or string
    /// literal. The cursor must be positioned on the leading `\`.
    /// On return the cursor is one past the last consumed byte of the
    /// escape. Emits E0007 for any unknown escape letter.
    ///
    /// Recognised shapes (C99 §6.4.4.4):
    /// - simple-escape: one of `\' \" \? \\ \a \b \f \n \r \t \v`,
    /// - octal-escape: `\` followed by 1..=3 octal digits,
    /// - hex-escape: `\x` followed by one or more hex digits.
    ///
    /// Truncation / overflow of the numeric value itself is deferred
    /// to phase 05; the lexer's only job here is delimiting the run.
    fn scan_char_escape(&mut self) {
        let esc_lo = self.pos();
        let bs = self.cursor.bump();
        debug_assert_eq!(bs, Some('\\'), "caller must position cursor on `\\`");

        match self.cursor.first() {
            Some(c) if is_simple_escape(c) => {
                self.cursor.bump();
            }
            Some('0'..='7') => {
                // Octal: maximal-munch of up to three octal digits.
                self.cursor.bump();
                for _ in 0..2 {
                    if matches!(self.cursor.first(), Some('0'..='7')) {
                        self.cursor.bump();
                    } else {
                        break;
                    }
                }
            }
            Some('x') => {
                self.cursor.bump();
                while matches!(self.cursor.first(), Some(c) if c.is_ascii_hexdigit()) {
                    self.cursor.bump();
                }
                // A `\x` with no following hex digit is malformed, but
                // per C99 §6.4.4.4 the numeric-value diagnostic belongs
                // to phase 05. The lexer records the run and moves on.
            }
            // Unknown escape letter: emit E0007 pointing at `\X`.
            Some(bad) => {
                self.cursor.bump();
                let span = Span::new(self.file, esc_lo, self.pos());
                if let Some(h) = self.handler.as_deref_mut() {
                    h.struct_err(span, format!("invalid escape sequence `\\{bad}`"))
                        .code(E0007)
                        .primary(span, "this escape is not recognised by C99")
                        .help(
                            "valid escapes are `\\' \\\" \\? \\\\ \\a \\b \\f \\n \\r \\t \\v`, \
                             octal `\\NNN`, hex `\\xHH+`, or universal character names \
                             `\\uXXXX` / `\\UXXXXXXXX`",
                        )
                        .note("C99 §6.4.4.4 enumerates the legal escape sequences")
                        .emit();
                }
            }
            // EOF right after a `\\` — the outer scanner will observe
            // EOF on the next loop iteration and emit E0006.
            None => {}
        }
    }

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

    /// Recognise a single C99 §6.4.6 punctuator starting at the
    /// current cursor position, using maximal-munch. Returns `None`
    /// only if the leading byte does not begin any known punctuator —
    /// the caller decides how to report that.
    ///
    /// Ordering within each leading byte's arm matters: the longest
    /// form is always tried first. The 3-char forms `<<=`, `>>=` and
    /// `...` therefore precede the 2-char forms `<<`, `>>`, `..`; the
    /// 2-char compound-assignments / logical / relational / shift
    /// operators precede their 1-char shared prefixes. Trigraph /
    /// digraph punctuators (`%:`, `<:`, …) are deliberately *not*
    /// handled here per the plan.
    fn scan_punctuator(&mut self) -> Option<Punct> {
        let c0 = self.cursor.first()?;
        let c1 = self.cursor.second();
        let c2 = self.cursor.peek_at(2);

        let (p, len) = match (c0, c1, c2) {
            // ── 3-char punctuators ────────────────────────────────────
            ('<', Some('<'), Some('=')) => (P::ShlEq, 3),
            ('>', Some('>'), Some('=')) => (P::ShrEq, 3),
            ('.', Some('.'), Some('.')) => (P::Ellipsis, 3),

            // ── 2-char punctuators ────────────────────────────────────
            ('-', Some('>'), _) => (P::Arrow, 2),
            ('+', Some('+'), _) => (P::PlusPlus, 2),
            ('-', Some('-'), _) => (P::MinusMinus, 2),
            ('<', Some('<'), _) => (P::ShlShl, 2),
            ('>', Some('>'), _) => (P::ShrShr, 2),
            ('<', Some('='), _) => (P::Le, 2),
            ('>', Some('='), _) => (P::Ge, 2),
            ('=', Some('='), _) => (P::EqEq, 2),
            ('!', Some('='), _) => (P::BangEq, 2),
            ('&', Some('&'), _) => (P::AmpAmp, 2),
            ('|', Some('|'), _) => (P::PipePipe, 2),
            ('+', Some('='), _) => (P::PlusEq, 2),
            ('-', Some('='), _) => (P::MinusEq, 2),
            ('*', Some('='), _) => (P::StarEq, 2),
            ('/', Some('='), _) => (P::SlashEq, 2),
            ('%', Some('='), _) => (P::PercentEq, 2),
            ('&', Some('='), _) => (P::AmpEq, 2),
            ('|', Some('='), _) => (P::PipeEq, 2),
            ('^', Some('='), _) => (P::CaretEq, 2),
            ('#', Some('#'), _) => (P::HashHash, 2),

            // ── 1-char punctuators ────────────────────────────────────
            ('[', _, _) => (P::LBracket, 1),
            (']', _, _) => (P::RBracket, 1),
            ('(', _, _) => (P::LParen, 1),
            (')', _, _) => (P::RParen, 1),
            ('{', _, _) => (P::LBrace, 1),
            ('}', _, _) => (P::RBrace, 1),
            ('.', _, _) => (P::Dot, 1),
            ('&', _, _) => (P::Amp, 1),
            ('*', _, _) => (P::Star, 1),
            ('+', _, _) => (P::Plus, 1),
            ('-', _, _) => (P::Minus, 1),
            ('~', _, _) => (P::Tilde, 1),
            ('!', _, _) => (P::Bang, 1),
            ('/', _, _) => (P::Slash, 1),
            ('%', _, _) => (P::Percent, 1),
            ('<', _, _) => (P::Lt, 1),
            ('>', _, _) => (P::Gt, 1),
            ('^', _, _) => (P::Caret, 1),
            ('|', _, _) => (P::Pipe, 1),
            ('?', _, _) => (P::Question, 1),
            (':', _, _) => (P::Colon, 1),
            (';', _, _) => (P::Semi, 1),
            ('=', _, _) => (P::Eq, 1),
            (',', _, _) => (P::Comma, 1),
            ('#', _, _) => (P::Hash, 1),

            _ => return None,
        };

        for _ in 0..len {
            self.cursor.bump();
        }
        Some(p)
    }

    /// Emit E0001 for a single stray character at `lo..pos()`.
    ///
    /// The token itself is still produced (`PpTokenKind::Unknown`) so
    /// that downstream consumers have a span to point at; this
    /// diagnostic is advisory and does not abort lexing.
    fn emit_stray_char(&mut self, lo: BytePos) {
        let span = self.span_from(lo);
        if let Some(h) = self.handler.as_deref_mut() {
            h.struct_err(span, "stray character in program")
                .code(E0001)
                .primary(span, "this byte cannot begin any C99 token")
                .help(
                    "remove the character, or quote it inside a string or \
                     character literal if you meant it as data",
                )
                .note("C99 §6.4 lists every legal preprocessing-token start")
                .emit();
        }
    }

    /// One-shot header-name recogniser (C99 §6.4p4, §6.4.7).
    ///
    /// `header-name` pp-tokens exist *only* inside `#include` (and
    /// implementation-defined spots of `#pragma`). The ordinary
    /// [`Iterator::next`] loop must therefore never emit
    /// [`PpTokenKind::HeaderName`] spontaneously — it would be
    /// ambiguous with `<` / `>` / `"` in expression context (e.g.
    /// `a < b` must yield `Ident`/`Lt`/`Ident`, not `Ident`/`HeaderName`).
    ///
    /// The preprocessor calls this method exactly once after it has
    /// consumed the `include` identifier token. On return:
    ///
    /// - `Some(PpToken)` with kind [`PpTokenKind::HeaderName`] covering
    ///   either `<...>` or `"..."` — leading horizontal whitespace
    ///   between `include` and the opening delimiter is absorbed but
    ///   the token span starts at the `<` / `"`.
    /// - `None` if the next non-whitespace char is neither `<` nor `"`;
    ///   the cursor is left at that char so the caller can emit its
    ///   own directive-level diagnostic (E0013) and fall back to
    ///   ordinary tokenisation.
    ///
    /// ### Unterminated input
    ///
    /// If a physical newline or EOF interrupts the header-name before
    /// the matching `>` / `"`, E0010 is emitted and a recovery
    /// `HeaderName` token is still produced so downstream consumers
    /// have a span to point at. The terminating newline (if any) is
    /// NOT consumed, preserving the directive boundary for the
    /// preprocessor.
    ///
    /// ### Scope
    ///
    /// The lexer does not know about `#include` itself — that is a
    /// 04-03 concern. This method is the one place where the caller
    /// explicitly opts into header-name mode; everywhere else the
    /// punctuator rules apply.
    pub fn lex_header_name(&mut self) -> Option<PpToken> {
        // Absorb horizontal whitespace between `include` and the
        // opening delimiter, matching what `next()` would do for an
        // ordinary token.
        while matches!(self.cursor.first(), Some(c) if is_horizontal_ws(c)) {
            self.cursor.bump();
            self.leading_ws = true;
        }

        let lo = self.pos();
        let open = self.cursor.first()?;
        let close = match open {
            '<' => '>',
            '"' => '"',
            _ => return None,
        };
        self.cursor.bump(); // consume the opening delimiter

        loop {
            match self.cursor.first() {
                Some(c) if c == close => {
                    self.cursor.bump();
                    return Some(self.make_token(PpTokenKind::HeaderName, lo));
                }
                // A physical newline or EOF inside a header-name is
                // malformed. The newline itself is left in the stream
                // so the directive boundary survives.
                Some('\n') | Some('\r') | None => {
                    self.emit_unterminated_header_name(lo);
                    return Some(self.make_token(PpTokenKind::HeaderName, lo));
                }
                Some(_) => {
                    self.cursor.bump();
                }
            }
        }
    }

    fn emit_unterminated_header_name(&mut self, lo: BytePos) {
        let span = self.span_from(lo);
        if let Some(h) = self.handler.as_deref_mut() {
            h.struct_err(span, "unterminated header name")
                .code(E0010)
                .primary(span, "this `#include` header name is missing its closing delimiter")
                .help(
                    "close the header name with `>` for a system header (`<stdio.h>`) \
                     or `\"` for a local header (`\"myheader.h\"`)",
                )
                .note("C99 §6.4.7 requires header names to be closed on the same logical line")
                .emit();
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

/// C99 §6.4.4.4 simple-escape-sequence letters (excluding the UCN
/// forms `\u` / `\U`, which are recognised separately, and excluding
/// the octal / hex numeric escapes).
fn is_simple_escape(c: char) -> bool {
    matches!(c, '\'' | '"' | '?' | '\\' | 'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v')
}

/// Whether `c` can begin a C99 §6.4.6 punctuator. Used as the
/// dispatch predicate in the top-level token loop; the actual
/// maximal-munch choice is done by
/// [`Tokenizer::scan_punctuator`]. Keep in sync with the `match`
/// arms there.
fn is_punct_start(c: char) -> bool {
    matches!(
        c,
        '[' | ']'
            | '('
            | ')'
            | '{'
            | '}'
            | '.'
            | '&'
            | '*'
            | '+'
            | '-'
            | '~'
            | '!'
            | '/'
            | '%'
            | '<'
            | '>'
            | '^'
            | '|'
            | '?'
            | ':'
            | ';'
            | '='
            | ','
            | '#'
    )
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
