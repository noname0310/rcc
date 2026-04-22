//! Preprocessing token kinds.

/// A C preprocessing token category (C99 §6.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PpTokenKind {
    /// `header-name`: produced only inside `#include` directives.
    HeaderName,
    /// An identifier or keyword candidate (keyword classification happens in `rcc_parse`).
    Ident,
    /// `pp-number`: raw numeric literal not yet classified into int/float.
    PpNumber(PpNumberKind),
    /// Character constant.
    CharConst {
        /// Encoding prefix (`L`, `u`, `U`).
        enc: StringEncoding,
    },
    /// String literal.
    StringLit {
        /// Encoding prefix.
        enc: StringEncoding,
    },
    /// A punctuator from C99 §6.4.6.
    Punct(Punct),
    /// Physical newline; marks directive boundaries for `rcc_preprocess`.
    Newline,
    /// Whitespace run (spaces, tabs, comments).
    Whitespace,
    /// Catch-all for anything the lexer cannot classify.
    Unknown,
    /// End of file marker. Not usually emitted (iterator returns `None`).
    Eof,
}

/// Shape hint attached to a `PpNumber` token.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PpNumberKind {
    /// Looks like an integer (`123`, `0x1f`, `0755`).
    Integer,
    /// Looks like a float (`1.0`, `.5e10`, `0x1.0p0`).
    Float,
}

/// String / char literal encoding prefix.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StringEncoding {
    /// No prefix (default `char` / narrow).
    None,
    /// `L` prefix (`wchar_t`).
    Wide,
    /// `u` prefix (C11 `char16_t`).
    Utf16,
    /// `U` prefix (C11 `char32_t`).
    Utf32,
    /// `u8` prefix (C11 UTF-8).
    Utf8,
}

/// C99 punctuators, §6.4.6. Two-/three-character punctuators are single variants.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Punct {
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `.`
    Dot,
    /// `->`
    Arrow,
    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,
    /// `&`
    Amp,
    /// `*`
    Star,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `~`
    Tilde,
    /// `!`
    Bang,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `<<`
    ShlShl,
    /// `>>`
    ShrShr,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `^`
    Caret,
    /// `|`
    Pipe,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `?`
    Question,
    /// `:`
    Colon,
    /// `;`
    Semi,
    /// `...`
    Ellipsis,
    /// `=`
    Eq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `<<=`
    ShlEq,
    /// `>>=`
    ShrEq,
    /// `&=`
    AmpEq,
    /// `^=`
    CaretEq,
    /// `|=`
    PipeEq,
    /// `,`
    Comma,
    /// `#` (preprocessor only; classification lives here for convenience).
    Hash,
    /// `##` (preprocessor only).
    HashHash,
}
