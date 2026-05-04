//! C99 keyword table (C99 §6.4.1).

use rcc_data_structures::FxHashMap;
use std::sync::OnceLock;

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
    ("__inline", Keyword::Inline),
    ("__inline__", Keyword::Inline),
    ("int", Keyword::Int),
    ("long", Keyword::Long),
    ("register", Keyword::Register),
    ("restrict", Keyword::Restrict),
    ("__restrict", Keyword::Restrict),
    ("__restrict__", Keyword::Restrict),
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

/// Classify an identifier spelling as a reserved C99 [`Keyword`], or
/// return `None` for an ordinary identifier.
///
/// Uses a process-wide `OnceLock`-backed [`FxHashMap`] built lazily from
/// [`KEYWORDS`] on the first call, giving amortised O(1) lookup. Keys are
/// `&'static str` borrowed from the table, so the cache never allocates
/// per-query.
///
/// C99 keywords are case-sensitive (§6.4.1); this function matches the
/// exact spelling and performs no case folding.
pub fn classify_ident(s: &str) -> Option<Keyword> {
    static MAP: OnceLock<FxHashMap<&'static str, Keyword>> = OnceLock::new();
    let map = MAP.get_or_init(|| KEYWORDS.iter().copied().collect());
    map.get(s).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_all_37_c99_keywords() {
        // C99 §6.4.1: 37 reserved words (32 from C89 + inline, restrict,
        // _Bool, _Complex, _Imaginary).
        let c99_count = KEYWORDS.iter().filter(|(spelling, _)| !spelling.starts_with("__")).count();
        assert_eq!(c99_count, 37);
    }

    #[test]
    fn every_keyword_round_trips() {
        for &(spelling, kw) in KEYWORDS {
            assert_eq!(classify_ident(spelling), Some(kw), "roundtrip for {spelling:?}");
        }
    }

    #[test]
    fn non_keyword_ident_returns_none() {
        assert_eq!(classify_ident("printf"), None);
        assert_eq!(classify_ident("main"), None);
        assert_eq!(classify_ident("x"), None);
        assert_eq!(classify_ident(""), None);
        assert_eq!(classify_ident("_my_var"), None);
    }

    #[test]
    fn keyword_classification_is_case_sensitive() {
        // C identifiers are case-sensitive; `Int` / `INT` are not keywords.
        assert_eq!(classify_ident("Int"), None);
        assert_eq!(classify_ident("INT"), None);
        assert_eq!(classify_ident("Return"), None);
        // Underscore-prefixed C99 keywords must keep exact casing.
        assert_eq!(classify_ident("_bool"), None);
        assert_eq!(classify_ident("_BOOL"), None);
    }

    #[test]
    fn sizeof_is_a_keyword_not_an_identifier() {
        // Common confusion: `sizeof` is an operator spelled as a keyword
        // (C99 §6.4.1, §6.5.3.4), not an ordinary identifier.
        assert_eq!(classify_ident("sizeof"), Some(Keyword::Sizeof));
    }

    #[test]
    fn c99_underscore_capital_keywords_are_classified() {
        assert_eq!(classify_ident("_Bool"), Some(Keyword::Bool));
        assert_eq!(classify_ident("_Complex"), Some(Keyword::Complex));
        assert_eq!(classify_ident("_Imaginary"), Some(Keyword::Imaginary));
    }

    #[test]
    fn c99_lowercase_keywords_are_classified() {
        assert_eq!(classify_ident("inline"), Some(Keyword::Inline));
        assert_eq!(classify_ident("restrict"), Some(Keyword::Restrict));
    }

    #[test]
    fn gnu_keyword_aliases_are_classified() {
        assert_eq!(classify_ident("__inline"), Some(Keyword::Inline));
        assert_eq!(classify_ident("__inline__"), Some(Keyword::Inline));
        assert_eq!(classify_ident("__restrict"), Some(Keyword::Restrict));
        assert_eq!(classify_ident("__restrict__"), Some(Keyword::Restrict));
    }

    #[test]
    fn reserved_ident_lookalikes_are_not_keywords() {
        // Implementation-reserved names (C99 §7.1.3) that are *not*
        // themselves C99 keywords must still classify as idents.
        assert_eq!(classify_ident("__func__"), None);
        assert_eq!(classify_ident("_Pragma"), None);
        assert_eq!(classify_ident("_Static_assert"), None);
    }
}
