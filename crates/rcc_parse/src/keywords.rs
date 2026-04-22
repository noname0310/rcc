//! C99 keyword table (C99 §6.4.1).

/// All C99 keywords.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Keyword {
    /// `auto`
    Auto,
    /// `break`
    Break,
    /// `case`
    Case,
    /// `char`
    Char,
    /// `const`
    Const,
    /// `continue`
    Continue,
    /// `default`
    Default,
    /// `do`
    Do,
    /// `double`
    Double,
    /// `else`
    Else,
    /// `enum`
    Enum,
    /// `extern`
    Extern,
    /// `float`
    Float,
    /// `for`
    For,
    /// `goto`
    Goto,
    /// `if`
    If,
    /// `inline` (C99)
    Inline,
    /// `int`
    Int,
    /// `long`
    Long,
    /// `register`
    Register,
    /// `restrict` (C99)
    Restrict,
    /// `return`
    Return,
    /// `short`
    Short,
    /// `signed`
    Signed,
    /// `sizeof`
    Sizeof,
    /// `static`
    Static,
    /// `struct`
    Struct,
    /// `switch`
    Switch,
    /// `typedef`
    Typedef,
    /// `union`
    Union,
    /// `unsigned`
    Unsigned,
    /// `void`
    Void,
    /// `volatile`
    Volatile,
    /// `while`
    While,
    /// `_Bool` (C99)
    Bool,
    /// `_Complex` (C99)
    Complex,
    /// `_Imaginary` (C99)
    Imaginary,
}

/// String -> `Keyword` lookup table.
pub const KEYWORDS: &[(&str, Keyword)] = &[
    ("auto", Keyword::Auto),
    ("break", Keyword::Break),
    ("case", Keyword::Case),
    ("char", Keyword::Char),
    ("const", Keyword::Const),
    ("continue", Keyword::Continue),
    ("default", Keyword::Default),
    ("do", Keyword::Do),
    ("double", Keyword::Double),
    ("else", Keyword::Else),
    ("enum", Keyword::Enum),
    ("extern", Keyword::Extern),
    ("float", Keyword::Float),
    ("for", Keyword::For),
    ("goto", Keyword::Goto),
    ("if", Keyword::If),
    ("inline", Keyword::Inline),
    ("int", Keyword::Int),
    ("long", Keyword::Long),
    ("register", Keyword::Register),
    ("restrict", Keyword::Restrict),
    ("return", Keyword::Return),
    ("short", Keyword::Short),
    ("signed", Keyword::Signed),
    ("sizeof", Keyword::Sizeof),
    ("static", Keyword::Static),
    ("struct", Keyword::Struct),
    ("switch", Keyword::Switch),
    ("typedef", Keyword::Typedef),
    ("union", Keyword::Union),
    ("unsigned", Keyword::Unsigned),
    ("void", Keyword::Void),
    ("volatile", Keyword::Volatile),
    ("while", Keyword::While),
    ("_Bool", Keyword::Bool),
    ("_Complex", Keyword::Complex),
    ("_Imaginary", Keyword::Imaginary),
];
