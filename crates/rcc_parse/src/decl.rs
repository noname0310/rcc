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

use rcc_ast::{
    ArrayDeclarator, DeclSpecs, Declarator, DerivedDeclarator, EnumSpec, Expr, FunctionDeclarator,
    ParamDecl, RecordKind, RecordSpec, StorageClass, TypeName, TypeQuals, TypeSpec,
};
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
//  Declarator (C99 §6.7.5)
// ─────────────────────────────────────────────────────────────────────
//
// The declarator syntax encodes a nested chain of derivations applied
// to an identifier:
//
//     declarator        ::= pointer? direct-declarator
//     direct-declarator ::= IDENT
//                         | `(` declarator `)`
//                         | direct-declarator `[` array-stuff `]`
//                         | direct-declarator `(` param-list? `)`
//                         | direct-declarator `(` id-list? `)`   (K&R)
//     pointer           ::= `*` type-qualifier-list? pointer?
//
// C99 §6.7.5p4 defines the *type* of an identifier by reading its
// declarator inside-out: start at the identifier, then at each step
// wrap with whatever construct the next surrounding level names.
// Suffixes (`[N]`, `(args)`) of a direct-declarator apply before any
// leading pointer of the same declarator (suffixes bind tighter than
// prefix `*`), and a nested `( declarator )` contributes its entire
// chain as one inner atom.
//
// We store the resulting chain OUTER-TO-INNER — i.e. the first element
// is the operation that describes the identifier's own shape ("array
// of 3", "pointer to"), the last is the innermost wrap around the
// element type. For `int (*fp[3])(int, int)` the chain is therefore
//
//     [Array(3), Pointer, Function(int, int)]
//
// matching the task 05-19 acceptance spec. Array suffixes from a
// single direct-declarator are accumulated in source order because
// `a[10][20]` reads as "a is array of 10 of array of 20 of …" —
// `[10]` is the outer layer (applied first to the ident `a`), `[20]`
// is inner. Leading pointers of the same declarator, in contrast,
// reverse on the way in: `* *const p[3]` makes `p` an array-of-3 of
// const-pointer-to-pointer-to-T, so the `* *const` prefix folds into
// the chain *after* the direct-declarator's own suffixes and in
// rightmost-first order.

/// Which declarator flavour we are parsing. Drives atom-parsing
/// behaviour: concrete declarators require a name (§6.7.5), parameter
/// declarators permit one (§6.7.5.3), and abstract declarators forbid
/// one (§6.7.6). Everything else — pointer prefix, array / function
/// suffixes, type-qualifier lists — is shared across all three.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum DeclCtx {
    /// Regular declarator: the atom must be an identifier (or a
    /// nested concrete declarator). Missing atom → diagnostic +
    /// [`parse_declarator`] returns `None`.
    Concrete,
    /// Parameter declarator (§6.7.5.3p10): the atom is optional; a
    /// name is accepted but not required, and the parser decides
    /// `(` ambiguity via one-token lookahead.
    Param,
    /// Abstract declarator (§6.7.6): the atom is optional and MUST
    /// NOT be a name. If an ident appears, we emit E0062 and discard
    /// it as recovery so the rest of the surrounding construct
    /// (cast / sizeof / compound literal / param type) parses clean.
    Abstract,
}

/// Parse a C99 *declarator* (§6.7.5), including leading pointer
/// modifiers, the identifier (or nested `(declarator)`), and any
/// trailing array / function suffixes.
///
/// The returned [`Declarator::derived`] chain is outer-to-inner as
/// described above. Callers that need an *abstract* declarator —
/// type-name position in `sizeof(T)`, casts `(T)e`, compound literals
/// `(T){...}`, and parameter-type lists with an omitted name — use
/// [`parse_abstract_declarator`] instead. The three flavours share
/// the pointer-prefix / suffix-loop machinery via a private
/// [`DeclCtx`] flag; only the atom-parsing step differs.
pub fn parse_declarator(p: &mut Parser<'_>) -> Option<Declarator> {
    parse_declarator_in_ctx(p, DeclCtx::Concrete)
}

/// Parse a C99 *abstract-declarator* (§6.7.6) — same shape as a
/// concrete declarator but with the identifier atom forbidden. The
/// result always has `name = None`; if a name appears in source we
/// emit E0062 and drop it as recovery.
///
/// Abstract declarators never fail: the grammar allows an entirely
/// empty abstract declarator (e.g. in `sizeof(int)` the abstract
/// declarator is literally zero tokens), so this function always
/// returns a `Declarator` — the `derived` chain may be empty. That
/// is why the signature is `Declarator` and not `Option<Declarator>`.
pub fn parse_abstract_declarator(p: &mut Parser<'_>) -> Declarator {
    parse_declarator_in_ctx(p, DeclCtx::Abstract)
        .expect("abstract declarator parsing never returns None")
}

/// Parse a C99 *type-name* (§6.7.6): a specifier-qualifier-list
/// followed by an optional abstract declarator.
///
/// The grammar production is:
///
/// ```text
/// type-name:
///     specifier-qualifier-list abstract-declarator?
/// ```
///
/// Strictly speaking, `type-name` does not permit a *storage-class-
/// specifier* or a *function-specifier*, only specifiers and
/// qualifiers. We reuse [`parse_decl_specs`] for symmetry (it handles
/// typedef-name recognition and the type-specifier multiset checks
/// already) and rely on a later pass to enforce the narrower rule;
/// the parser only cares that specs + abstract-declarator form a
/// well-shaped span here.
pub fn parse_type_name(p: &mut Parser<'_>) -> TypeName {
    let start = p.cur_span();
    let specs = parse_decl_specs(p).unwrap_or_default();
    let declarator = parse_abstract_declarator(p);
    let end = last_consumed_span(p, start);
    TypeName { specs, declarator, span: start.to(end) }
}

/// Shared declarator engine. Pointer prefix + atom (ctx-dependent) +
/// suffix loop, with the chain folded the same way regardless of
/// flavour.
///
/// Abstract / parameter declarators may consume *zero* tokens (e.g.
/// the abstract declarator inside `sizeof(int)` is empty, and the
/// parser is already past end-of-input when the outer `parse_type_
/// name` calls us). In that case we return `start` as the span
/// unchanged — [`Span::to`] would otherwise debug-assert on a merge
/// between the `DUMMY_SP` returned by [`Parser::cur_span`] at EOF
/// and the last consumed token's span.
fn parse_declarator_in_ctx(p: &mut Parser<'_>, ctx: DeclCtx) -> Option<Declarator> {
    let start_cursor = p.cursor;
    let start = p.cur_span();
    let pointer_prefix = parse_pointer_prefix(p);
    let (name, mut chain) = parse_declarator_atom(p, ctx)?;
    parse_declarator_suffixes(p, &mut chain);
    // Pointer prefix modifiers wrap the *whole* direct-declarator from
    // the outside in the source but fold INSIDE of its suffixes in the
    // inside-out type, so they append after the direct-declarator's
    // chain. Within the prefix run itself, the rightmost `*` is the
    // closest to the direct-declarator and therefore applies first:
    // reverse the source order when stitching.
    chain.extend(pointer_prefix.into_iter().rev());
    let span =
        if p.cursor == start_cursor { start } else { start.to(last_consumed_span(p, start)) };
    Some(Declarator { name, derived: chain, span })
}

/// Consume zero or more `*` pointer tokens together with any
/// intervening *type-qualifier-list*s and return them in **source
/// order** — leftmost `*` first. The caller is responsible for
/// folding them into the final chain in the right direction (see
/// [`parse_declarator`]).
fn parse_pointer_prefix(p: &mut Parser<'_>) -> Vec<DerivedDeclarator> {
    let mut out = Vec::new();
    while matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Star))) {
        p.bump();
        let quals = parse_type_qualifier_list(p);
        out.push(DerivedDeclarator::Pointer(quals));
    }
    out
}

/// Consume a *type-qualifier-list* (C99 §6.7.3): a possibly-empty run
/// of `const` / `volatile` / `restrict`. Duplicate qualifiers are
/// accepted but warned (W0004), matching the specifier-list policy
/// in [`parse_decl_specs`].
fn parse_type_qualifier_list(p: &mut Parser<'_>) -> TypeQuals {
    let mut quals = TypeQuals::default();
    while let Some(tok) = p.peek() {
        let span = tok.span;
        let slot: &mut bool = match &tok.kind {
            TokenKind::Keyword(Keyword::Const) => &mut quals.const_,
            TokenKind::Keyword(Keyword::Volatile) => &mut quals.volatile,
            TokenKind::Keyword(Keyword::Restrict) => &mut quals.restrict,
            _ => break,
        };
        let name = match tok.kind {
            TokenKind::Keyword(Keyword::Const) => "const",
            TokenKind::Keyword(Keyword::Volatile) => "volatile",
            TokenKind::Keyword(Keyword::Restrict) => "restrict",
            _ => unreachable!(),
        };
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
    quals
}

/// Result of parsing a direct-declarator: the optional identifier
/// (name + its span) and the chain of suffix derivations collected on
/// the way out, in outer-to-inner order.
type DirectDecl = (Option<(Symbol, Span)>, Vec<DerivedDeclarator>);

/// Parse the atom of a direct-declarator: either an identifier, a
/// nested `( declarator )`, or nothing (for parameter / abstract
/// declarators that may start directly with a suffix or be empty).
///
/// Returns `(name, chain)` where `chain` is the *inner* chain
/// contributed by a nested declarator, if any; the caller runs the
/// suffix loop afterwards. Missing atom:
///
/// - `Concrete` — diagnosed, returns `None` so the outer parser can
///   bail.
/// - `Param` / `Abstract` — legal (the declarator may be empty or
///   start with a suffix); returns `Some((None, vec![]))`.
///
/// `Abstract` context additionally rejects an identifier atom with
/// E0062 and recovers by discarding the name.
///
/// ## `(` disambiguation in Param / Abstract contexts
///
/// A `(` at the atom slot could open either a nested declarator or
/// a function-suffix acting on an empty direct-abstract-declarator.
/// We peek one token past the `(`:
///
/// - `*`, `(`, `[`        — nested declarator (prefix / nested paren /
///   leading array, which direct-declarator can start with in a
///   nested position) — `Param`: a non-typedef ident also qualifies.
/// - anything else        — the `(` is a function suffix on an empty
///   direct-declarator; leave it untouched for the suffix loop.
///
/// Concrete declarators always recurse on `(` because a concrete
/// declarator must contain an identifier somewhere, so a top-level
/// `(` must be wrapping it (`(*fp)(int)` shape).
fn parse_declarator_atom(p: &mut Parser<'_>, ctx: DeclCtx) -> Option<DirectDecl> {
    match p.peek() {
        Some(tok) => match tok.kind {
            TokenKind::Ident(sym) => {
                let span = tok.span;
                p.bump();
                match ctx {
                    DeclCtx::Concrete | DeclCtx::Param => Some((Some((sym, span)), Vec::new())),
                    DeclCtx::Abstract => {
                        p.session
                            .handler
                            .struct_err(span, "abstract declarator cannot contain a name")
                            .code(codes::E0062)
                            .emit();
                        Some((None, Vec::new()))
                    }
                }
            }
            TokenKind::Punct(Punct::LParen) => {
                let is_nested = match ctx {
                    DeclCtx::Concrete => true,
                    _ => looks_like_nested_declarator(p, p.cursor + 1, ctx),
                };
                if is_nested {
                    p.bump();
                    let inner = parse_declarator_in_ctx(p, ctx);
                    match inner {
                        Some(d) => {
                            expect_rparen(p, "declarator");
                            Some((d.name, d.derived))
                        }
                        None => {
                            // Concrete-only failure path: consume `)`
                            // for recovery so the outer parser doesn't
                            // spin on it, then propagate the failure.
                            expect_rparen(p, "declarator");
                            None
                        }
                    }
                } else {
                    // `(` is a function suffix on an empty direct-
                    // declarator; leave it for the suffix loop.
                    Some((None, Vec::new()))
                }
            }
            _ => match ctx {
                DeclCtx::Concrete => {
                    let sp = tok.span;
                    p.session
                        .handler
                        .struct_err(sp, "expected identifier or `(` in declarator")
                        .emit();
                    None
                }
                _ => Some((None, Vec::new())),
            },
        },
        None => match ctx {
            DeclCtx::Concrete => {
                p.session
                    .handler
                    .struct_err(p.cur_span(), "expected declarator but found end of input")
                    .emit();
                None
            }
            _ => Some((None, Vec::new())),
        },
    }
}

/// One-token lookahead used by [`parse_declarator_atom`] to decide
/// whether a `(` at the atom position of a Param / Abstract declarator
/// opens a nested declarator or a function suffix on an empty direct-
/// declarator. See the docstring on the caller for the full table.
fn looks_like_nested_declarator(p: &Parser<'_>, at: usize, ctx: DeclCtx) -> bool {
    match p.tokens.get(at).map(|t| &t.kind) {
        Some(TokenKind::Punct(Punct::Star)) => true,
        Some(TokenKind::Punct(Punct::LParen)) => true,
        Some(TokenKind::Punct(Punct::LBracket)) => true,
        Some(TokenKind::Ident(sym)) => match ctx {
            // A non-typedef ident inside a parameter atom is almost
            // certainly the name of a concrete nested declarator;
            // a typedef-name is the start of a parameter type.
            DeclCtx::Param => !p.scopes.is_typedef(*sym),
            // Abstract declarators never contain names; any ident
            // here (typedef or not) belongs to a following parameter
            // list parsed as a function suffix.
            DeclCtx::Abstract => false,
            // Concrete context never reaches this helper.
            DeclCtx::Concrete => true,
        },
        _ => false,
    }
}

/// Run the trailing `[...]` / `(...)` suffix loop of a direct-
/// declarator, appending one [`DerivedDeclarator`] per suffix to
/// `chain`. Each suffix is outer to everything already in the chain
/// from a previous suffix (so `a[10][20]` ends up as
/// `[Array(10), Array(20)]`, the outer `[10]` wrapping the already-
/// present inner chain).
fn parse_declarator_suffixes(p: &mut Parser<'_>, chain: &mut Vec<DerivedDeclarator>) {
    while let Some(tok) = p.peek() {
        match tok.kind {
            TokenKind::Punct(Punct::LBracket) => {
                let arr = parse_array_suffix(p);
                chain.push(DerivedDeclarator::Array(arr));
            }
            TokenKind::Punct(Punct::LParen) => {
                let func = parse_function_suffix(p);
                chain.push(DerivedDeclarator::Function(func));
            }
            _ => break,
        }
    }
}

/// Parse a `[...]` array-declarator suffix — cursor must be on `[`.
/// Accepts all C99 forms:
///
/// - `[ ]`                         — incomplete / unknown size
/// - `[ N ]`                       — fixed or VLA runtime size
/// - `[ * ]`                       — VLA of unspecified size (prototype
///   scope only; §6.7.5.2p1)
/// - `[ static qual-list? N ]`     — `static` in array-parameter decl
/// - `[ qual-list static N ]`
/// - `[ qual-list N? ]`
///
/// `static` and the type-qualifier-list can appear in either order
/// (§6.7.5.2p1); the loop below accepts any interleaving because later
/// passes (05-20 abstract declarator, HIR lowering) re-validate shape.
fn parse_array_suffix(p: &mut Parser<'_>) -> ArrayDeclarator {
    p.bump(); // `[`

    let mut quals = TypeQuals::default();
    let mut has_static = false;
    while let Some(tok) = p.peek() {
        let span = tok.span;
        match &tok.kind {
            TokenKind::Keyword(Keyword::Static) => {
                if has_static {
                    p.session
                        .handler
                        .struct_warn(span, "duplicate `static` in array declarator")
                        .code(codes::W0004)
                        .emit();
                }
                has_static = true;
                p.bump();
            }
            TokenKind::Keyword(Keyword::Const)
            | TokenKind::Keyword(Keyword::Volatile)
            | TokenKind::Keyword(Keyword::Restrict) => {
                // Fold into the qualifier set; use the shared helper so
                // duplicate-warning behaviour stays in one place.
                let one = parse_type_qualifier_list(p);
                quals.const_ |= one.const_;
                quals.volatile |= one.volatile;
                quals.restrict |= one.restrict;
            }
            _ => break,
        }
    }

    let mut star = false;
    let mut size: Option<Expr> = None;
    match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RBracket)) => {
            // Empty `[]` — nothing to consume.
        }
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::Star)) => {
            // `[*]` is VLA-unspecified only when the `*` is the sole
            // thing inside the brackets. Otherwise it's the start of
            // an expression like `[*p]`.
            let after = p.tokens.get(p.cursor + 1);
            if matches!(after.map(|t| &t.kind), Some(TokenKind::Punct(Punct::RBracket))) {
                star = true;
                p.bump();
            } else {
                size = crate::expr::parse_assignment_expression(p);
            }
        }
        Some(_) => {
            size = crate::expr::parse_assignment_expression(p);
        }
        None => {}
    }

    match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RBracket)) => {
            p.bump();
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected `]` to close array declarator")
                .emit();
        }
    }

    ArrayDeclarator { quals, has_static, star, size }
}

/// Parse a `(...)` function-declarator suffix — cursor must be on
/// `(`. Distinguishes three shapes permitted by C99 §6.7.5.3p1:
///
/// - `()`                       — empty, unspecified parameters
/// - `(void)`                   — explicit zero-parameter prototype
/// - `(param , ... , param)`    — prototype, optionally variadic
///
/// The K&R identifier-list path is recognised but left empty: a
/// full implementation lands in task 05-26 which also needs to
/// disambiguate "unknown ident" vs "typedef-name" at this spot.
fn parse_function_suffix(p: &mut Parser<'_>) -> FunctionDeclarator {
    p.bump(); // `(`

    let mut params: Vec<ParamDecl> = Vec::new();
    let mut is_void = false;
    let mut variadic = false;
    let kr_names: Vec<(Symbol, Span)> = Vec::new();

    // `()` — empty parameter list.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::RParen))) {
        p.bump();
        return FunctionDeclarator { params, is_void, variadic, kr_names };
    }

    // `(void)` — single `void` immediately followed by `)`.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Keyword(Keyword::Void))) {
        let next = p.tokens.get(p.cursor + 1);
        if matches!(next.map(|t| &t.kind), Some(TokenKind::Punct(Punct::RParen))) {
            p.bump();
            p.bump();
            is_void = true;
            return FunctionDeclarator { params, is_void, variadic, kr_names };
        }
    }

    loop {
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Ellipsis))) {
            p.bump();
            variadic = true;
            break;
        }
        let start = p.cur_span();
        let specs = parse_decl_specs(p).unwrap_or_default();
        let declarator = parse_param_declarator(p);
        let end = last_consumed_span(p, start);
        params.push(ParamDecl { specs, declarator, span: start.to(end) });

        match p.peek().map(|t| &t.kind) {
            Some(TokenKind::Punct(Punct::Comma)) => {
                p.bump();
                continue;
            }
            _ => break,
        }
    }

    match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RParen)) => {
            p.bump();
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected `)` to close parameter list")
                .emit();
        }
    }

    FunctionDeclarator { params, is_void, variadic, kr_names }
}

/// Parse a parameter declarator — the shared engine is
/// [`parse_declarator_in_ctx`] with the [`DeclCtx::Param`] flavour.
/// A parameter declarator's atom is optional (prototype parameters
/// may omit the identifier entirely — `int f(int, char)` — and may
/// also contain a full nested concrete declarator for function-
/// pointer parameters like `int f(int (*pf)(int))`), so this call
/// site never returns `None`.
fn parse_param_declarator(p: &mut Parser<'_>) -> Declarator {
    parse_declarator_in_ctx(p, DeclCtx::Param)
        .expect("parameter declarator parsing never returns None")
}

/// Consume a `)` if present, or emit a "expected `)`" diagnostic
/// pointing at the current cursor position otherwise. Shared between
/// the nested-declarator and parameter-list call sites so the error
/// message stays in one place.
fn expect_rparen(p: &mut Parser<'_>, ctx: &str) {
    match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RParen)) => {
            p.bump();
        }
        _ => {
            p.session.handler.struct_err(p.cur_span(), format!("expected `)` in {ctx}")).emit();
        }
    }
}

/// Span of the token that was most recently consumed, if any; falls
/// back to `start` when the parser has not advanced. Used so every
/// declarator span covers the full run of tokens the parser actually
/// took without needing to thread an "end" return through every
/// helper.
fn last_consumed_span(p: &Parser<'_>, start: Span) -> Span {
    if p.cursor == 0 {
        return start;
    }
    p.tokens.get(p.cursor - 1).map(|t| t.span).unwrap_or(start)
}

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

    // ── Declarator (C99 §6.7.5) ─────────────────────────────────────

    use rcc_ast::ExprKind;

    /// Parse a declarator from a source slice that contains JUST the
    /// declarator (no leading declaration specifiers). Returns the
    /// declarator, the remaining unconsumed token count, and the
    /// captured diagnostics — the trio every declarator test wants.
    fn parse_decl(src: &str) -> (Declarator, usize, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let total = tokens.len();
        let mut parser = Parser::new(&mut sess, tokens);
        let d = parse_declarator(&mut parser).expect("declarator parses");
        let remaining = total.saturating_sub(parser.cursor);
        (d, remaining, cap.diagnostics(), sess)
    }

    fn assert_name(d: &Declarator, sess: &Session, expected: &str) {
        let (sym, _) = d.name.as_ref().expect("declarator has a name");
        assert_eq!(sess.interner.get(*sym), expected);
    }

    fn int_lit_text(e: &Expr, sess: &Session) -> String {
        match &e.kind {
            ExprKind::IntLit { text } => sess.interner.get(*text).to_string(),
            other => panic!("expected IntLit, got {other:?}"),
        }
    }

    #[test]
    fn plain_ident_is_empty_chain() {
        // `int x` — the declarator is just `x`. The derivation chain
        // is empty because there are no pointer / array / function
        // decorations in the source.
        let (d, rem, diags, sess) = parse_decl("x");
        assert_name(&d, &sess, "x");
        assert!(d.derived.is_empty(), "no derivations: {:?}", d.derived);
        assert_eq!(rem, 0, "whole input consumed");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn pointer_to_ident_parses() {
        let (d, _rem, diags, sess) = parse_decl("*p");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(q)] => {
                assert!(!q.const_ && !q.volatile && !q.restrict);
            }
            other => panic!("expected [Pointer], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn pointer_with_qualifier_parses() {
        // `*const p` — a `const`-qualified pointer. The qualifier sits
        // on the pointer itself, not on the pointee (§6.7.5.1p1).
        let (d, _rem, diags, sess) = parse_decl("*const p");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(q)] => {
                assert!(q.const_ && !q.volatile && !q.restrict);
            }
            other => panic!("expected [Pointer(const)], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn pointer_volatile_parses() {
        let (d, _rem, diags, sess) = parse_decl("*volatile p");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(q)] => {
                assert!(!q.const_ && q.volatile && !q.restrict);
            }
            other => panic!("expected [Pointer(volatile)], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn nested_pointers_reverse_in_chain() {
        // `* *const p`: reading inside-out from `p`, the nearest token
        // to `p` is the `*const` (rightmost pointer), so that layer
        // wraps `p` first — `p` becomes a const-pointer-to-X. The
        // outer, leftmost `*` then wraps that, giving X = pointer to
        // T. So p is "const pointer to pointer to T": the directly-
        // outer layer of p is the const pointer, and the innermost
        // layer (the pointed-to-pointer) is the plain `*`. In the
        // outer-to-inner chain representation the const-qualified
        // pointer comes FIRST.
        let (d, _rem, diags, sess) = parse_decl("* *const p");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(outer), DerivedDeclarator::Pointer(inner)] => {
                assert!(outer.const_, "outer pointer is `const` (wraps p directly)");
                assert!(!inner.const_, "inner pointer has no qualifier");
            }
            other => panic!("expected two Pointers, got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn two_dim_array_parses() {
        // `arr[10][20]` — two array layers, outer-to-inner.
        let (d, _rem, diags, sess) = parse_decl("arr[10][20]");
        assert_name(&d, &sess, "arr");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a10), DerivedDeclarator::Array(a20)] => {
                assert!(!a10.has_static && !a10.star);
                assert!(!a20.has_static && !a20.star);
                let s10 = a10.size.as_ref().expect("size present");
                let s20 = a20.size.as_ref().expect("size present");
                assert_eq!(int_lit_text(s10, &sess), "10");
                assert_eq!(int_lit_text(s20, &sess), "20");
            }
            other => panic!("expected two Array derivations, got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn array_incomplete_size_parses() {
        // `arr[]` — size omitted (§6.7.5.2p1, legal on a declaration
        // that will be completed by an initializer or at a later
        // definition).
        let (d, _rem, diags, _sess) = parse_decl("arr[]");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a)] => {
                assert!(a.size.is_none());
                assert!(!a.has_static);
                assert!(!a.star);
            }
            other => panic!("expected single Array, got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn array_star_vla_parses() {
        // `arr[*]` — VLA of unspecified size, legal only in a function
        // prototype scope (§6.7.5.2p1). The declarator parser accepts
        // the form; the context check is a later task.
        let (d, _rem, diags, _sess) = parse_decl("arr[*]");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a)] => {
                assert!(a.star);
                assert!(a.size.is_none());
                assert!(!a.has_static);
            }
            other => panic!("expected single Array(*), got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn array_static_qualified_parses() {
        // `arr[static const 10]` — both `static` and a qualifier list
        // inside the brackets (§6.7.5.2p1, array-parameter form).
        let (d, _rem, diags, sess) = parse_decl("arr[static const 10]");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a)] => {
                assert!(a.has_static);
                assert!(a.quals.const_);
                let s = a.size.as_ref().expect("size present");
                assert_eq!(int_lit_text(s, &sess), "10");
            }
            other => panic!("expected Array[static const N], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn function_with_prototype_params_parses() {
        // `f(int, char)` — two abstract (un-named) parameters. The
        // declarator itself names `f`; the parameters carry just
        // specifiers with an empty declarator each.
        let (d, _rem, diags, sess) = parse_decl("f(int, char)");
        assert_name(&d, &sess, "f");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert!(!fd.is_void);
                assert!(!fd.variadic);
                assert!(fd.kr_names.is_empty());
                assert_eq!(fd.params.len(), 2);
                let (a, b) = (&fd.params[0], &fd.params[1]);
                assert!(matches!(a.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(matches!(b.specs.type_specs.as_slice(), [TypeSpec::Char]));
                assert!(a.declarator.name.is_none(), "abstract param");
                assert!(b.declarator.name.is_none(), "abstract param");
            }
            other => panic!("expected Function(int, char), got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn function_void_prototype_parses() {
        // `f(void)` — explicit zero-parameter prototype. `is_void` is
        // set; the params vec remains empty.
        let (d, _rem, diags, _sess) = parse_decl("f(void)");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert!(fd.is_void);
                assert!(fd.params.is_empty());
                assert!(!fd.variadic);
            }
            other => panic!("expected Function(void), got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn function_variadic_prototype_parses() {
        // `f(int, ...)` — trailing `...` sets `variadic`.
        let (d, _rem, diags, _sess) = parse_decl("f(int, ...)");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert!(fd.variadic);
                assert_eq!(fd.params.len(), 1);
            }
            other => panic!("expected Function(int, ...), got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn function_empty_params_parses() {
        // `f()` — empty parameter list, NOT the same as `(void)`:
        // this is an unspecified-arguments prototype in C99 §6.7.5.3p14.
        let (d, _rem, diags, _sess) = parse_decl("f()");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert!(!fd.is_void);
                assert!(fd.params.is_empty());
                assert!(!fd.variadic);
            }
            other => panic!("expected Function(), got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn nested_pointer_to_function_parses() {
        // `(*fp)(int)` — fp is a pointer to function. The `*` lives
        // inside parens, so the `(int)` suffix applies to the whole
        // parenthesised declarator rather than to `fp` directly.
        // Chain (outer-to-inner): Pointer, then Function.
        let (d, _rem, diags, sess) = parse_decl("(*fp)(int)");
        assert_name(&d, &sess, "fp");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(_), DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 1);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
            }
            other => panic!("expected [Pointer, Function], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn array_of_pointers_to_functions_parses() {
        // The canonical task-19 acceptance case. `(*fp[3])(int, int)`
        // — fp is an array of 3 of pointers to functions taking
        // `(int, int)` and returning the base type. Chain must be
        // exactly [Array(3), Pointer, Function(int, int)].
        let (d, _rem, diags, sess) = parse_decl("(*fp[3])(int, int)");
        assert_name(&d, &sess, "fp");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(arr), DerivedDeclarator::Pointer(_), DerivedDeclarator::Function(fd)] =>
            {
                let s = arr.size.as_ref().expect("array size present");
                assert_eq!(int_lit_text(s, &sess), "3");
                assert!(!arr.has_static && !arr.star);
                assert_eq!(fd.params.len(), 2);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(matches!(fd.params[1].specs.type_specs.as_slice(), [TypeSpec::Int]));
            }
            other => panic!("expected [Array(3), Pointer, Function], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn array_of_pointers_parses() {
        // `*p[3]`: postfix `[]` binds tighter than prefix `*`. p is
        // therefore an array-of-3 of pointers, not a pointer to an
        // array. Chain: [Array(3), Pointer].
        let (d, _rem, diags, sess) = parse_decl("*p[3]");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(arr), DerivedDeclarator::Pointer(_)] => {
                let s = arr.size.as_ref().expect("size present");
                assert_eq!(int_lit_text(s, &sess), "3");
            }
            other => panic!("expected [Array, Pointer], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn pointer_to_array_parses() {
        // `(*p)[3]`: p is a POINTER to array of 3. Parens force the
        // outer ordering. Chain: [Pointer, Array(3)].
        let (d, _rem, diags, sess) = parse_decl("(*p)[3]");
        assert_name(&d, &sess, "p");
        match d.derived.as_slice() {
            [DerivedDeclarator::Pointer(_), DerivedDeclarator::Array(arr)] => {
                let s = arr.size.as_ref().expect("size present");
                assert_eq!(int_lit_text(s, &sess), "3");
            }
            other => panic!("expected [Pointer, Array], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn function_with_named_params_parses() {
        // `f(int x, char y)` — parameters carry names. Each parameter
        // declarator has a name slot populated.
        let (d, _rem, diags, sess) = parse_decl("f(int x, char y)");
        assert_name(&d, &sess, "f");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 2);
                let (xs, _) = fd.params[0].declarator.name.as_ref().expect("x");
                let (ys, _) = fd.params[1].declarator.name.as_ref().expect("y");
                assert_eq!(sess.interner.get(*xs), "x");
                assert_eq!(sess.interner.get(*ys), "y");
            }
            other => panic!("expected Function with two named params, got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn function_with_pointer_param_parses() {
        // `f(int *p)` — pointer parameter. The parameter declarator
        // chain has the [Pointer] layer attached to its own name.
        let (d, _rem, diags, _sess) = parse_decl("f(int *p)");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 1);
                let pd = &fd.params[0].declarator;
                assert!(pd.name.is_some());
                assert!(matches!(pd.derived.as_slice(), [DerivedDeclarator::Pointer(_)]));
            }
            other => panic!("expected Function([Pointer p]), got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn declarator_span_covers_all_consumed_tokens() {
        // The declarator's span must extend from the first consumed
        // token to the last — regression guard against helpers that
        // only track the name's span.
        let src = "(*fp[3])(int, int)";
        let (d, _rem, _diags, _sess) = parse_decl(src);
        assert_eq!(d.span.lo.0, 0);
        assert_eq!(d.span.hi.0 as usize, src.len());
    }

    #[test]
    fn duplicate_pointer_qualifier_warns_w0004() {
        // `*const const p` — repeated qualifier inside the same
        // qualifier list warns W0004, consistent with the specifier-
        // list rule (C99 §6.7.3p4).
        let (_d, _rem, diags, _sess) = parse_decl("*const const p");
        assert_eq!(codes_of(&diags), vec!["W0004"], "{diags:?}");
    }

    #[test]
    fn missing_name_errors() {
        // `*` on its own is not a valid concrete declarator — the
        // name is mandatory here; abstract-declarator parsing lives
        // in [`parse_abstract_declarator`] and is exercised by the
        // §6.7.6 tests further below.
        let src = "*";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let d = parse_declarator(&mut parser);
        assert!(d.is_none(), "missing name should bail");
        let diags = cap.diagnostics();
        assert!(!diags.is_empty(), "expected a diagnostic");
    }

    // ── Abstract declarator + type-name (C99 §6.7.6) ────────────────
    //
    // `parse_type_name` wraps `parse_decl_specs` + `parse_abstract_
    // declarator`; exercising it end-to-end keeps the helpers below
    // small.

    fn parse_tn(src: &str) -> (TypeName, usize, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let total = tokens.len();
        let mut parser = Parser::new(&mut sess, tokens);
        let tn = parse_type_name(&mut parser);
        let remaining = total.saturating_sub(parser.cursor);
        (tn, remaining, cap.diagnostics(), sess)
    }

    #[test]
    fn type_name_plain_int_parses() {
        // `int` — bare type-name with an empty abstract declarator.
        let (tn, rem, diags, _sess) = parse_tn("int");
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
        assert!(tn.declarator.name.is_none());
        assert!(tn.declarator.derived.is_empty());
        assert_eq!(rem, 0);
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn type_name_pointer_parses() {
        // `int *` — pointer abstract declarator.
        let (tn, _rem, diags, _sess) = parse_tn("int *");
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
        assert!(tn.declarator.name.is_none());
        assert!(matches!(tn.declarator.derived.as_slice(), [DerivedDeclarator::Pointer(_)]));
        assert!(diags.is_empty());
    }

    #[test]
    fn type_name_pointer_qualified_parses() {
        // `int *const` — a const-qualified pointer. The qualifier
        // sits on the pointer, not the pointee (§6.7.5.1p1).
        let (tn, _rem, diags, _sess) = parse_tn("int *const");
        match tn.declarator.derived.as_slice() {
            [DerivedDeclarator::Pointer(q)] => {
                assert!(q.const_ && !q.volatile);
            }
            other => panic!("expected [Pointer(const)], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn type_name_array_parses() {
        // `int [3]` — abstract declarator is a sole `[3]` suffix.
        let (tn, _rem, diags, sess) = parse_tn("int [3]");
        assert!(tn.declarator.name.is_none());
        match tn.declarator.derived.as_slice() {
            [DerivedDeclarator::Array(a)] => {
                let s = a.size.as_ref().expect("size present");
                assert_eq!(int_lit_text(s, &sess), "3");
            }
            other => panic!("expected [Array(3)], got {other:?}"),
        }
        assert!(diags.is_empty());
    }

    #[test]
    fn type_name_function_pointer_parses() {
        // `int (*)(int)` — function pointer. This is the canonical
        // acceptance shape for task 05-20. Chain must be exactly
        // [Pointer, Function(int)]: the abstract declarator reads
        // the nested `(*)`, then the outer `(int)` attaches as a
        // function suffix of the whole parenthesised atom.
        let (tn, _rem, diags, _sess) = parse_tn("int (*)(int)");
        assert!(tn.declarator.name.is_none());
        match tn.declarator.derived.as_slice() {
            [DerivedDeclarator::Pointer(_), DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 1);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(fd.params[0].declarator.name.is_none());
            }
            other => panic!("expected [Pointer, Function(int)], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn type_name_with_name_errors_e0062() {
        // `int *foo` where a type-name was expected: `foo` is a name
        // and abstract declarators must not carry one — E0062.
        // Recovery keeps the rest of the shape so the pointer chain
        // still lands in the declarator.
        let (tn, _rem, diags, _sess) = parse_tn("int *foo");
        assert_eq!(codes_of(&diags), vec!["E0062"], "{diags:?}");
        assert!(tn.declarator.name.is_none(), "name recovered away");
        assert!(matches!(tn.declarator.derived.as_slice(), [DerivedDeclarator::Pointer(_)]));
    }

    #[test]
    fn param_with_nested_abstract_function_pointer_parses() {
        // Regression for the shared atom-parser: now that parameter
        // declarators recurse with ctx=Param, a nested abstract
        // function-pointer parameter (`int (*)(int)` inside a
        // parameter list) parses correctly. Before task 05-20 the
        // nested recursion was Concrete, which required an ident.
        let (d, _rem, diags, sess) = parse_decl("f(int (*)(int))");
        assert_name(&d, &sess, "f");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 1);
                let p0 = &fd.params[0];
                assert!(p0.declarator.name.is_none());
                match p0.declarator.derived.as_slice() {
                    [DerivedDeclarator::Pointer(_), DerivedDeclarator::Function(inner)] => {
                        assert_eq!(inner.params.len(), 1);
                    }
                    other => panic!("expected [Pointer, Function] param, got {other:?}"),
                }
            }
            other => panic!("expected Function([nested abstract]), got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn type_name_span_covers_all_consumed_tokens() {
        // End-to-end span: type-name's span must span from the first
        // specifier token to the last declarator token.
        let src = "int (*)(int)";
        let (tn, _rem, _diags, _sess) = parse_tn(src);
        assert_eq!(tn.span.lo.0, 0);
        assert_eq!(tn.span.hi.0 as usize, src.len());
    }
}
