//! Parser-level `Token` produced from a preprocessing-token stream after
//! phase-7 conversions (keyword classification, literal decoding, adjacent
//! string-literal concatenation).

use rcc_lexer::Punct;
use rcc_span::{Span, Symbol};

use crate::keywords::Keyword;

/// A post-phase-7 token.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// Token kind.
    pub kind: TokenKind,
    /// Span.
    pub span: Span,
}

/// Parser-level token kind.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// A reserved word.
    Keyword(Keyword),
    /// An identifier that is NOT a keyword (may be a typedef-name depending on scope).
    Ident(Symbol),
    /// Integer constant with an `IntLiteral` parsed value.
    IntLit(IntLiteral),
    /// Floating constant.
    FloatLit(FloatLiteral),
    /// Character constant.
    CharLit(CharLiteral),
    /// String literal (post-concatenation).
    StringLit(StringLiteral),
    /// A punctuator.
    Punct(Punct),
    /// End of input.
    Eof,
}

/// Parsed integer literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntLiteral {
    /// Numeric value as u128 (sign handled by parse).
    pub value: u128,
    /// Declared / deduced type category.
    pub suffix: IntSuffix,
}

/// Integer-literal suffix / deduced type.
// Variant spellings mirror the C source suffix set (`u`, `l`, `ul`, `ll`,
// `ull`) so every variant stays fully uppercase for a consistent mapping;
// that means `ULL` would trip `clippy::upper_case_acronyms` even though
// renaming only that variant to `Ull` (while leaving `UL`/`LL` intact)
// would be the inconsistent choice.
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntSuffix {
    /// No suffix.
    None,
    /// `u`/`U`.
    U,
    /// `l`/`L`.
    L,
    /// `ul`/`uL`/...
    UL,
    /// `ll`/`LL`.
    LL,
    /// `ull`/`uLL`.
    ULL,
}

/// Parsed float literal.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatLiteral {
    /// Raw value as f64 (long double handled separately).
    pub value: f64,
    /// Suffix-derived kind.
    pub suffix: FloatSuffix,
}

/// Float suffix.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FloatSuffix {
    /// `double` (no suffix).
    None,
    /// `f`/`F` -> `float`.
    F,
    /// `l`/`L` -> `long double`.
    L,
}

/// Parsed character literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharLiteral {
    /// Code-point value.
    pub value: u32,
    /// Source encoding.
    pub encoding: rcc_lexer::StringEncoding,
}

/// Parsed string literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringLiteral {
    /// Decoded bytes (includes trailing NUL).
    pub bytes: Vec<u8>,
    /// Encoding of the resulting string (merged from concatenated parts).
    pub encoding: rcc_lexer::StringEncoding,
}
