//! Parsed preprocessing directive form. M5 materialises these; held here to
//! freeze the interface.
//!
//! Task 04-02 adds [`parse_directive`], which classifies a single logical
//! `#`-line (as produced by [`crate::line_stream::LineStream`]) into a
//! [`Directive`] variant. Body-level semantics (resolving `#include` paths,
//! expanding macros, evaluating `#if` expressions, ...) are deferred to the
//! directive-specific tasks 03, 06, 07, 13, ...

use rcc_errors::{codes::E0019, Diagnostic, Label, Level};
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{Interner, Span, Symbol};

use crate::macros::{MacroDef, MacroKind};

/// A single preprocessing directive after lexing `#...`.
#[derive(Clone, Debug)]
pub enum Directive {
    /// `#include "..."` / `#include <...>`
    Include {
        /// Full directive span.
        span: Span,
        /// Whether the form was `<...>` (system header).
        is_system: bool,
        /// Raw header name text (lexed as `HeaderName`).
        header: String,
    },
    /// `#define ...`
    Define(MacroDef),
    /// `#undef NAME`
    Undef {
        /// Full directive span.
        span: Span,
        /// Target macro name.
        name: Symbol,
    },
    /// `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else`, `#endif`
    Conditional {
        /// Full directive span.
        span: Span,
        /// Specific conditional kind.
        kind: ConditionalKind,
        /// Controlling expression tokens (for `#if`/`#elif`), raw.
        condition: Vec<PpToken>,
    },
    /// `#line N "file"?`
    Line {
        /// Full directive span.
        span: Span,
        /// New line number.
        line: u32,
        /// Optional new file name.
        file: Option<String>,
    },
    /// `#error "msg"`
    Error {
        /// Full directive span.
        span: Span,
        /// Message body.
        message: String,
    },
    /// `#pragma ...`
    Pragma {
        /// Full directive span.
        span: Span,
        /// Raw pragma tokens (implementation-defined).
        tokens: Vec<PpToken>,
    },
}

/// Specific `#if` / `#ifdef` / ... form.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConditionalKind {
    /// `#if`
    If,
    /// `#ifdef`
    IfDef,
    /// `#ifndef`
    IfNDef,
    /// `#elif`
    ElIf,
    /// `#else`
    Else,
    /// `#endif`
    EndIf,
}

/// Classify one logical `#`-line into a [`Directive`].
///
/// `line` must be the tokens of a single logical line as produced by
/// [`crate::line_stream::LineStream`], starting with a `Punct(Hash)`
/// token (the caller filters non-directive lines upstream).
///
/// ### Signature deviation from task 04-02
///
/// The task spec lists the signature as
/// `parse_directive(line: &[PpToken]) -> Result<Directive, Diagnostic>`,
/// but `PpToken` is by design text-free — it only carries a `Span`. To
/// resolve a token to its source text (needed to tell `include` from
/// `define`, to intern `#undef NAME`, and to materialise the raw
/// `header` / `message` strings that the [`Directive`] variants carry)
/// we have to look the span up in the file's source buffer, and to
/// create `Symbol`s we need the session-wide interner. Both are added
/// as explicit parameters rather than smuggled in via a global or a
/// re-lex step. This is documented in the task's `## Notes (agent)`.
///
/// ### Body-level validation
///
/// Per the task scope ("Out: evaluating the body — task-specific"),
/// this function does only the minimum body parsing required to fill
/// in each [`Directive`] variant's fields. In particular:
///
/// - `#include`: splits on the first token's kind (`<` vs `"..."`) and
///   stores the raw source substring covering the header tokens; full
///   header-name re-lexing is task 04-03.
/// - `#define`: stubs [`MacroDef`] with `kind = ObjectLike` and the
///   rest of the line stored verbatim in `body`. Function-like-vs-
///   object-like classification and parameter parsing are tasks 04-06
///   and 04-07.
/// - `#if`/`#ifdef`/`#ifndef`/`#elif`: stores the raw condition tokens;
///   constant-expression evaluation is task 04-13.
/// - `#else`/`#endif`: extra trailing tokens are not diagnosed here
///   (task 04-14 surfaces them when stack-matching).
/// - `#line`: parses the leading pp-number as `u32` and the optional
///   string literal as `file`; does not renumber the source map (task
///   04-15).
/// - `#error`: concatenates raw body text into `message`.
/// - `#pragma`: stores body tokens verbatim.
///
/// ### Errors
///
/// - Unknown directive name (identifier after `#` matches none of the
///   C99 §6.10 directives) → E0019 with a `help:` suggestion.
/// - Malformed `#include` / `#define` / `#undef` / `#line` bodies
///   that cannot populate the required variant fields → E0013 /
///   E0014 / E0015 respectively.
pub fn parse_directive(
    line: &[PpToken],
    src: &str,
    interner: &mut Interner,
) -> Result<Directive, Diagnostic> {
    let hash = line.first().expect("parse_directive called on empty line");
    debug_assert!(
        matches!(hash.kind, PpTokenKind::Punct(Punct::Hash)) && hash.at_line_start,
        "parse_directive expects a line starting with `#` at line start"
    );

    let line_span = line.last().map(|t| hash.span.to(t.span)).unwrap_or(hash.span);

    // C99 §6.10.7 null directive: `#` alone. Legal, parses as a
    // `Pragma` with empty body per plan §M5.
    if line.len() == 1 {
        return Ok(Directive::Pragma { span: line_span, tokens: Vec::new() });
    }

    let name_tok = &line[1];
    if name_tok.kind != PpTokenKind::Ident {
        return Err(unknown_directive(name_tok.span, src));
    }
    let name = token_text(name_tok, src);
    let body = &line[2..];

    match name {
        "include" => parse_include(line_span, body, src),
        "define" => parse_define(line_span, body, src, interner),
        "undef" => parse_undef(line_span, body, src, interner),
        "if" => Ok(make_conditional(line_span, ConditionalKind::If, body.to_vec())),
        "ifdef" => Ok(make_conditional(line_span, ConditionalKind::IfDef, body.to_vec())),
        "ifndef" => Ok(make_conditional(line_span, ConditionalKind::IfNDef, body.to_vec())),
        "elif" => Ok(make_conditional(line_span, ConditionalKind::ElIf, body.to_vec())),
        "else" => Ok(make_conditional(line_span, ConditionalKind::Else, Vec::new())),
        "endif" => Ok(make_conditional(line_span, ConditionalKind::EndIf, Vec::new())),
        "line" => parse_line_directive(line_span, body, src),
        "error" => Ok(parse_error_directive(line_span, body, src)),
        "pragma" => Ok(Directive::Pragma { span: line_span, tokens: body.to_vec() }),
        _ => Err(unknown_directive(name_tok.span, src)),
    }
}

fn token_text<'a>(tok: &PpToken, src: &'a str) -> &'a str {
    &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]
}

fn make_conditional(span: Span, kind: ConditionalKind, condition: Vec<PpToken>) -> Directive {
    Directive::Conditional { span, kind, condition }
}

fn parse_include(span: Span, body: &[PpToken], src: &str) -> Result<Directive, Diagnostic> {
    let Some(first) = body.first() else {
        return Err(malformed_include(span));
    };
    let last = body.last().expect("body.first() matched, so body.last() must also");

    let is_system = match first.kind {
        PpTokenKind::Punct(Punct::Lt) => true,
        PpTokenKind::StringLit { .. } => false,
        _ => return Err(malformed_include(first.span.to(last.span))),
    };

    // Raw substring from the first body token through the last: for
    // `<foo.h>` we capture `<foo.h>`, for `"foo.h"` we capture `"foo.h"`.
    // Task 04-03 re-lexes this into a proper `HeaderName` token and
    // strips delimiters.
    let lo = first.span.lo.0 as usize;
    let hi = last.span.hi.0 as usize;
    let header = src[lo..hi].to_string();

    Ok(Directive::Include { span, is_system, header })
}

fn parse_define(
    span: Span,
    body: &[PpToken],
    src: &str,
    interner: &mut Interner,
) -> Result<Directive, Diagnostic> {
    let Some(name_tok) = body.first() else {
        return Err(malformed_define(span));
    };
    if name_tok.kind != PpTokenKind::Ident {
        return Err(malformed_define(name_tok.span));
    }
    let name = interner.intern(token_text(name_tok, src));

    // Function-like vs object-like split and parameter parsing belong
    // to tasks 04-06 / 04-07. For now every `#define` is modelled as
    // object-like with the raw post-name tokens as its body; later
    // tasks reclassify in place.
    let body_tokens = body[1..].to_vec();
    Ok(Directive::Define(MacroDef {
        name,
        kind: MacroKind::ObjectLike,
        body: body_tokens,
        def_span: span,
    }))
}

fn parse_undef(
    span: Span,
    body: &[PpToken],
    src: &str,
    interner: &mut Interner,
) -> Result<Directive, Diagnostic> {
    let Some(name_tok) = body.first() else {
        return Err(malformed_undef(span));
    };
    if name_tok.kind != PpTokenKind::Ident {
        return Err(malformed_undef(name_tok.span));
    }
    let name = interner.intern(token_text(name_tok, src));
    Ok(Directive::Undef { span, name })
}

fn parse_line_directive(span: Span, body: &[PpToken], src: &str) -> Result<Directive, Diagnostic> {
    let Some(num_tok) = body.first() else {
        return Err(malformed_line(span));
    };
    if !matches!(num_tok.kind, PpTokenKind::PpNumber(_)) {
        return Err(malformed_line(num_tok.span));
    }
    let line_no: u32 =
        token_text(num_tok, src).parse().map_err(|_| malformed_line(num_tok.span))?;

    // Optional `"file"` — string literal, quotes stripped.
    let file = body.get(1).and_then(|t| match t.kind {
        PpTokenKind::StringLit { .. } => {
            let raw = token_text(t, src);
            // Strip leading encoding prefix (`u8`, `L`, `u`, `U`) and
            // the surrounding quotes; `#line`'s filename is a plain
            // s-char-sequence and doesn't use prefixes, but be
            // defensive.
            let after_prefix = raw
                .trim_start_matches("u8")
                .trim_start_matches('L')
                .trim_start_matches('u')
                .trim_start_matches('U');
            let inner = after_prefix.trim_start_matches('"').trim_end_matches('"');
            Some(inner.to_string())
        }
        _ => None,
    });

    Ok(Directive::Line { span, line: line_no, file })
}

fn parse_error_directive(span: Span, body: &[PpToken], src: &str) -> Directive {
    let message = match (body.first(), body.last()) {
        (Some(f), Some(l)) => {
            let lo = f.span.lo.0 as usize;
            let hi = l.span.hi.0 as usize;
            src[lo..hi].to_string()
        }
        _ => String::new(),
    };
    Directive::Error { span, message }
}

// ── Diagnostic constructors ──────────────────────────────────────────

fn unknown_directive(span: Span, src: &str) -> Diagnostic {
    // E0019's registry description already reads "unknown preprocessor
    // directive"; the task spec names the code E0020, but E0019 is the
    // pre-existing matching code and task 16 (#error) has dibs on
    // E0020. See `## Notes (agent)` in tasks/04-preprocess/02-...md.
    let name = src.get(span.lo.0 as usize..span.hi.0 as usize).unwrap_or("").to_string();
    let primary_msg = if name.is_empty() {
        "this token cannot introduce a preprocessing directive".to_string()
    } else {
        format!("`{name}` is not a recognised preprocessing directive")
    };
    Diagnostic {
        level: Level::Error,
        code: Some(E0019),
        message: "unknown preprocessing directive".into(),
        labels: vec![Label { span, message: primary_msg, primary: true }],
        notes: vec!["C99 §6.10 lists the ten directive names: `include`, `define`, \
             `undef`, `if`, `ifdef`, `ifndef`, `elif`, `else`, `endif`, \
             `line`, `error`, `pragma`"
            .into()],
        help: vec!["unknown preprocessing directive".into()],
    }
}

fn malformed_include(span: Span) -> Diagnostic {
    use rcc_errors::codes::E0013;
    Diagnostic {
        level: Level::Error,
        code: Some(E0013),
        message: "malformed #include directive".into(),
        labels: vec![Label {
            span,
            message: "expected `\"FILENAME\"` or `<FILENAME>` after `#include`".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.2 specifies the two header-name forms".into()],
        help: vec!["use `#include <header.h>` for a system header or \
             `#include \"header.h\"` for a user header"
            .into()],
    }
}

fn malformed_define(span: Span) -> Diagnostic {
    use rcc_errors::codes::E0014;
    Diagnostic {
        level: Level::Error,
        code: Some(E0014),
        message: "invalid #define directive".into(),
        labels: vec![Label {
            span,
            message: "expected an identifier for the macro name".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.3 requires #define NAME to be an identifier".into()],
        help: vec![],
    }
}

fn malformed_undef(span: Span) -> Diagnostic {
    use rcc_errors::codes::E0014;
    Diagnostic {
        level: Level::Error,
        code: Some(E0014),
        message: "invalid #undef directive".into(),
        labels: vec![Label {
            span,
            message: "expected an identifier for the macro name".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.5 requires #undef NAME to be an identifier".into()],
        help: vec![],
    }
}

fn malformed_line(span: Span) -> Diagnostic {
    use rcc_errors::codes::E0015;
    // E0015 originally read "expected identifier after #ifdef/#ifndef";
    // reusing the slot for "malformed #line" is a stretch but the PP
    // block E0001..E0020 is fully allocated and task 04-15 will
    // rework this code's description.
    Diagnostic {
        level: Level::Error,
        code: Some(E0015),
        message: "malformed #line directive".into(),
        labels: vec![Label {
            span,
            message: "expected a decimal line number after `#line`".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.4 requires `#line` to be followed by a \
             pp-number that is a decimal integer"
            .into()],
        help: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_stream::LineStream;
    use rcc_lexer::tokenize;
    use rcc_span::{FileId, Interner};

    /// Tokenise `src`, take its first logical line, and run
    /// `parse_directive` against it. Returns both the result and the
    /// interner so tests can resolve `Symbol`s.
    fn parse(src: &str) -> (Result<Directive, Diagnostic>, Interner) {
        let mut interner = Interner::new();
        let mut ls = LineStream::new(tokenize(FileId(0), src));
        let line = ls.next_line().expect("test source must contain one line");
        let result = parse_directive(&line, src, &mut interner);
        (result, interner)
    }

    // ── Positive: every Directive variant ────────────────────────────

    #[test]
    fn include_system_header() {
        let (d, _) = parse("#include <stdio.h>\n");
        match d.expect("include classified") {
            Directive::Include { is_system, header, .. } => {
                assert!(is_system, "`<...>` form must be system");
                assert_eq!(header, "<stdio.h>");
            }
            other => panic!("expected Include, got {other:?}"),
        }
    }

    #[test]
    fn include_local_header() {
        let (d, _) = parse("#include \"myheader.h\"\n");
        match d.expect("include classified") {
            Directive::Include { is_system, header, .. } => {
                assert!(!is_system, "`\"...\"` form must be local");
                assert_eq!(header, "\"myheader.h\"");
            }
            other => panic!("expected Include, got {other:?}"),
        }
    }

    #[test]
    fn define_object_like() {
        let (d, mut interner) = parse("#define PI 314\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("PI"), def.name);
                assert!(matches!(def.kind, MacroKind::ObjectLike));
                assert_eq!(def.body.len(), 1, "`314` is a single pp-number token");
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_kept_object_like_for_now() {
        // Function-like parsing is task 04-07; at this stage the
        // parens land in `body` and the whole thing is still
        // ObjectLike.
        let (d, mut interner) = parse("#define MAX(a,b) ((a)>(b)?(a):(b))\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("MAX"), def.name);
                assert!(matches!(def.kind, MacroKind::ObjectLike));
                assert!(!def.body.is_empty());
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn undef_directive() {
        let (d, mut interner) = parse("#undef FOO\n");
        match d.expect("undef classified") {
            Directive::Undef { name, .. } => {
                assert_eq!(interner.intern("FOO"), name);
            }
            other => panic!("expected Undef, got {other:?}"),
        }
    }

    #[test]
    fn if_directive() {
        let (d, _) = parse("#if 1 + 2\n");
        match d.expect("if classified") {
            Directive::Conditional { kind: ConditionalKind::If, condition, .. } => {
                assert_eq!(condition.len(), 3, "`1 + 2` is three tokens");
            }
            other => panic!("expected Conditional::If, got {other:?}"),
        }
    }

    #[test]
    fn ifdef_directive() {
        let (d, _) = parse("#ifdef DEBUG\n");
        match d.expect("ifdef classified") {
            Directive::Conditional { kind: ConditionalKind::IfDef, condition, .. } => {
                assert_eq!(condition.len(), 1);
            }
            other => panic!("expected Conditional::IfDef, got {other:?}"),
        }
    }

    #[test]
    fn ifndef_directive() {
        let (d, _) = parse("#ifndef HEADER_GUARD\n");
        match d.expect("ifndef classified") {
            Directive::Conditional { kind: ConditionalKind::IfNDef, condition, .. } => {
                assert_eq!(condition.len(), 1);
            }
            other => panic!("expected Conditional::IfNDef, got {other:?}"),
        }
    }

    #[test]
    fn elif_directive() {
        let (d, _) = parse("#elif X\n");
        match d.expect("elif classified") {
            Directive::Conditional { kind: ConditionalKind::ElIf, condition, .. } => {
                assert_eq!(condition.len(), 1);
            }
            other => panic!("expected Conditional::ElIf, got {other:?}"),
        }
    }

    #[test]
    fn else_directive() {
        let (d, _) = parse("#else\n");
        match d.expect("else classified") {
            Directive::Conditional { kind: ConditionalKind::Else, condition, .. } => {
                assert!(condition.is_empty());
            }
            other => panic!("expected Conditional::Else, got {other:?}"),
        }
    }

    #[test]
    fn endif_directive() {
        let (d, _) = parse("#endif\n");
        match d.expect("endif classified") {
            Directive::Conditional { kind: ConditionalKind::EndIf, condition, .. } => {
                assert!(condition.is_empty());
            }
            other => panic!("expected Conditional::EndIf, got {other:?}"),
        }
    }

    #[test]
    fn line_directive_number_only() {
        let (d, _) = parse("#line 42\n");
        match d.expect("line classified") {
            Directive::Line { line, file, .. } => {
                assert_eq!(line, 42);
                assert!(file.is_none());
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    #[test]
    fn line_directive_with_file() {
        let (d, _) = parse("#line 100 \"other.c\"\n");
        match d.expect("line classified") {
            Directive::Line { line, file, .. } => {
                assert_eq!(line, 100);
                assert_eq!(file.as_deref(), Some("other.c"));
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    #[test]
    fn error_directive() {
        let (d, _) = parse("#error unsupported platform\n");
        match d.expect("error classified") {
            Directive::Error { message, .. } => {
                assert!(
                    message.contains("unsupported") && message.contains("platform"),
                    "message should contain raw body text, got {message:?}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn pragma_directive() {
        let (d, _) = parse("#pragma once\n");
        match d.expect("pragma classified") {
            Directive::Pragma { tokens, .. } => {
                assert_eq!(tokens.len(), 1);
            }
            other => panic!("expected Pragma, got {other:?}"),
        }
    }

    // ── Acceptance: null directive is a legal empty Pragma ──────────

    #[test]
    fn null_directive_is_empty_pragma() {
        // C99 §6.10.7: `#` alone is the "null directive", a legal no-op.
        let (d, _) = parse("#\n");
        match d.expect("null directive classified") {
            Directive::Pragma { tokens, .. } => {
                assert!(tokens.is_empty(), "null directive must have no tokens");
            }
            other => panic!("null directive must parse as empty Pragma, got {other:?}"),
        }
    }

    // ── Acceptance: unknown directive emits E0019 with help ─────────

    #[test]
    fn unknown_directive_emits_e0019_with_help() {
        let (d, _) = parse("#foobar\n");
        let diag = d.expect_err("unknown directive must fail");
        assert_eq!(diag.code, Some(E0019), "unknown directive must carry E0019");
        assert!(
            diag.help.iter().any(|h| h.contains("unknown preprocessing directive")),
            "help text must mention `unknown preprocessing directive`: got {:?}",
            diag.help
        );
    }

    // ── Negatives: malformed bodies ─────────────────────────────────

    #[test]
    fn include_without_header_name_is_malformed() {
        let (d, _) = parse("#include foo\n");
        let diag = d.expect_err("bareword after #include must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0013));
    }

    #[test]
    fn define_without_identifier_is_malformed() {
        let (d, _) = parse("#define 123\n");
        let diag = d.expect_err("non-ident after #define must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0014));
    }

    #[test]
    fn undef_without_identifier_is_malformed() {
        let (d, _) = parse("#undef\n");
        let diag = d.expect_err("bare #undef must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0014));
    }

    #[test]
    fn line_without_number_is_malformed() {
        let (d, _) = parse("#line foo\n");
        let diag = d.expect_err("non-number after #line must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0015));
    }

    // ── Sanity: spans are stable and cover the whole directive ──────

    #[test]
    fn span_covers_whole_line() {
        let src = "#include <x>\n";
        let (d, _) = parse(src);
        let span = match d.unwrap() {
            Directive::Include { span, .. } => span,
            _ => panic!(),
        };
        // `#include <x>` is 12 bytes; newline is not part of the line
        // (LineStream strips it).
        assert_eq!(span.lo.0, 0);
        assert_eq!(span.hi.0, 12);
    }
}
