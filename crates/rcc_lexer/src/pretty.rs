//! Pretty-printer for the preprocessing-token stream, used by the
//! driver's `--emit=tokens` debug/inspection mode and by
//! `crates/rcc_driver/tests/emit_tokens.rs`.
//!
//! Format (one line per token, LF-terminated):
//! ```text
//! <lo_line>:<lo_col>-<hi_line>:<hi_col>  <kind>  <text>
//! ```
//!
//! - Line/column are 1-based (`SourceMap::lookup_line_col` semantics).
//!   `hi_col` is the column of `span.hi` itself — i.e. the byte
//!   *after* the last byte of the token — matching the half-open
//!   `Span` convention elsewhere in `rcc`.
//! - `<kind>` is a stable dotted label — see [`kind_label`] — rather
//!   than `Debug`, so reformatting the enum never touches snapshots.
//! - `<text>` is the raw source slice for the token's span, with
//!   control characters escaped by [`escape_text`]. Every token
//!   therefore occupies exactly one line in the output.

use std::fmt::Write as _;

use rcc_span::{FileId, SourceMap};

use crate::{PpNumberKind, PpToken, PpTokenKind, Punct, StringEncoding, Tokenizer};

/// Tokenise `src` under `file`, then pretty-print the stream. Equivalent
/// to [`format_token_iter`] with a fresh whitespace-collapsing
/// [`Tokenizer`].
pub fn format_tokens(src: &str, source_map: &SourceMap, file: FileId) -> String {
    format_token_iter(src, source_map, file, Tokenizer::new(file, src))
}

/// Like [`format_tokens`] but with a caller-supplied token iterator.
/// Useful when the caller wants to enable `preserve_whitespace` or
/// otherwise configure the tokenizer.
pub fn format_token_iter<I>(src: &str, source_map: &SourceMap, file: FileId, tokens: I) -> String
where
    I: IntoIterator<Item = PpToken>,
{
    let mut out = String::new();
    for tok in tokens {
        let lo = source_map.lookup_line_col(file, tok.span.lo);
        let hi = source_map.lookup_line_col(file, tok.span.hi);
        let kind = kind_label(&tok.kind);
        let text = &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize];
        let escaped = escape_text(text);
        writeln!(out, "{}:{}-{}:{}  {}  {}", lo.line, lo.col, hi.line, hi.col, kind, escaped)
            .expect("writeln into String never fails");
    }
    out
}

/// Stable dotted label for a [`PpTokenKind`]. Each variant maps to a
/// human-readable name; compound variants (`PpNumber`, literals,
/// punctuators) are suffixed with their sub-kind so snapshots remain
/// unambiguous without dumping `Debug`.
pub fn kind_label(k: &PpTokenKind) -> String {
    match k {
        PpTokenKind::HeaderName => "HeaderName".into(),
        PpTokenKind::Ident => "Ident".into(),
        PpTokenKind::PpNumber(PpNumberKind::Integer) => "PpNumber.Integer".into(),
        PpTokenKind::PpNumber(PpNumberKind::Float) => "PpNumber.Float".into(),
        PpTokenKind::CharConst { enc } => format!("CharConst.{}", encoding_label(enc)),
        PpTokenKind::StringLit { enc } => format!("StringLit.{}", encoding_label(enc)),
        PpTokenKind::Punct(p) => format!("Punct.{}", punct_label(p)),
        PpTokenKind::Newline => "Newline".into(),
        PpTokenKind::Whitespace => "Whitespace".into(),
        PpTokenKind::Unknown => "Unknown".into(),
        PpTokenKind::Eof => "Eof".into(),
    }
}

fn encoding_label(e: &StringEncoding) -> &'static str {
    match e {
        StringEncoding::None => "None",
        StringEncoding::Wide => "Wide",
        StringEncoding::Utf16 => "Utf16",
        StringEncoding::Utf32 => "Utf32",
        StringEncoding::Utf8 => "Utf8",
    }
}

fn punct_label(p: &Punct) -> &'static str {
    match p {
        Punct::LBracket => "LBracket",
        Punct::RBracket => "RBracket",
        Punct::LParen => "LParen",
        Punct::RParen => "RParen",
        Punct::LBrace => "LBrace",
        Punct::RBrace => "RBrace",
        Punct::Dot => "Dot",
        Punct::Arrow => "Arrow",
        Punct::PlusPlus => "PlusPlus",
        Punct::MinusMinus => "MinusMinus",
        Punct::Amp => "Amp",
        Punct::Star => "Star",
        Punct::Plus => "Plus",
        Punct::Minus => "Minus",
        Punct::Tilde => "Tilde",
        Punct::Bang => "Bang",
        Punct::Slash => "Slash",
        Punct::Percent => "Percent",
        Punct::ShlShl => "ShlShl",
        Punct::ShrShr => "ShrShr",
        Punct::Lt => "Lt",
        Punct::Gt => "Gt",
        Punct::Le => "Le",
        Punct::Ge => "Ge",
        Punct::EqEq => "EqEq",
        Punct::BangEq => "BangEq",
        Punct::Caret => "Caret",
        Punct::Pipe => "Pipe",
        Punct::AmpAmp => "AmpAmp",
        Punct::PipePipe => "PipePipe",
        Punct::Question => "Question",
        Punct::Colon => "Colon",
        Punct::Semi => "Semi",
        Punct::Ellipsis => "Ellipsis",
        Punct::Eq => "Eq",
        Punct::StarEq => "StarEq",
        Punct::SlashEq => "SlashEq",
        Punct::PercentEq => "PercentEq",
        Punct::PlusEq => "PlusEq",
        Punct::MinusEq => "MinusEq",
        Punct::ShlEq => "ShlEq",
        Punct::ShrEq => "ShrEq",
        Punct::AmpEq => "AmpEq",
        Punct::CaretEq => "CaretEq",
        Punct::PipeEq => "PipeEq",
        Punct::Comma => "Comma",
        Punct::Hash => "Hash",
        Punct::HashHash => "HashHash",
    }
}

/// Escape control characters so every token fits on one line without
/// quoting tricks. Backslash is doubled for unambiguous round-tripping.
/// Non-control Unicode (incl. multi-byte UTF-8) is written verbatim.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                write!(out, "\\x{:02x}", c as u32).expect("String write never fails");
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use rcc_span::SourceMap;

    use super::*;

    fn render(src: &str) -> String {
        let mut sm = SourceMap::new();
        let id = sm.add_file(PathBuf::from("<test>"), Arc::from(src));
        format_tokens(src, &sm, id)
    }

    #[test]
    fn single_ident_is_one_line() {
        let out = render("foo\n");
        let mut it = out.lines();
        assert_eq!(it.next(), Some("1:1-1:4  Ident  foo"));
        assert_eq!(it.next(), Some("1:4-2:1  Newline  \\n"));
        assert!(it.next().is_none());
    }

    #[test]
    fn pp_number_labels() {
        let out = render("1 1.0 0x1p0");
        assert!(out.contains("PpNumber.Integer  1\n"));
        assert!(out.contains("PpNumber.Float  1.0\n"));
        assert!(out.contains("PpNumber.Float  0x1p0\n"));
    }

    #[test]
    fn punctuator_labels() {
        let out = render("<<= ...");
        assert!(out.contains("Punct.ShlEq  <<="));
        assert!(out.contains("Punct.Ellipsis  ..."));
    }

    #[test]
    fn string_encoding_label() {
        let out = render("L\"a\" u8\"b\"");
        assert!(out.contains("StringLit.Wide  L\"a\""));
        assert!(out.contains("StringLit.Utf8  u8\"b\""));
    }

    #[test]
    fn escapes_control_bytes_in_text() {
        // `\t` is horizontal whitespace and is collapsed in default
        // (non-whitespace-preserving) mode; only the newline survives,
        // and its escaped text must be the literal `\n` string.
        let out = render("\t\n");
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("1:2-2:1  Newline  \\n"));
        assert!(lines.next().is_none());
    }
}
