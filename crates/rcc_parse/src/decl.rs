//! Declaration specifiers (C99 §6.7).
//!
//! [`parse_decl_specs`] walks a *declaration-specifiers* run and
//! collects the four orthogonal families §6.7 §6.7.1 §6.7.2 §6.7.3
//! §6.7.4 allow to appear in any order:
//!
//! - *storage-class-specifier*   — `typedef`, `extern`, `static`,
//!   `auto`, `register`.
//! - *type-specifier*            — `void`, `char`, `short`, `int`,
//!   `long`, `float`, `double`, `signed`, `unsigned`, `_Bool`,
//!   `_Complex`, `_Imaginary`, a `struct`/`union`/`enum` specifier,
//!   or a `typedef-name`.
//! - *type-qualifier*            — `const`, `volatile`, `restrict`.
//! - *function-specifier*        — `inline`.
//!
//! The parser loops on the lookahead token and appends to the in-
//! progress [`DeclSpecs`] until it hits something that is not a
//! specifier keyword. The caller (declaration / parameter parser) is
//! responsible for deciding whether the resulting [`DeclSpecs`] is
//! well-formed *for its context* — e.g. a K&R-style function
//! definition is allowed to omit the type specifier entirely (C99
//! §6.9.1p5), but a regular declaration is not. This function only
//! reports violations that are local to the specifier list itself
//! (conflicting storage class, illegal multiset of type specifiers,
//! duplicated qualifier).
//!
//! ## Typedef-name recognition (partial)
//!
//! Inside a specifier list an ordinary `Ident` token may be a
//! *typedef-name* — the C99 declaration grammar would otherwise be
//! ambiguous (§6.7.2p2 footnote). We consult the parser's scope
//! stack, and only recognise the ident as a type specifier when **no
//! prior type specifier** has been seen in this run: once the list
//! already has, say, `unsigned`, the next ident must be the
//! declarator (`unsigned T x;` declares `x` of type `unsigned int`
//! named `T`? — no, `T` there is the declarator-name, not a type).
//! This matches the "longest-specifier" disambiguation every real-
//! world C compiler uses.
//!
//! Task 05-21 will formalise the full typedef-name-hack across all
//! call sites; this local lookup is enough for the simple
//! declaration shapes exercised by the task-18 acceptance fixtures.
//!
//! ## Struct / union / enum — stub
//!
//! Tasks 05-22 and 05-23 own struct/union and enum parsing. For now
//! [`parse_record_spec_stub`] and [`parse_enum_spec_stub`] recognise
//! the keyword, the optional tag, and a brace-balanced body they
//! skip without interpreting the contents. That is enough for the
//! `typedef struct S { ... } S` fixture in this task without pulling
//! in field-list / enumerator-list parsing.

use rcc_ast::{DeclSpecs, EnumSpec, RecordKind, RecordSpec, StorageClass, TypeSpec};
use rcc_errors::codes;
use rcc_lexer::Punct;
use rcc_span::{Span, Symbol};

use crate::keywords::Keyword;
use crate::token::TokenKind;
use crate::Parser;

/// Which "base" type keyword (if any) has fixed the type so far.
///
/// Only one of these may appear per specifier list. `Int` and `Char`
/// are still allowed to coexist with the sign / length modifiers
/// (`short int`, `unsigned char`, `long long int`); every other base
/// is exclusive.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum BaseKind {
    Void,
    Char,
    Int,
    Float,
    Double,
    Bool,
    Typedef,
    Record,
    Enum,
}

/// Running type-state used to reject illegal multisets of type
/// specifiers (C99 §6.7.2p2). The parser mutates this in lockstep
/// with [`DeclSpecs::type_specs`]; the two views agree on which
/// tokens have been accepted.
#[derive(Default, Debug)]
struct TypeState {
    base: Option<BaseKind>,
    short_count: u8,
    long_count: u8,
    signed_flag: bool,
    unsigned_flag: bool,
    complex_flag: bool,
    imaginary_flag: bool,
}

/// Parse a (possibly empty) `declaration-specifiers` production.
///
/// The loop terminates on the first lookahead token that is neither a
/// specifier keyword nor (in the no-base-yet case) a `typedef-name`.
/// Callers decide whether an empty result is acceptable:
///
/// - A regular declaration (`declaration : declaration-specifiers
///   init-declarator-list ;`, §6.7) must contain at least one type
///   specifier; the caller will reject an empty result. This rejection
///   lives in the declaration parser (task 05-19+), not here.
/// - A K&R-style old-style function definition (§6.9.1p5) may omit the
///   specifiers entirely — the implicit type is `int`.
///
/// Returns `None` only when the cursor is already past end of input
/// before any specifier had a chance to be consumed; in every other
/// case we return `Some(specs)`, possibly with an empty body.
pub fn parse_decl_specs(p: &mut Parser<'_>) -> Option<DeclSpecs> {
    let start_span = p.cur_span();
    let mut specs = DeclSpecs::default();
    let mut state = TypeState::default();
    let mut first_span: Option<Span> = None;
    let mut last_span: Span = start_span;

    while let Some(tok) = p.peek() {
        let tok_span = tok.span;

        match &tok.kind {
            TokenKind::Keyword(kw) => {
                if !consume_kw_specifier(p, *kw, tok_span, &mut specs, &mut state) {
                    break;
                }
            }
            TokenKind::Ident(sym) => {
                // Typedef-name recognition: only when no type
                // specifier has been accepted yet for this run.
                if state.base.is_none()
                    && state.short_count == 0
                    && state.long_count == 0
                    && !state.signed_flag
                    && !state.unsigned_flag
                    && !state.complex_flag
                    && !state.imaginary_flag
                    && p.scopes.is_typedef(*sym)
                {
                    let sym = *sym;
                    specs.type_specs.push(TypeSpec::TypedefName(sym));
                    state.base = Some(BaseKind::Typedef);
                    p.bump();
                } else {
                    break;
                }
            }
            _ => break,
        }

        if first_span.is_none() {
            first_span = Some(tok_span);
        }
        last_span = tok_span;
    }

    specs.span = match first_span {
        Some(lo) => lo.to(last_span),
        None => start_span,
    };
    Some(specs)
}

/// Try to consume the lookahead keyword as a declaration-specifier
/// token. Returns `true` when the keyword was a specifier (consumed,
/// possibly after emitting a diagnostic), `false` when it is not a
/// specifier keyword at all and the caller should break out of the
/// loop.
fn consume_kw_specifier(
    p: &mut Parser<'_>,
    kw: Keyword,
    span: Span,
    specs: &mut DeclSpecs,
    state: &mut TypeState,
) -> bool {
    match kw {
        // ── storage-class-specifier ──────────────────────────────
        Keyword::Typedef => accept_storage(p, specs, StorageClass::Typedef, "typedef", span),
        Keyword::Extern => accept_storage(p, specs, StorageClass::Extern, "extern", span),
        Keyword::Static => accept_storage(p, specs, StorageClass::Static, "static", span),
        Keyword::Auto => accept_storage(p, specs, StorageClass::Auto, "auto", span),
        Keyword::Register => accept_storage(p, specs, StorageClass::Register, "register", span),

        // ── type-qualifier ───────────────────────────────────────
        Keyword::Const => {
            accept_qual(p, &mut specs.quals.const_, "const", span);
        }
        Keyword::Volatile => {
            accept_qual(p, &mut specs.quals.volatile, "volatile", span);
        }
        Keyword::Restrict => {
            accept_qual(p, &mut specs.quals.restrict, "restrict", span);
        }

        // ── function-specifier ───────────────────────────────────
        Keyword::Inline => {
            if specs.func_specs.inline {
                p.session
                    .handler
                    .struct_warn(span, "duplicate `inline` function specifier")
                    .code(codes::W0004)
                    .emit();
            } else {
                specs.func_specs.inline = true;
            }
            p.bump();
        }

        // ── type-specifier — simple keywords ─────────────────────
        Keyword::Void => accept_base(p, specs, state, BaseKind::Void, TypeSpec::Void, "void", span),
        Keyword::Char => accept_base(p, specs, state, BaseKind::Char, TypeSpec::Char, "char", span),
        Keyword::Int => accept_base(p, specs, state, BaseKind::Int, TypeSpec::Int, "int", span),
        Keyword::Float => {
            accept_base(p, specs, state, BaseKind::Float, TypeSpec::Float, "float", span)
        }
        Keyword::Double => {
            accept_base(p, specs, state, BaseKind::Double, TypeSpec::Double, "double", span)
        }
        Keyword::Bool => {
            accept_base(p, specs, state, BaseKind::Bool, TypeSpec::Bool, "_Bool", span)
        }

        // ── length / sign modifiers ──────────────────────────────
        Keyword::Short => accept_short(p, specs, state, span),
        Keyword::Long => accept_long(p, specs, state, span),
        Keyword::Signed => accept_sign(p, specs, state, /*is_signed=*/ true, span),
        Keyword::Unsigned => accept_sign(p, specs, state, /*is_signed=*/ false, span),

        Keyword::Complex => accept_complex(p, specs, state, /*imaginary=*/ false, span),
        Keyword::Imaginary => accept_complex(p, specs, state, /*imaginary=*/ true, span),

        // ── tagged type specifiers ───────────────────────────────
        Keyword::Struct => accept_record(p, specs, state, RecordKind::Struct, span),
        Keyword::Union => accept_record(p, specs, state, RecordKind::Union, span),
        Keyword::Enum => accept_enum(p, specs, state, span),

        // Anything else is a non-specifier keyword (if, while, ...);
        // stop the loop and let the caller handle it.
        _ => return false,
    }
    true
}

// ─────────────────────────────────────────────────────────────────────
//  Storage class (C99 §6.7.1)
// ─────────────────────────────────────────────────────────────────────

fn accept_storage(
    p: &mut Parser<'_>,
    specs: &mut DeclSpecs,
    new: StorageClass,
    name: &str,
    span: Span,
) {
    match specs.storage {
        None => {
            specs.storage = Some(new);
        }
        Some(prev) if prev == new => {
            // e.g. `static static`. Same constraint (§6.7.1p2); the
            // code is E0060 regardless of whether it is a conflict
            // or a duplicate since "at most one" covers both.
            p.session
                .handler
                .struct_err(span, format!("duplicate `{name}` storage-class specifier"))
                .code(codes::E0060)
                .emit();
        }
        Some(_prev) => {
            p.session
                .handler
                .struct_err(
                    span,
                    format!("cannot combine `{name}` with previous storage-class specifier"),
                )
                .code(codes::E0060)
                .emit();
        }
    }
    p.bump();
}

// ─────────────────────────────────────────────────────────────────────
//  Type qualifiers (C99 §6.7.3)
// ─────────────────────────────────────────────────────────────────────

fn accept_qual(p: &mut Parser<'_>, slot: &mut bool, name: &str, span: Span) {
    if *slot {
        p.session
            .handler
            .struct_warn(span, format!("duplicate `{name}` type qualifier"))
            .code(codes::W0004)
            .emit();
    } else {
        *slot = true;
    }
    p.bump();
}

// ─────────────────────────────────────────────────────────────────────
//  Type specifiers (C99 §6.7.2)
// ─────────────────────────────────────────────────────────────────────

/// Accept an "exclusive-ish" base type keyword.
///
/// The compatibility matrix we enforce here matches §6.7.2p2:
///
/// | base       | may coexist with                         |
/// |------------|------------------------------------------|
/// | `void`     | nothing                                  |
/// | `_Bool`    | nothing                                  |
/// | `char`     | `signed` / `unsigned`                    |
/// | `int`      | `short` / `long` / `long long` / `signed` / `unsigned` |
/// | `float`    | `_Complex` / `_Imaginary`                |
/// | `double`   | one `long` / `_Complex` / `_Imaginary`   |
fn accept_base(
    p: &mut Parser<'_>,
    specs: &mut DeclSpecs,
    state: &mut TypeState,
    base: BaseKind,
    spec: TypeSpec,
    name: &str,
    span: Span,
) {
    if state.base.is_some() {
        specifier_conflict(
            p,
            format!("cannot combine `{name}` with previous type specifier"),
            span,
        );
        p.bump();
        return;
    }

    // Check modifier compatibility per the table above.
    let modifier_ok = match base {
        BaseKind::Void | BaseKind::Bool => {
            state.short_count == 0
                && state.long_count == 0
                && !state.signed_flag
                && !state.unsigned_flag
                && !state.complex_flag
                && !state.imaginary_flag
        }
        BaseKind::Char => {
            state.short_count == 0
                && state.long_count == 0
                && !state.complex_flag
                && !state.imaginary_flag
        }
        BaseKind::Int => !state.complex_flag && !state.imaginary_flag,
        BaseKind::Float => {
            state.short_count == 0
                && state.long_count == 0
                && !state.signed_flag
                && !state.unsigned_flag
        }
        BaseKind::Double => {
            state.short_count == 0
                && state.long_count <= 1
                && !state.signed_flag
                && !state.unsigned_flag
        }
        BaseKind::Typedef | BaseKind::Record | BaseKind::Enum => {
            // unreachable: those kinds take different entry points.
            true
        }
    };
    if !modifier_ok {
        specifier_conflict(
            p,
            format!("cannot combine `{name}` with previous type specifier"),
            span,
        );
        p.bump();
        return;
    }

    state.base = Some(base);
    specs.type_specs.push(spec);
    p.bump();
}

fn accept_short(p: &mut Parser<'_>, specs: &mut DeclSpecs, state: &mut TypeState, span: Span) {
    if state.short_count > 0 {
        specifier_conflict(p, "cannot combine `short` with previous `short`", span);
    } else if state.long_count > 0 {
        specifier_conflict(p, "cannot combine `short` with `long`", span);
    } else if let Some(b) = state.base {
        if !matches!(b, BaseKind::Int) {
            specifier_conflict(
                p,
                "cannot combine `short` with previous non-integer type specifier",
                span,
            );
            p.bump();
            return;
        }
        state.short_count += 1;
        specs.type_specs.push(TypeSpec::Short);
    } else {
        state.short_count = 1;
        specs.type_specs.push(TypeSpec::Short);
    }
    p.bump();
}

fn accept_long(p: &mut Parser<'_>, specs: &mut DeclSpecs, state: &mut TypeState, span: Span) {
    if state.short_count > 0 {
        specifier_conflict(p, "cannot combine `long` with `short`", span);
    } else if state.long_count >= 2 {
        specifier_conflict(p, "`long long long` is not a valid type specifier", span);
    } else if let Some(b) = state.base {
        match b {
            BaseKind::Int => {
                state.long_count += 1;
                specs.type_specs.push(TypeSpec::Long);
            }
            BaseKind::Double if state.long_count == 0 => {
                state.long_count = 1;
                specs.type_specs.push(TypeSpec::Long);
            }
            _ => {
                specifier_conflict(p, "cannot combine `long` with previous type specifier", span);
                p.bump();
                return;
            }
        }
    } else {
        state.long_count += 1;
        specs.type_specs.push(TypeSpec::Long);
    }
    p.bump();
}

fn accept_sign(
    p: &mut Parser<'_>,
    specs: &mut DeclSpecs,
    state: &mut TypeState,
    is_signed: bool,
    span: Span,
) {
    let (flag, name, other, spec) = if is_signed {
        (&mut state.signed_flag, "signed", state.unsigned_flag, TypeSpec::Signed)
    } else {
        (&mut state.unsigned_flag, "unsigned", state.signed_flag, TypeSpec::Unsigned)
    };

    if *flag {
        specifier_conflict(p, format!("duplicate `{name}`"), span);
        p.bump();
        return;
    }
    if other {
        specifier_conflict(
            p,
            format!("cannot combine `{name}` with opposite sign specifier"),
            span,
        );
        p.bump();
        return;
    }
    // Sign must pair with an integer base (or nothing yet).
    if let Some(b) = state.base {
        if !matches!(b, BaseKind::Char | BaseKind::Int) {
            specifier_conflict(
                p,
                format!("cannot combine `{name}` with previous non-integer type specifier"),
                span,
            );
            p.bump();
            return;
        }
    }
    *flag = true;
    specs.type_specs.push(spec);
    p.bump();
}

fn accept_complex(
    p: &mut Parser<'_>,
    specs: &mut DeclSpecs,
    state: &mut TypeState,
    imaginary: bool,
    span: Span,
) {
    let (flag, name, other, spec) = if imaginary {
        (&mut state.imaginary_flag, "_Imaginary", state.complex_flag, TypeSpec::Imaginary)
    } else {
        (&mut state.complex_flag, "_Complex", state.imaginary_flag, TypeSpec::Complex)
    };

    if *flag {
        specifier_conflict(p, format!("duplicate `{name}`"), span);
        p.bump();
        return;
    }
    if other {
        specifier_conflict(
            p,
            format!("cannot combine `{name}` with the other complex specifier"),
            span,
        );
        p.bump();
        return;
    }
    if state.short_count > 0 || state.signed_flag || state.unsigned_flag {
        specifier_conflict(p, format!("cannot combine `{name}` with integer modifiers"), span);
        p.bump();
        return;
    }
    if let Some(b) = state.base {
        if !matches!(b, BaseKind::Float | BaseKind::Double) {
            specifier_conflict(
                p,
                format!("cannot combine `{name}` with previous non-floating type specifier"),
                span,
            );
            p.bump();
            return;
        }
    }
    *flag = true;
    specs.type_specs.push(spec);
    p.bump();
}

fn accept_record(
    p: &mut Parser<'_>,
    specs: &mut DeclSpecs,
    state: &mut TypeState,
    kind: RecordKind,
    span: Span,
) {
    if state.base.is_some() || !is_type_state_clean(state) {
        let name = match kind {
            RecordKind::Struct => "struct",
            RecordKind::Union => "union",
        };
        specifier_conflict(
            p,
            format!("cannot combine `{name}` with previous type specifier"),
            span,
        );
        // Still parse the body so recovery is clean.
        let _ = parse_record_spec_stub(p, kind);
        return;
    }
    let rec = parse_record_spec_stub(p, kind);
    state.base = Some(BaseKind::Record);
    specs.type_specs.push(TypeSpec::Record(rec));
}

fn accept_enum(p: &mut Parser<'_>, specs: &mut DeclSpecs, state: &mut TypeState, span: Span) {
    if state.base.is_some() || !is_type_state_clean(state) {
        specifier_conflict(p, "cannot combine `enum` with previous type specifier", span);
        let _ = parse_enum_spec_stub(p);
        return;
    }
    let e = parse_enum_spec_stub(p);
    state.base = Some(BaseKind::Enum);
    specs.type_specs.push(TypeSpec::Enum(e));
}

fn is_type_state_clean(state: &TypeState) -> bool {
    state.short_count == 0
        && state.long_count == 0
        && !state.signed_flag
        && !state.unsigned_flag
        && !state.complex_flag
        && !state.imaginary_flag
}

fn specifier_conflict(p: &mut Parser<'_>, msg: impl Into<String>, span: Span) {
    p.session.handler.struct_err(span, msg).code(codes::E0061).emit();
}

// ─────────────────────────────────────────────────────────────────────
//  Stubs for tagged types (tasks 05-22, 05-23)
// ─────────────────────────────────────────────────────────────────────

/// Minimal struct/union recogniser used by task 05-18.
///
/// Consumes the `struct`/`union` keyword, an optional tag identifier,
/// and — if present — a brace-balanced body that is skipped without
/// interpretation. The result has `fields = None` either way; task
/// 05-22 will replace this with a real field-list parser and fill in
/// the body.
///
/// The caller has already validated the peek; this function assumes
/// the cursor is positioned on the `struct`/`union` keyword.
pub(crate) fn parse_record_spec_stub(p: &mut Parser<'_>, kind: RecordKind) -> RecordSpec {
    let kw_span = p.cur_span();
    p.bump(); // struct/union

    let tag = match p.peek() {
        Some(t) => match t.kind {
            TokenKind::Ident(sym) => {
                let s = t.span;
                p.bump();
                Some((sym, s))
            }
            _ => None,
        },
        None => None,
    };

    let mut end_span = tag.map(|(_, s)| s).unwrap_or(kw_span);
    let mut has_body = false;
    if let Some(t) = p.peek() {
        if matches!(t.kind, TokenKind::Punct(Punct::LBrace)) {
            has_body = true;
            end_span = skip_brace_body(p);
        }
    }

    if tag.is_none() && !has_body {
        let name = match kind {
            RecordKind::Struct => "struct",
            RecordKind::Union => "union",
        };
        p.session
            .handler
            .struct_err(kw_span, format!("`{name}` specifier needs a tag or a `{{` body"))
            .code(codes::E0061)
            .emit();
    }

    let id = p.fresh_id();
    RecordSpec { id, kind, tag: tag.map(|(sym, _)| sym), fields: None, span: kw_span.to(end_span) }
}

/// Minimal enum recogniser used by task 05-18. Same shape as
/// [`parse_record_spec_stub`].
pub(crate) fn parse_enum_spec_stub(p: &mut Parser<'_>) -> EnumSpec {
    let kw_span = p.cur_span();
    p.bump(); // enum

    let tag = match p.peek() {
        Some(t) => match t.kind {
            TokenKind::Ident(sym) => {
                let s = t.span;
                p.bump();
                Some((sym, s))
            }
            _ => None,
        },
        None => None,
    };

    let mut end_span = tag.map(|(_, s)| s).unwrap_or(kw_span);
    let mut has_body = false;
    if let Some(t) = p.peek() {
        if matches!(t.kind, TokenKind::Punct(Punct::LBrace)) {
            has_body = true;
            end_span = skip_brace_body(p);
        }
    }

    if tag.is_none() && !has_body {
        p.session
            .handler
            .struct_err(kw_span, "`enum` specifier needs a tag or a `{` body")
            .code(codes::E0061)
            .emit();
    }

    let id = p.fresh_id();
    EnumSpec { id, tag: tag.map(|(sym, _)| sym), enumerators: None, span: kw_span.to(end_span) }
}

/// Consume a brace-balanced `{ ... }` body at the cursor, returning the
/// span of the closing `}` (or the span of the opening `{` if EOI is
/// reached before a matching close — which is also diagnosed).
///
/// Used by the struct/union/enum stubs. Nested braces are counted so a
/// body containing a compound literal or an inner aggregate initialiser
/// does not short-circuit the match. At end-of-input we emit a
/// diagnostic pointing at the unclosed `{` and return the `{`'s span so
/// the caller can still synthesise a reasonable overall span.
fn skip_brace_body(p: &mut Parser<'_>) -> Span {
    // Caller has already confirmed the cursor is on `{`.
    let open = p.bump().expect("caller peeked `{`").span;
    let mut depth: u32 = 1;
    loop {
        let Some(tok) = p.peek() else {
            p.session
                .handler
                .struct_err(p.cur_span(), "unexpected end of input inside `{`-delimited body")
                .label(open, "unclosed `{` here")
                .code(codes::E0061)
                .emit();
            return open;
        };
        let span = tok.span;
        match tok.kind {
            TokenKind::Punct(Punct::LBrace) => {
                depth += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RBrace) => {
                depth -= 1;
                p.bump();
                if depth == 0 {
                    return span;
                }
            }
            _ => {
                p.bump();
            }
        }
    }
}

/// Dead-code guard: re-export for potential callers within the crate.
/// Keeps rustc from warning that `Symbol` is unused when the file is
/// trimmed down during bisection. This assignment is zero-cost and
/// optimised out.
#[allow(dead_code)]
const _SYMBOL_IS_LIVE: fn(Symbol) -> Symbol = |s| s;

// ─────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase7::convert;
    use crate::scope::NameKind;
    use rcc_lexer::{PpToken, Tokenizer};
    use rcc_session::Session;
    use rcc_span::FileId;
    use std::sync::Arc;

    fn mk_session(src: &str) -> (Session, FileId, rcc_errors::CaptureEmitter) {
        let (sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        (sess, fid, cap)
    }

    fn tokens_from_src(sess: &mut Session, fid: FileId, src: &str) -> Vec<crate::token::Token> {
        let pps: Vec<PpToken> = Tokenizer::new(fid, src).collect();
        convert(sess, &pps)
    }

    fn parse(src: &str) -> (DeclSpecs, Vec<rcc_errors::Diagnostic>) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        (specs, cap.diagnostics())
    }

    fn codes_of(diags: &[rcc_errors::Diagnostic]) -> Vec<&'static str> {
        diags.iter().filter_map(|d| d.code).collect()
    }

    // ── positive cases ──────────────────────────────────────────────

    #[test]
    fn plain_int_parses() {
        let (specs, diags) = parse("int");
        assert_eq!(specs.storage, None);
        assert!(matches!(specs.type_specs.as_slice(), [TypeSpec::Int]));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn const_volatile_int_parses() {
        let (specs, diags) = parse("const volatile int");
        assert!(specs.quals.const_);
        assert!(specs.quals.volatile);
        assert!(!specs.quals.restrict);
        assert!(matches!(specs.type_specs.as_slice(), [TypeSpec::Int]));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn static_const_unsigned_long_long_parses() {
        let (specs, diags) = parse("static const unsigned long long");
        assert_eq!(specs.storage, Some(StorageClass::Static));
        assert!(specs.quals.const_);
        // Order of collected specifiers matches source order.
        assert!(matches!(
            specs.type_specs.as_slice(),
            [TypeSpec::Unsigned, TypeSpec::Long, TypeSpec::Long]
        ));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn inline_extern_parses() {
        // Function specifier + storage class may coexist.
        let (specs, diags) = parse("inline extern");
        assert_eq!(specs.storage, Some(StorageClass::Extern));
        assert!(specs.func_specs.inline);
        assert!(specs.type_specs.is_empty(), "no type: {:?}", specs.type_specs);
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn typedef_struct_tagged_parses() {
        // `typedef struct S { ... } S`: typedef + struct recognised;
        // trailing `S` is the declarator and not consumed here.
        let src = "typedef struct S { int x; } S";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let total = tokens.len();
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        assert_eq!(specs.storage, Some(StorageClass::Typedef));
        match specs.type_specs.as_slice() {
            [TypeSpec::Record(rec)] => {
                assert_eq!(rec.kind, RecordKind::Struct);
                assert!(rec.tag.is_some(), "struct tag should be preserved");
                let name = parser.session.interner.get(rec.tag.unwrap());
                assert_eq!(name, "S");
                // Body was brace-matched; field parsing is task 05-22.
                assert!(rec.fields.is_none(), "stub returns fields=None");
            }
            other => panic!("expected single Record spec, got {other:?}"),
        }
        // The trailing `S` must remain unconsumed for the declarator.
        assert!(parser.cursor < total, "trailing declarator ident not consumed");
        let trailing = &parser.tokens[parser.cursor];
        assert!(
            matches!(trailing.kind, TokenKind::Ident(_)),
            "next token should be the declarator ident, got {:?}",
            trailing.kind
        );
        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn typedef_name_recognised_as_type_spec() {
        // If `T` is known to the scope as a typedef-name, it
        // contributes a TypedefName type specifier.
        let src = "const T";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let t_sym = sess.interner.intern("T");
        let mut parser = Parser::new(&mut sess, tokens);
        parser.scopes.declare(t_sym, NameKind::Typedef);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        assert!(specs.quals.const_);
        match specs.type_specs.as_slice() {
            [TypeSpec::TypedefName(sym)] => assert_eq!(parser.session.interner.get(*sym), "T"),
            other => panic!("expected TypedefName, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn ordinary_ident_does_not_consume() {
        // `int x`: `x` is not a typedef-name, so parsing stops at
        // `int` and leaves `x` for the declarator parser.
        let src = "int x";
        let (mut sess, fid, _cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let _ = parse_decl_specs(&mut parser).expect("parser returns specs");
        let rem = &parser.tokens[parser.cursor];
        assert!(matches!(rem.kind, TokenKind::Ident(_)));
    }

    #[test]
    fn trailing_ident_after_full_type_is_declarator_not_typedef() {
        // `unsigned T` — even if T is a typedef, we already have a
        // type modifier, so the trailing ident must fall through to
        // the declarator slot.
        let src = "unsigned T";
        let (mut sess, fid, _cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let t_sym = sess.interner.intern("T");
        let mut parser = Parser::new(&mut sess, tokens);
        parser.scopes.declare(t_sym, NameKind::Typedef);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        assert!(matches!(specs.type_specs.as_slice(), [TypeSpec::Unsigned]));
        // Cursor must stop at the ident so a later declarator parser
        // picks up `T` as the declared name.
        let rem = &parser.tokens[parser.cursor];
        assert!(matches!(rem.kind, TokenKind::Ident(_)));
    }

    #[test]
    fn long_double_parses() {
        // `long double` is a distinct C99 type — must be accepted.
        let (specs, diags) = parse("long double");
        assert!(matches!(specs.type_specs.as_slice(), [TypeSpec::Long, TypeSpec::Double]));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn empty_specs_is_not_none() {
        // Cursor sitting on a non-specifier token yields an empty
        // but valid DeclSpecs; the caller decides whether to reject.
        let src = ";";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("even empty runs return Some");
        assert!(specs.storage.is_none());
        assert!(specs.type_specs.is_empty());
        assert_eq!(parser.cursor, 0, "nothing consumed");
        assert!(cap.diagnostics().is_empty());
    }

    // ── warning cases (W0004) ───────────────────────────────────────

    #[test]
    fn const_const_warns_but_accepts() {
        let (specs, diags) = parse("const const int");
        assert!(specs.quals.const_);
        assert!(matches!(specs.type_specs.as_slice(), [TypeSpec::Int]));
        let codes = codes_of(&diags);
        assert_eq!(codes, vec!["W0004"], "redundant const should warn W0004: {diags:?}");
    }

    #[test]
    fn duplicate_inline_warns() {
        let (specs, diags) = parse("inline inline");
        assert!(specs.func_specs.inline);
        assert_eq!(codes_of(&diags), vec!["W0004"]);
    }

    // ── storage-class conflicts (E0060) ────────────────────────────

    #[test]
    fn static_extern_errors_e0060() {
        let (_specs, diags) = parse("static extern");
        assert_eq!(codes_of(&diags), vec!["E0060"], "{diags:?}");
    }

    #[test]
    fn duplicate_static_errors_e0060() {
        let (_specs, diags) = parse("static static");
        assert_eq!(codes_of(&diags), vec!["E0060"]);
    }

    // ── type-specifier conflicts (E0061) ───────────────────────────

    #[test]
    fn short_long_errors_e0061() {
        let (_specs, diags) = parse("short long");
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
    }

    #[test]
    fn long_long_long_errors_e0061() {
        let (_specs, diags) = parse("long long long");
        // First `long long` is fine; the third `long` triggers E0061.
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
    }

    #[test]
    fn int_int_errors_e0061() {
        let (_specs, diags) = parse("int int");
        assert_eq!(codes_of(&diags), vec!["E0061"]);
    }

    #[test]
    fn signed_unsigned_errors_e0061() {
        let (_specs, diags) = parse("signed unsigned");
        assert_eq!(codes_of(&diags), vec!["E0061"]);
    }

    #[test]
    fn float_int_errors_e0061() {
        let (_specs, diags) = parse("float int");
        assert_eq!(codes_of(&diags), vec!["E0061"]);
    }

    // ── stub shapes: enum / union ──────────────────────────────────

    #[test]
    fn enum_tagged_stub_parses() {
        let src = "enum E { A, B, C }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        match specs.type_specs.as_slice() {
            [TypeSpec::Enum(e)] => {
                assert!(e.tag.is_some());
                assert_eq!(parser.session.interner.get(e.tag.unwrap()), "E");
                assert!(e.enumerators.is_none(), "stub: enumerators not parsed yet");
            }
            other => panic!("expected Enum, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn union_tag_only_stub_parses() {
        let src = "union U";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        match specs.type_specs.as_slice() {
            [TypeSpec::Record(r)] => {
                assert_eq!(r.kind, RecordKind::Union);
                assert!(r.tag.is_some());
                assert!(r.fields.is_none());
            }
            other => panic!("expected Record(Union), got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn struct_without_tag_or_body_errors() {
        // `struct` with nothing else must be a constraint violation.
        let src = "struct ;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let _ = parse_decl_specs(&mut parser).expect("parser returns specs");
        assert_eq!(codes_of(&cap.diagnostics()), vec!["E0061"]);
    }
}
