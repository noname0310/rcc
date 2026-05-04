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
use rcc_span::{Interner, SourceMap, Span, Symbol};

use crate::macros::{MacroDef, MacroKind};

/// Whether the token that introduces a `#define NAME<?>…` parameter
/// list is the function-like `(`. Per C99 §6.10.3p10, a `(` *without*
/// intervening whitespace after the macro name is the lparen of the
/// parameter list; a `(` separated from the name by whitespace is the
/// first token of an object-like replacement list.
fn is_function_like_lparen(tok: &PpToken) -> bool {
    matches!(tok.kind, PpTokenKind::Punct(Punct::LParen)) && !tok.leading_ws
}

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
    /// `#include MACRO` — the body does not start with `"` or `<`, so
    /// the tokens are stored raw for the caller to macro-expand and
    /// re-parse as a header name (C99 §6.10.2p4, GNU extension).
    IncludeTokens {
        /// Full directive span.
        span: Span,
        /// Raw body tokens to be macro-expanded.
        tokens: Vec<PpToken>,
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
    parse_directive_ext(line, src, interner, false)
}

/// Extended directive parser that accepts GNU extension flags.
pub fn parse_directive_ext(
    line: &[PpToken],
    src: &str,
    interner: &mut Interner,
    gnu_named_variadic: bool,
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
        "define" => parse_define(line_span, body, src, interner, gnu_named_variadic),
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
        // Not `<` or `"` — the body might be a macro that expands to a
        // header name (C99 §6.10.2p4). Return the raw tokens for the
        // caller to expand and re-parse.
        _ => return Ok(Directive::IncludeTokens { span, tokens: body.to_vec() }),
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
    gnu_named_variadic: bool,
) -> Result<Directive, Diagnostic> {
    let Some(name_tok) = body.first() else {
        return Err(malformed_define(span));
    };
    if name_tok.kind != PpTokenKind::Ident {
        return Err(malformed_define(name_tok.span));
    }
    let name = interner.intern(token_text(name_tok, src));

    let rest = &body[1..];
    let (kind, body_tokens) = match rest.first() {
        Some(tok) if is_function_like_lparen(tok) => {
            parse_function_like_signature(rest, src, interner, gnu_named_variadic)?
        }
        _ => (MacroKind::ObjectLike, rest.to_vec()),
    };

    Ok(Directive::Define(MacroDef {
        name,
        kind,
        body: body_tokens,
        def_span: span,
        is_predefined: false,
    }))
}

/// Parse the parameter list of a function-like `#define`, given the
/// tokens starting at the opening `(`. Returns the resulting
/// [`MacroKind::FunctionLike`] and the replacement-list tokens that
/// follow the closing `)`.
///
/// Grammar (C99 §6.10.3):
/// ```text
///     lparen identifier-list_opt rparen
///     lparen ... rparen
///     lparen identifier-list , ... rparen
/// ```
/// `identifier-list` is a comma-separated list of distinct identifiers.
fn parse_function_like_signature(
    rest: &[PpToken],
    src: &str,
    interner: &mut Interner,
    gnu_named_variadic: bool,
) -> Result<(MacroKind, Vec<PpToken>), Diagnostic> {
    debug_assert!(
        matches!(rest.first(), Some(t) if is_function_like_lparen(t)),
        "parse_function_like_signature expects `rest` to start with the fn-like `(`"
    );

    let lparen_span = rest[0].span;
    let mut idx = 1usize;
    let mut params: Vec<Symbol> = Vec::new();
    let mut variadic = false;
    let mut named_variadic: Option<Symbol> = None;

    // Allow empty `()` as a zero-parameter list. Otherwise the list is
    // a non-empty comma-separated sequence, each slot holding either
    // an `Ident` or (in the final slot) an `Ellipsis`.
    if let Some(tok) = rest.get(idx) {
        if matches!(tok.kind, PpTokenKind::Punct(Punct::RParen)) {
            idx += 1;
        } else {
            loop {
                let tok = rest.get(idx).ok_or_else(|| malformed_define_param_list(lparen_span))?;
                match tok.kind {
                    PpTokenKind::Ident => {
                        let text = token_text(tok, src);
                        let sym = interner.intern(text);
                        if params.contains(&sym) {
                            return Err(duplicate_param(tok.span, text));
                        }
                        params.push(sym);
                        idx += 1;
                        // GNU extension: `args...` — identifier immediately
                        // followed by `...` makes it a named variadic.
                        if gnu_named_variadic {
                            if let Some(next) = rest.get(idx) {
                                if matches!(next.kind, PpTokenKind::Punct(Punct::Ellipsis)) {
                                    variadic = true;
                                    named_variadic = Some(sym);
                                    // Pop from params — it's the variadic name, not a regular param.
                                    params.pop();
                                    idx += 1;
                                    let close = rest
                                        .get(idx)
                                        .ok_or_else(|| malformed_define_param_list(lparen_span))?;
                                    if !matches!(close.kind, PpTokenKind::Punct(Punct::RParen)) {
                                        return Err(malformed_define(close.span));
                                    }
                                    idx += 1;
                                    break;
                                }
                            }
                        }
                    }
                    PpTokenKind::Punct(Punct::Ellipsis) => {
                        variadic = true;
                        idx += 1;
                        // `...` must be the last slot; only `)` may follow.
                        let close = rest
                            .get(idx)
                            .ok_or_else(|| malformed_define_param_list(lparen_span))?;
                        if !matches!(close.kind, PpTokenKind::Punct(Punct::RParen)) {
                            return Err(malformed_define(close.span));
                        }
                        idx += 1;
                        break;
                    }
                    _ => return Err(malformed_define(tok.span)),
                }
                // Separator: `,` continues the list, `)` ends it.
                let sep = rest.get(idx).ok_or_else(|| malformed_define_param_list(lparen_span))?;
                match sep.kind {
                    PpTokenKind::Punct(Punct::RParen) => {
                        idx += 1;
                        break;
                    }
                    PpTokenKind::Punct(Punct::Comma) => {
                        idx += 1;
                    }
                    _ => return Err(malformed_define(sep.span)),
                }
            }
        }
    } else {
        return Err(malformed_define_param_list(lparen_span));
    }

    let body_tokens = rest[idx..].to_vec();
    Ok((MacroKind::FunctionLike { params, variadic, named_variadic }, body_tokens))
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
    let (line_no, file) =
        parse_line_operands(span, body, |tok| Some(token_text(tok, src).to_string()))?;
    Ok(Directive::Line { span, line: line_no, file })
}

/// Parse the operands of a `#line` directive after macro expansion.
///
/// C99 §6.10.4 requires the preprocessing tokens after `line` to be
/// macro-expanded before interpreting the resulting decimal line
/// number and optional string-literal file name. Expanded tokens may
/// originate from macro replacement lists in synthetic files, so this
/// helper resolves text through the session [`SourceMap`] instead of
/// through the directive's original source buffer.
pub fn parse_expanded_line_operands(
    span: Span,
    body: &[PpToken],
    source_map: &SourceMap,
) -> Result<(u32, Option<String>), Diagnostic> {
    parse_line_operands(span, body, |tok| {
        let file = source_map.file(tok.span.file);
        file.src.get(tok.span.lo.0 as usize..tok.span.hi.0 as usize).map(ToString::to_string)
    })
}

/// Parse the operands of a computed `#include` after macro expansion.
///
/// C99 §6.10.2p4 requires macro expansion first, then interpretation as a
/// header name. Expanded tokens may come from macro replacement lists in
/// synthetic files, so token spelling is resolved through the session
/// [`SourceMap`].
pub fn parse_expanded_include_operands(
    span: Span,
    body: &[PpToken],
    source_map: &SourceMap,
) -> Result<(bool, String), Diagnostic> {
    let Some(first) = body.first() else {
        return Err(malformed_include(span));
    };

    match first.kind {
        PpTokenKind::StringLit { .. } | PpTokenKind::HeaderName => {
            if body.len() != 1 {
                return Err(malformed_include(span));
            }
            let raw = expanded_token_text(first, source_map)
                .ok_or_else(|| malformed_include(first.span))?;
            let is_system = raw.starts_with('<');
            Ok((is_system, raw))
        }
        PpTokenKind::Punct(Punct::Lt) => {
            let Some(last) = body.last() else {
                return Err(malformed_include(span));
            };
            if !matches!(last.kind, PpTokenKind::Punct(Punct::Gt)) || body.len() < 2 {
                return Err(malformed_include(span));
            }
            let mut header = String::from("<");
            for tok in &body[1..body.len() - 1] {
                let text = expanded_token_text(tok, source_map)
                    .ok_or_else(|| malformed_include(tok.span))?;
                header.push_str(&text);
            }
            header.push('>');
            Ok((true, header))
        }
        _ => Err(malformed_include(first.span)),
    }
}

fn expanded_token_text(tok: &PpToken, source_map: &SourceMap) -> Option<String> {
    let file = source_map.file(tok.span.file);
    file.src.get(tok.span.lo.0 as usize..tok.span.hi.0 as usize).map(ToString::to_string)
}

fn parse_line_operands<F>(
    span: Span,
    body: &[PpToken],
    mut token_text: F,
) -> Result<(u32, Option<String>), Diagnostic>
where
    F: FnMut(&PpToken) -> Option<String>,
{
    let Some(num_tok) = body.first() else {
        return Err(malformed_line(span));
    };
    if !matches!(num_tok.kind, PpTokenKind::PpNumber(_)) {
        return Err(malformed_line(num_tok.span));
    }
    let line_no: u32 = token_text(num_tok)
        .ok_or_else(|| malformed_line(num_tok.span))?
        .parse()
        .map_err(|_| malformed_line(num_tok.span))?;

    // Optional `"file"` — string literal, quotes stripped.
    let file = body.get(1).and_then(|t| match t.kind {
        PpTokenKind::StringLit { .. } => {
            let raw = token_text(t)?;
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

    Ok((line_no, file))
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

/// Unterminated / malformed function-like parameter list
/// (e.g. EOL encountered before the closing `)`).
fn malformed_define_param_list(lparen_span: Span) -> Diagnostic {
    use rcc_errors::codes::E0014;
    Diagnostic {
        level: Level::Error,
        code: Some(E0014),
        message: "invalid #define directive".into(),
        labels: vec![Label {
            span: lparen_span,
            message: "unterminated macro parameter list".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.3 requires a `)` to close the parameter list \
             of a function-like macro"
            .into()],
        help: vec![],
    }
}

fn duplicate_param(span: Span, name: &str) -> Diagnostic {
    use rcc_errors::codes::E0023;
    Diagnostic {
        level: Level::Error,
        code: Some(E0023),
        message: format!("duplicate macro parameter name `{name}`"),
        labels: vec![Label { span, message: "duplicate parameter".into(), primary: true }],
        notes: vec!["C99 §6.10.3p6 requires each macro parameter name to \
             be distinct"
            .into()],
        help: vec!["rename one of the parameters".into()],
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
    fn define_function_like_two_params() {
        // C99 §6.10.3: `(` directly after NAME (no whitespace) opens a
        // function-like parameter list.
        let (d, mut interner) = parse("#define MAX(a,b) ((a)>(b)?(a):(b))\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("MAX"), def.name);
                match def.kind {
                    MacroKind::FunctionLike { params, variadic, .. } => {
                        assert_eq!(params.len(), 2, "MAX has two parameters");
                        assert_eq!(params[0], interner.intern("a"));
                        assert_eq!(params[1], interner.intern("b"));
                        assert!(!variadic);
                    }
                    other => panic!("expected FunctionLike, got {other:?}"),
                }
                assert!(!def.body.is_empty(), "body must contain the replacement list");
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_zero_params() {
        let (d, mut interner) = parse("#define F() 42\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("F"), def.name);
                match def.kind {
                    MacroKind::FunctionLike { params, variadic, .. } => {
                        assert!(params.is_empty(), "F has no parameters");
                        assert!(!variadic);
                    }
                    other => panic!("expected FunctionLike, got {other:?}"),
                }
                assert_eq!(def.body.len(), 1, "replacement list is just `42`");
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_variadic_only() {
        let (d, mut interner) = parse("#define V(...) __VA_ARGS__\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("V"), def.name);
                match def.kind {
                    MacroKind::FunctionLike { params, variadic, .. } => {
                        assert!(params.is_empty(), "V has no named parameters");
                        assert!(variadic, "V is variadic");
                    }
                    other => panic!("expected FunctionLike, got {other:?}"),
                }
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_named_and_variadic() {
        let (d, mut interner) = parse("#define LOG(fmt, ...) printf(fmt, __VA_ARGS__)\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("LOG"), def.name);
                match def.kind {
                    MacroKind::FunctionLike { params, variadic, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0], interner.intern("fmt"));
                        assert!(variadic);
                    }
                    other => panic!("expected FunctionLike, got {other:?}"),
                }
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_duplicate_param_is_e0023() {
        // C99 §6.10.3p6: parameter names must be distinct.
        let (d, _) = parse("#define FOO(a, a) a\n");
        let diag = d.expect_err("duplicate parameter must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0023));
        assert!(
            diag.message.contains("duplicate"),
            "diag message should mention duplication, got {:?}",
            diag.message
        );
        assert!(
            diag.labels.iter().any(|l| l.primary),
            "E0023 must carry a primary label on the duplicate"
        );
    }

    #[test]
    fn define_space_before_paren_is_object_like() {
        // C99 §6.10.3p10: a `(` separated from NAME by whitespace is
        // part of the replacement list, NOT the parameter-list opener.
        let (d, mut interner) = parse("#define F (x) x\n");
        match d.expect("define classified") {
            Directive::Define(def) => {
                assert_eq!(interner.intern("F"), def.name);
                assert!(
                    matches!(def.kind, MacroKind::ObjectLike),
                    "whitespace before `(` → object-like"
                );
                // Body: `(`, `x`, `)`, `x` — four pp-tokens.
                assert_eq!(def.body.len(), 4);
            }
            other => panic!("expected Define, got {other:?}"),
        }
    }

    #[test]
    fn define_function_like_unterminated_param_list_is_e0014() {
        let (d, _) = parse("#define FOO(a, b\n");
        let diag = d.expect_err("missing `)` must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0014));
    }

    #[test]
    fn define_function_like_non_ident_param_is_e0014() {
        let (d, _) = parse("#define FOO(1) x\n");
        let diag = d.expect_err("non-ident parameter must fail");
        assert_eq!(diag.code, Some(rcc_errors::codes::E0014));
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
    fn include_bareword_yields_include_tokens_for_computed_include() {
        let (d, _) = parse("#include foo\n");
        match d.expect("bareword after #include yields IncludeTokens") {
            Directive::IncludeTokens { tokens, .. } => {
                assert_eq!(tokens.len(), 1, "single identifier token expected");
            }
            other => panic!("expected IncludeTokens, got {other:?}"),
        }
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
