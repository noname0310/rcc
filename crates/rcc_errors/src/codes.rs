//! Stable error-code registry for the rcc C compiler.
//!
//! Every user-facing diagnostic should carry one of these codes so users
//! can look it up in `docs/error-codes.md`.
//!
//! Codes are allocated in contiguous blocks per subsystem:
//!   E0001..E0020  — lexer / preprocessor
//!   E0021..E0040  — parser          (reserved, future)
//!   E0041..E0060  — type-checking   (reserved, future)
//!   E0061..E0080  — HIR lowering    (reserved, future)
//!   E0081..E0100  — codegen         (reserved, future)
//!
//! The preprocessor block E0001..E0020 was filled during lexer work, so
//! task 04-03 borrows the first slot of the parser window for the
//! `#include` resolver. Downstream parser tasks should allocate from
//! E0022 onward; see the `## Notes (agent)` in
//! `tasks/04-preprocess/03-include-search-path.md`.

/// Collects every registered error code for programmatic iteration.
///
/// Each entry is `(code, short_description)`.
pub const ALL_CODES: &[(&str, &str)] = &[
    (E0001, E0001_DESC),
    (E0002, E0002_DESC),
    (E0003, E0003_DESC),
    (E0004, E0004_DESC),
    (E0005, E0005_DESC),
    (E0006, E0006_DESC),
    (E0007, E0007_DESC),
    (E0008, E0008_DESC),
    (E0009, E0009_DESC),
    (E0010, E0010_DESC),
    (E0011, E0011_DESC),
    (E0012, E0012_DESC),
    (E0013, E0013_DESC),
    (E0014, E0014_DESC),
    (E0015, E0015_DESC),
    (E0016, E0016_DESC),
    (E0017, E0017_DESC),
    (E0018, E0018_DESC),
    (E0019, E0019_DESC),
    (E0020, E0020_DESC),
    (E0021, E0021_DESC),
    (E0022, E0022_DESC),
    (E0023, E0023_DESC),
    (E0024, E0024_DESC),
    (E0025, E0025_DESC),
    (E0026, E0026_DESC),
    (E0027, E0027_DESC),
];

// ── Lexer / preprocessor block: E0001..E0020 ────────────────────────

/// Unexpected character in source input.
pub const E0001: &str = "E0001";
const E0001_DESC: &str = "unexpected character";

/// Unterminated string literal.
pub const E0002: &str = "E0002";
const E0002_DESC: &str = "unterminated string literal";

/// Nested block comment (`/*` inside another `/* ... */`).
///
/// C99 block comments do not nest (§6.4.9). A nested `/*` is almost
/// always a typo — the outer comment is silently closed at the first
/// `*/`, leaking the remaining lines into regular source.
pub const E0003: &str = "E0003";
const E0003_DESC: &str = "nested block comment";

/// Unterminated block comment (`/* ... */`).
pub const E0004: &str = "E0004";
const E0004_DESC: &str = "unterminated block comment";

/// Invalid escape sequence in string or character literal.
pub const E0005: &str = "E0005";
const E0005_DESC: &str = "invalid escape sequence";

/// Unterminated character constant (`'...` with no closing `'`).
pub const E0006: &str = "E0006";
const E0006_DESC: &str = "unterminated character constant";

/// Invalid escape sequence in a string or character literal.
pub const E0007: &str = "E0007";
const E0007_DESC: &str = "invalid escape sequence";

/// Unterminated string literal (`"...` with no closing `"`).
pub const E0008: &str = "E0008";
const E0008_DESC: &str = "unterminated string literal";

/// Integer literal overflow.
pub const E0009: &str = "E0009";
const E0009_DESC: &str = "integer literal overflow";

/// Unterminated header name in `#include` directive.
///
/// A `<...>` or `"..."` header name was opened but the matching
/// closing delimiter was not found before the end of the logical
/// line or end of file (C99 §6.4.7).
pub const E0010: &str = "E0010";
const E0010_DESC: &str = "unterminated header name";

/// Invalid octal digit in integer literal.
pub const E0011: &str = "E0011";
const E0011_DESC: &str = "invalid octal digit";

/// Invalid hexadecimal escape in string/char literal.
pub const E0012: &str = "E0012";
const E0012_DESC: &str = "invalid hex escape";

/// `#include` expects `"FILENAME"` or `<FILENAME>`.
pub const E0013: &str = "E0013";
const E0013_DESC: &str = "malformed #include directive";

/// `#define` macro name is missing or invalid.
pub const E0014: &str = "E0014";
const E0014_DESC: &str = "invalid #define directive";

/// `#ifdef` / `#ifndef` expects an identifier.
pub const E0015: &str = "E0015";
const E0015_DESC: &str = "expected identifier after #ifdef/#ifndef";

/// Unmatched `#endif`.
pub const E0016: &str = "E0016";
const E0016_DESC: &str = "unmatched #endif";

/// Unmatched `#else` or `#elif`.
pub const E0017: &str = "E0017";
const E0017_DESC: &str = "unmatched #else/#elif";

/// Missing `#endif` at end of file.
pub const E0018: &str = "E0018";
const E0018_DESC: &str = "missing #endif at end of file";

/// Unknown preprocessor directive.
pub const E0019: &str = "E0019";
const E0019_DESC: &str = "unknown preprocessor directive";

/// `#error` directive encountered.
pub const E0020: &str = "E0020";
const E0020_DESC: &str = "#error directive encountered";

/// `#include` header could not be located in any search path.
///
/// For the `"..."` form the current source file's directory is
/// searched first, then `Session::opts.include_paths`; for the
/// `<...>` form only `include_paths` is consulted (C99 §6.10.2).
pub const E0021: &str = "E0021";
const E0021_DESC: &str = "cannot find header";

/// `#define` redefines a macro with a different replacement list.
///
/// C99 §6.10.3p1 permits "benign" redefinition — repeating an
/// identical `#define` is silently accepted — but any difference in
/// the replacement-list's token count, ordering, spelling, or
/// whitespace separation is ill-formed.
pub const E0022: &str = "E0022";
const E0022_DESC: &str = "macro redefined with a different body";

/// Duplicate parameter name in a function-like `#define`.
///
/// C99 §6.10.3p6: the identifiers naming the parameters of a
/// function-like macro "shall be distinct" — two identical names in
/// the same parameter list is a constraint violation.
pub const E0023: &str = "E0023";
const E0023_DESC: &str = "duplicate macro parameter name";

/// Stringize operator `#` not followed by a parameter name.
///
/// C99 §6.10.3.2p1: each `#` preprocessing token in the replacement
/// list for a function-like macro shall be followed by a parameter
/// name as the next preprocessing token in the replacement list.
pub const E0024: &str = "E0024";
const E0024_DESC: &str = "`#` is not followed by a macro parameter";

/// Token-paste operator `##` produced an invalid token.
///
/// C99 §6.10.3.3 — the concatenation of the two operand texts must
/// form a single valid preprocessing token. If the combined text
/// re-lexes to more than one pp-token the paste is ill-formed. This
/// code is also used for the C99 §6.10.3.3p1 positional constraint
/// violation (`##` at the very beginning or end of a replacement
/// list).
pub const E0025: &str = "E0025";
const E0025_DESC: &str = "pasting forms an invalid token";

/// `__VA_ARGS__` referenced outside a variadic function-like macro.
///
/// C99 §6.10.3p5: the identifier `__VA_ARGS__` shall occur only in
/// the replacement list of a function-like macro that uses the
/// ellipsis notation in the parameters. Any other use — inside an
/// object-like macro body, inside a non-variadic function-like
/// macro, or as an ordinary identifier in regular source — is a
/// constraint violation.
pub const E0026: &str = "E0026";
const E0026_DESC: &str = "`__VA_ARGS__` outside a variadic macro";

/// Attempt to `#define` or `#undef` a predefined macro.
///
/// C99 §6.10.8p2: the implementation shall not predefine the macro
/// `__cplusplus`, nor shall it define it in any standard header; and
/// the predefined macros listed in §6.10.8p1 — `__DATE__`,
/// `__FILE__`, `__LINE__`, `__STDC__`, `__STDC_HOSTED__`,
/// `__STDC_VERSION__`, `__TIME__` — "shall not be the subject of a
/// `#define` or `#undef` preprocessing directive". Doing so is a
/// constraint violation.
pub const E0027: &str = "E0027";
const E0027_DESC: &str = "cannot redefine or undefine a predefined macro";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_codes_have_correct_format() {
        for &(code, desc) in ALL_CODES {
            assert!(code.starts_with('E'), "code {code:?} must start with 'E'");
            assert_eq!(code.len(), 5, "code {code:?} must be exactly 5 chars");
            assert!(
                code[1..].chars().all(|c| c.is_ascii_digit()),
                "code {code:?} digits portion must be all digits"
            );
            assert!(!desc.is_empty(), "description for {code} must not be empty");
        }
    }

    #[test]
    fn no_duplicate_codes() {
        let mut seen = std::collections::HashSet::new();
        for &(code, _) in ALL_CODES {
            assert!(seen.insert(code), "duplicate error code: {code}");
        }
    }

    #[test]
    fn codes_are_sorted() {
        for window in ALL_CODES.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "codes must be sorted: {} should come before {}",
                window[0].0,
                window[1].0
            );
        }
    }
}
