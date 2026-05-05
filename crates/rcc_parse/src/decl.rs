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
//! The full parser uses this lookup at every declaration, type-name,
//! parameter, and expression disambiguation site. Fresh typedef names
//! are registered immediately after their declarator is parsed, so
//! later declarators and later declarations see the same scoped name
//! classification that C requires.
//!
//! ## Struct / union / enum
//!
//! [`parse_record_spec`] implements C99 §6.7.2.1 — a `struct` or
//! `union` specifier followed by an optional tag and an optional
//! `{ field-decl* }` body. A field-decl is a specifier-qualifier
//! list followed by a comma-separated list of field declarators,
//! each of which may be a plain declarator, an anonymous bitfield
//! (`: width`), or a named bitfield (`declarator : width`). A bare
//! tag reference (e.g. `struct S *p;`) leaves `fields = None`.
//!
//! [`parse_enum_spec`] implements C99 §6.7.2.2 — an `enum` keyword
//! with an optional tag and an optional `{ enumerator-list , }`
//! body. An enumerator is an identifier optionally followed by
//! `= constant-expression`; a trailing comma is permitted
//! (§6.7.2.2p5). An empty body `enum {}` is a constraint violation.
//!
//! Duplicate-name detection, underlying-type selection for `enum`,
//! and enumerator-value evaluation are all deferred to HIR lowering
//! — the parser's job here is purely syntactic.

use rcc_ast::{
    ArrayDeclarator, Decl, DeclSpecs, Declarator, DerivedDeclarator, EnumSpec, Enumerator, Expr,
    ExternalDecl, FieldDecl, FieldDeclarator, FunctionDeclarator, FunctionDef, InitDeclarator,
    ParamDecl, RecordKind, RecordSpec, StorageClass, TypeName, TypeQuals, TypeSpec,
};
use rcc_errors::codes;
use rcc_lexer::Punct;
use rcc_span::{Span, Symbol};

use crate::keywords::Keyword;
use crate::scope::NameKind;
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
    Typeof,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum QualifierAliasKind {
    Const,
    Volatile,
    Restrict,
}

pub(crate) fn qualifier_alias_kind(p: &Parser<'_>, sym: Symbol) -> Option<QualifierAliasKind> {
    if !(p.session.opts.linux_gnu_hosted || p.session.opts.gnu_qualifier_aliases) {
        return None;
    }
    match p.session.interner.get(sym) {
        "__const" | "__const__" => Some(QualifierAliasKind::Const),
        "__volatile" | "__volatile__" => Some(QualifierAliasKind::Volatile),
        "__restrict" | "__restrict__" | "__restrict_arr" => Some(QualifierAliasKind::Restrict),
        _ => None,
    }
}

pub(crate) fn ident_is_type_qualifier_alias(p: &Parser<'_>, sym: Symbol) -> bool {
    qualifier_alias_kind(p, sym).is_some()
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
///   lives in the declaration parser, not here.
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
        if crate::attr::peek_attribute(p) {
            let attrs = crate::attr::parse_attributes(p);
            let attr_end = attrs.last().map(|a| a.span).unwrap_or(tok_span);
            specs.attrs.extend(attrs);
            if first_span.is_none() {
                first_span = Some(tok_span);
            }
            last_span = attr_end;
            continue;
        }

        match &tok.kind {
            TokenKind::Keyword(kw) => {
                if !consume_kw_specifier(p, *kw, tok_span, &mut specs, &mut state) {
                    break;
                }
            }
            TokenKind::Ident(sym) => {
                let sym = *sym;
                if consume_qualifier_alias(p, sym, tok_span, &mut specs.quals) {
                    if first_span.is_none() {
                        first_span = Some(tok_span);
                    }
                    last_span = tok_span;
                    continue;
                }
                if is_gnu_typeof_name(p, sym) {
                    accept_typeof(p, &mut specs, &mut state, tok_span);
                    if first_span.is_none() {
                        first_span = Some(tok_span);
                    }
                    last_span = last_consumed_span(p, tok_span);
                    continue;
                }
                // Typedef-name recognition: only when no type
                // specifier has been accepted yet for this run.
                if state.base.is_none()
                    && state.short_count == 0
                    && state.long_count == 0
                    && !state.signed_flag
                    && !state.unsigned_flag
                    && !state.complex_flag
                    && !state.imaginary_flag
                {
                    let name = p.session.interner.get(sym);
                    if name == "__builtin_va_list" {
                        specs.type_specs.push(TypeSpec::BuiltinVaList);
                        state.base = Some(BaseKind::Typedef);
                        p.bump();
                    } else if p.scopes.is_typedef(sym) {
                        specs.type_specs.push(TypeSpec::TypedefName(sym));
                        state.base = Some(BaseKind::Typedef);
                        p.bump();
                    } else {
                        break;
                    }
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

fn consume_qualifier_alias(
    p: &mut Parser<'_>,
    sym: Symbol,
    span: Span,
    quals: &mut TypeQuals,
) -> bool {
    let Some(kind) = qualifier_alias_kind(p, sym) else {
        return false;
    };
    match kind {
        QualifierAliasKind::Const => accept_qual(p, &mut quals.const_, "__const", span),
        QualifierAliasKind::Volatile => accept_qual(p, &mut quals.volatile, "__volatile", span),
        QualifierAliasKind::Restrict => accept_qual(p, &mut quals.restrict, "__restrict", span),
    }
    true
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
        BaseKind::Typedef | BaseKind::Record | BaseKind::Enum | BaseKind::Typeof => {
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
        let _ = parse_record_spec(p, kind);
        return;
    }
    let rec = parse_record_spec(p, kind);
    state.base = Some(BaseKind::Record);
    specs.type_specs.push(TypeSpec::Record(rec));
}

fn accept_enum(p: &mut Parser<'_>, specs: &mut DeclSpecs, state: &mut TypeState, span: Span) {
    if state.base.is_some() || !is_type_state_clean(state) {
        specifier_conflict(p, "cannot combine `enum` with previous type specifier", span);
        let _ = parse_enum_spec(p);
        return;
    }
    let e = parse_enum_spec(p);
    state.base = Some(BaseKind::Enum);
    specs.type_specs.push(TypeSpec::Enum(e));
}

fn accept_typeof(p: &mut Parser<'_>, specs: &mut DeclSpecs, state: &mut TypeState, span: Span) {
    if state.base.is_some() || !is_type_state_clean(state) {
        specifier_conflict(p, "cannot combine `typeof` with previous type specifier", span);
        let _ = parse_gnu_typeof_spec(p, span);
        return;
    }
    if let Some(spec) = parse_gnu_typeof_spec(p, span) {
        state.base = Some(BaseKind::Typeof);
        specs.type_specs.push(spec);
    }
}

fn parse_gnu_typeof_spec(p: &mut Parser<'_>, typeof_span: Span) -> Option<TypeSpec> {
    if !p.session.opts.gnu_typeof {
        p.session
            .handler
            .struct_warn(typeof_span, "GNU `typeof` type specifier is not part of C99")
            .code(codes::W0024)
            .note("parsing it as an extension for compatibility declarations")
            .emit();
    }

    p.bump(); // `typeof` / `__typeof` / `__typeof__`

    let _lparen_span = match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::LParen)) => {
            let span = tok.span;
            p.bump();
            span
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected `(` after GNU `typeof`")
                .code(codes::E0061)
                .emit();
            return None;
        }
    };

    let spec = if starts_type_name_at_cursor(p) {
        let ty = parse_type_name(p);
        TypeSpec::TypeofType(Box::new(ty))
    } else {
        let expr = crate::expr::parse_expression(p)?;
        TypeSpec::TypeofExpr(Box::new(expr))
    };
    expect_rparen(p, "GNU `typeof`");
    Some(spec)
}

fn starts_type_name_at_cursor(p: &Parser<'_>) -> bool {
    let at = crate::attr::skip_attribute_groups_at(p, p.cursor);
    match p.tokens.get(at).map(|t| &t.kind) {
        Some(TokenKind::Keyword(kw)) => is_type_name_start_kw(*kw),
        Some(TokenKind::Ident(sym)) => p.scopes.is_typedef(*sym),
        _ => false,
    }
}

fn is_type_name_start_kw(kw: Keyword) -> bool {
    matches!(
        kw,
        Keyword::Void
            | Keyword::Char
            | Keyword::Short
            | Keyword::Int
            | Keyword::Long
            | Keyword::Float
            | Keyword::Double
            | Keyword::Signed
            | Keyword::Unsigned
            | Keyword::Bool
            | Keyword::Complex
            | Keyword::Imaginary
            | Keyword::Struct
            | Keyword::Union
            | Keyword::Enum
            | Keyword::Const
            | Keyword::Volatile
            | Keyword::Restrict
    )
}

fn is_gnu_typeof_name(p: &Parser<'_>, sym: Symbol) -> bool {
    matches!(p.session.interner.get(sym), "typeof" | "__typeof" | "__typeof__")
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
//  Tagged types: struct / union (C99 §6.7.2.1) and enum (§6.7.2.2)
// ─────────────────────────────────────────────────────────────────────

/// Parse a *struct-or-union-specifier* (C99 §6.7.2.1). Consumes the
/// `struct`/`union` keyword, an optional tag identifier, and — if
/// present — a full `{ field-decl* }` body. A bare reference
/// (`struct S`) leaves `fields = None`; a definition with or without
/// tag yields `fields = Some(..)` — possibly empty for `struct {}`
/// which is a C99 constraint violation diagnosed later at HIR.
///
/// The caller has already confirmed the lookahead is `struct` /
/// `union`; this function assumes the cursor is positioned on that
/// keyword.
pub(crate) fn parse_record_spec(p: &mut Parser<'_>, kind: RecordKind) -> RecordSpec {
    let kw_span = p.cur_span();
    p.bump(); // struct/union
    let attrs = crate::attr::parse_attributes(p);

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
    let mut fields: Option<Vec<FieldDecl>> = None;

    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
        let (fs, close) = parse_struct_body(p);
        end_span = close;
        fields = Some(fs);
    } else if tag.is_none() {
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
    RecordSpec { id, kind, tag: tag.map(|(sym, _)| sym), fields, span: kw_span.to(end_span), attrs }
}

/// Parse a `{ field-decl* }` struct/union body. Cursor must be on
/// `{`. Returns the field vector and the span of the closing `}`.
///
/// Each field-decl is `specifier-qualifier-list struct-declarator-
/// list ;`. On a parse failure inside a field-decl we skip forward
/// to the next `;` or the surrounding `}` so the remaining fields
/// still parse; bracket depth is tracked to stay inside the current
/// body level when skipping.
fn parse_struct_body(p: &mut Parser<'_>) -> (Vec<FieldDecl>, Span) {
    // Caller has already confirmed the cursor is on `{`.
    let open = p.bump().expect("caller peeked `{`").span;
    let mut fields: Vec<FieldDecl> = Vec::new();

    loop {
        match p.peek() {
            Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RBrace)) => {
                let close = tok.span;
                p.bump();
                return (fields, close);
            }
            Some(_) => {}
            None => {
                p.session
                    .handler
                    .struct_err(p.cur_span(), "unexpected end of input inside struct/union body")
                    .label(open, "unclosed `{` here")
                    .code(codes::E0061)
                    .emit();
                return (fields, open);
            }
        }

        match parse_field_decl(p) {
            Some(fd) => fields.push(fd),
            None => {
                skip_to_semi_or_rbrace(p);
                if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
                    p.bump();
                }
            }
        }
    }
}

/// Parse a single `struct-declaration` (C99 §6.7.2.1):
/// specifier-qualifier-list struct-declarator-list `;`.
///
/// Returns `None` when the specifier-qualifier-list was empty (a
/// constraint violation that we diagnose here and let the caller
/// recover from by skipping to the next `;` / `}`).
fn parse_field_decl(p: &mut Parser<'_>) -> Option<FieldDecl> {
    let start = p.cur_span();
    let specs = parse_decl_specs(p)?;

    let specs_empty = specs.type_specs.is_empty()
        && specs.storage.is_none()
        && !specs.quals.const_
        && !specs.quals.volatile
        && !specs.quals.restrict
        && !specs.func_specs.inline;
    if specs_empty {
        p.session
            .handler
            .struct_err(start, "expected type in struct/union field declaration")
            .code(codes::E0061)
            .emit();
        return None;
    }

    let mut declarators: Vec<FieldDeclarator> = Vec::new();
    loop {
        let fd = parse_field_declarator(p);
        declarators.push(fd);
        match p.peek().map(|t| &t.kind) {
            Some(TokenKind::Punct(Punct::Comma)) => {
                p.bump();
                continue;
            }
            _ => break,
        }
    }

    let end = match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::Semi)) => {
            let s = tok.span;
            p.bump();
            s
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected `;` after struct/union field declaration")
                .code(codes::E0061)
                .emit();
            p.cur_span()
        }
    };

    Some(FieldDecl { specs, declarators, span: start.to(end) })
}

/// Parse one *struct-declarator* (C99 §6.7.2.1):
///
/// ```text
/// struct-declarator:
///     declarator
///     declarator? : constant-expression
/// ```
///
/// Three shapes:
///
/// - `declarator`            — regular field
/// - `declarator : width`    — named bitfield
/// - `: width`               — anonymous bitfield (no declarator)
///
/// The bitfield width is a *constant-expression*; we parse it as an
/// assignment-expression (matching the existing array-size
/// precedent) and defer constant-folding to a later pass.
fn parse_field_declarator(p: &mut Parser<'_>) -> FieldDeclarator {
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Colon))) {
        p.bump();
        let bit_width = crate::expr::parse_assignment_expression(p);
        return FieldDeclarator { declarator: None, bit_width };
    }

    let declarator = parse_declarator(p);
    let bit_width = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Colon))) {
        p.bump();
        crate::expr::parse_assignment_expression(p)
    } else {
        None
    };
    FieldDeclarator { declarator, bit_width }
}

/// Parse an *enum-specifier* (C99 §6.7.2.2). Consumes `enum`, an
/// optional tag, and an optional `{ enumerator-list , }` body. A
/// bare reference (`enum E`) leaves `enumerators = None`; a
/// definition yields `enumerators = Some(..)`.
///
/// An empty body `enum {}` is a constraint violation per §6.7.2.2p1
/// ("An identifier declared as an enumeration constant …"), reported
/// as E0061 here; the empty enumerators vector is still stored so
/// downstream shape checks do not choke on `None`.
pub(crate) fn parse_enum_spec(p: &mut Parser<'_>) -> EnumSpec {
    let kw_span = p.cur_span();
    p.bump(); // enum
    let attrs = crate::attr::parse_attributes(p);

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
    let mut enumerators: Option<Vec<Enumerator>> = None;

    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
        let (list, close) = parse_enum_body(p);
        end_span = close;
        enumerators = Some(list);
    } else if tag.is_none() {
        p.session
            .handler
            .struct_err(kw_span, "`enum` specifier needs a tag or a `{` body")
            .code(codes::E0061)
            .emit();
    }

    let id = p.fresh_id();
    EnumSpec { id, tag: tag.map(|(sym, _)| sym), enumerators, span: kw_span.to(end_span), attrs }
}

/// Parse a `{ enumerator-list , }` body. Cursor must be on `{`.
/// Returns the enumerator vector and the span of the closing `}`.
///
/// The trailing comma is optional (C99 §6.7.2.2p1 grammar admits
/// both `{ list }` and `{ list , }`). An empty body is a constraint
/// violation (§6.7.2.2p1 requires at least one enumerator).
fn parse_enum_body(p: &mut Parser<'_>) -> (Vec<Enumerator>, Span) {
    // Caller has already confirmed the cursor is on `{`.
    let open = p.bump().expect("caller peeked `{`").span;
    let mut list: Vec<Enumerator> = Vec::new();

    // Empty `{}` is a constraint violation.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::RBrace))) {
        let close = p.cur_span();
        p.bump();
        p.session
            .handler
            .struct_err(open, "`enum` declaration requires at least one enumerator")
            .code(codes::E0061)
            .emit();
        return (list, close);
    }

    loop {
        // Trailing-comma support: a `}` after a `,` ends the list.
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::RBrace))) {
            break;
        }
        match parse_enumerator(p) {
            Some(en) => list.push(en),
            None => {
                skip_until_comma_or_rbrace(p);
            }
        }
        match p.peek().map(|t| &t.kind) {
            Some(TokenKind::Punct(Punct::Comma)) => {
                p.bump();
                continue;
            }
            _ => break,
        }
    }

    let close = match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::RBrace)) => {
            let s = tok.span;
            p.bump();
            s
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected `}` to close `enum` body")
                .label(open, "unclosed `{` here")
                .code(codes::E0061)
                .emit();
            open
        }
    };
    (list, close)
}

/// Parse one *enumerator* (C99 §6.7.2.2):
///
/// ```text
/// enumerator:  enumeration-constant
///              enumeration-constant = constant-expression
/// ```
///
/// Returns `None` when the name slot is missing — the caller then
/// synchronises to the next `,` or `}` for recovery.
fn parse_enumerator(p: &mut Parser<'_>) -> Option<Enumerator> {
    let (sym, name_span) = match p.peek() {
        Some(tok) => match tok.kind {
            TokenKind::Ident(sym) => {
                let s = tok.span;
                p.bump();
                (sym, s)
            }
            _ => {
                let sp = tok.span;
                p.session
                    .handler
                    .struct_err(sp, "expected enumerator name")
                    .code(codes::E0061)
                    .emit();
                return None;
            }
        },
        None => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected enumerator name before end of input")
                .code(codes::E0061)
                .emit();
            return None;
        }
    };

    let mut attrs = crate::attr::parse_attributes(p);
    let value = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Eq))) {
        p.bump();
        crate::expr::parse_assignment_expression(p)
    } else {
        None
    };
    attrs.extend(crate::attr::parse_attributes(p));
    let end = match &value {
        Some(e) => e.span,
        None => attrs.last().map(|a| a.span).unwrap_or(name_span),
    };
    Some(Enumerator { name: sym, value, span: name_span.to(end), attrs })
}

/// Recovery: advance the cursor until a top-level `;` or `}` is
/// seen, without consuming it. Nested `{ ... }` blocks are skipped
/// over so that e.g. a nested-struct initialiser inside a broken
/// field doesn't terminate our scan prematurely.
fn skip_to_semi_or_rbrace(p: &mut Parser<'_>) {
    let mut depth: u32 = 0;
    while let Some(tok) = p.peek() {
        match &tok.kind {
            TokenKind::Punct(Punct::LBrace) => {
                depth += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RBrace) => {
                if depth == 0 {
                    return;
                }
                depth -= 1;
                p.bump();
            }
            TokenKind::Punct(Punct::Semi) if depth == 0 => return,
            _ => {
                p.bump();
            }
        }
    }
}

/// Recovery: advance the cursor until a top-level `,` or `}` is
/// seen (not consumed). Used inside the enumerator-list loop so a
/// malformed enumerator doesn't derail the whole list.
fn skip_until_comma_or_rbrace(p: &mut Parser<'_>) {
    let mut depth: u32 = 0;
    while let Some(tok) = p.peek() {
        match &tok.kind {
            TokenKind::Punct(Punct::LBrace) | TokenKind::Punct(Punct::LParen) => {
                depth += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RBrace) => {
                if depth == 0 {
                    return;
                }
                depth -= 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RParen) => {
                depth = depth.saturating_sub(1);
                p.bump();
            }
            TokenKind::Punct(Punct::Comma) if depth == 0 => return,
            _ => {
                p.bump();
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
//  External declarations (C99 §6.9)
// ─────────────────────────────────────────────────────────────────────

/// Parse a top-level *external-declaration* (C99 §6.9):
///
/// ```text
/// external-declaration:
///     function-definition
///     declaration
///
/// function-definition:
///     declaration-specifiers declarator declaration-list? compound-statement
///
/// declaration:
///     declaration-specifiers init-declarator-list? ;
/// ```
///
/// Disambiguation: after parsing `declaration-specifiers declarator`,
/// peek at the current token:
///
/// - `{`        → function definition
/// - `;`, `,`, `=` → declaration
///
/// K&R-style `declaration-list` between declarator and `{` is parsed
/// when the declarator carries an identifier-list.
pub fn parse_external_decl(p: &mut Parser<'_>) -> Option<ExternalDecl> {
    let start = p.cur_span();
    let specs = parse_decl_specs(p)?;

    // Bare `;` after specifiers → declaration with no init-declarator-list.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
        let end = p.cur_span();
        p.bump();
        let id = p.fresh_id();
        return Some(ExternalDecl::Decl(Decl {
            id,
            span: start.to(end),
            specs,
            inits: Vec::new(),
        }));
    }

    // Parse the first declarator.
    let declarator = parse_declarator(p)?;

    // K&R-style function definition: the declarator has an identifier
    // list (kr_names) instead of prototype parameters. Parse the
    // declaration-list between `)` and `{`, emit W0005, and validate.
    let has_kr_names = declarator
        .derived
        .last()
        .is_some_and(|d| matches!(d, DerivedDeclarator::Function(fd) if !fd.kr_names.is_empty()));
    if has_kr_names {
        // Emit obsolescence warning.
        p.session
            .handler
            .struct_warn(declarator.span, "K&R function definition is obsolete")
            .code(codes::W0005)
            .help("rewrite using prototype syntax")
            .emit();

        // Collect the identifier list for validation.
        let kr_name_set: rcc_data_structures::FxHashSet<Symbol> =
            if let Some(DerivedDeclarator::Function(fd)) = declarator.derived.last() {
                fd.kr_names.iter().map(|(s, _)| *s).collect()
            } else {
                Default::default()
            };

        // Parse K&R declaration list (declarations before `{`).
        let mut kr_decls = Vec::new();
        while !matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace)) | None) {
            let before = p.cursor;
            let err_before = p.session.handler.error_count();
            if let Some(decl) = parse_declaration(p) {
                // Validate: every name in this declaration must appear
                // in the identifier list.
                for init in &decl.inits {
                    if let Some((sym, span)) = init.declarator.name {
                        if !kr_name_set.contains(&sym) {
                            let name = p.session.interner.get(sym).to_owned();
                            p.session
                                .handler
                                .struct_err(
                                    span,
                                    format!("parameter `{name}` not found in identifier list"),
                                )
                                .code(codes::E0063)
                                .emit();
                        }
                    }
                }
                kr_decls.push(decl);
            } else {
                if p.session.handler.error_count() == err_before {
                    p.session
                        .handler
                        .struct_err(
                            p.cur_span(),
                            "expected K&R parameter declaration or function body",
                        )
                        .code(codes::E0030)
                        .emit();
                }
                recover_kr_decl_list(p);
                if p.cursor == before
                    && !matches!(
                        p.peek().map(|t| &t.kind),
                        Some(TokenKind::Punct(Punct::LBrace)) | None
                    )
                {
                    p.bump();
                }
            }
        }

        declare_declarator_name(p, &specs, &declarator);
        let body = crate::stmt::parse_block(p)?;
        let end = body.span;
        let id = p.fresh_id();
        return Some(ExternalDecl::Function(FunctionDef {
            id,
            span: start.to(end),
            specs,
            declarator,
            kr_decls,
            body,
        }));
    }

    // Disambiguate: `{` → function definition.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
        declare_declarator_name(p, &specs, &declarator);
        let body = crate::stmt::parse_block(p)?;
        let end = body.span;
        let id = p.fresh_id();
        return Some(ExternalDecl::Function(FunctionDef {
            id,
            span: start.to(end),
            specs,
            declarator,
            kr_decls: Vec::new(),
            body,
        }));
    }

    // Otherwise it's a declaration — finish the init-declarator-list.
    let init = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Eq))) {
        p.bump();
        crate::init::parse_initializer(p)
    } else {
        None
    };
    declare_declarator_name(p, &specs, &declarator);
    let mut inits = vec![InitDeclarator { declarator, init }];

    while matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma))) {
        p.bump();
        let d = match parse_declarator(p) {
            Some(d) => d,
            None => break,
        };
        let init = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Eq))) {
            p.bump();
            crate::init::parse_initializer(p)
        } else {
            None
        };
        declare_declarator_name(p, &specs, &d);
        inits.push(InitDeclarator { declarator: d, init });
    }

    let end = match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::Semi)) => {
            let s = tok.span;
            p.bump();
            s
        }
        _ => {
            p.session.handler.struct_err(p.cur_span(), "expected `;` after declaration").emit();
            last_consumed_span(p, start)
        }
    };

    let id = p.fresh_id();
    Some(ExternalDecl::Decl(Decl { id, span: start.to(end), specs, inits }))
}

/// Parse a `declaration` inside a block (C99 §6.7):
///
/// ```text
/// declaration:
///     declaration-specifiers init-declarator-list? ;
/// ```
///
/// Returns `None` when the cursor is not positioned on something that
/// starts a declaration. The caller ([`parse_block_item`]) uses this
/// to fall through to the statement path.
pub fn parse_declaration(p: &mut Parser<'_>) -> Option<Decl> {
    let start = p.cur_span();
    let saved_cursor = p.cursor;
    let specs = parse_decl_specs(p)?;

    // If the specifier list consumed nothing, it is not a declaration.
    let specs_empty = specs.type_specs.is_empty()
        && specs.storage.is_none()
        && !specs.quals.const_
        && !specs.quals.volatile
        && !specs.quals.restrict
        && !specs.func_specs.inline;
    if specs_empty {
        p.cursor = saved_cursor;
        return None;
    }

    // Bare `;` after specifiers.
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
        let end = p.cur_span();
        p.bump();
        let id = p.fresh_id();
        return Some(Decl { id, span: start.to(end), specs, inits: Vec::new() });
    }

    let declarator = parse_declarator(p)?;
    let init = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Eq))) {
        p.bump();
        crate::init::parse_initializer(p)
    } else {
        None
    };
    declare_declarator_name(p, &specs, &declarator);
    let mut inits = vec![InitDeclarator { declarator, init }];

    while matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma))) {
        p.bump();
        let d = match parse_declarator(p) {
            Some(d) => d,
            None => break,
        };
        let init = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Eq))) {
            p.bump();
            crate::init::parse_initializer(p)
        } else {
            None
        };
        declare_declarator_name(p, &specs, &d);
        inits.push(InitDeclarator { declarator: d, init });
    }

    let end = match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(Punct::Semi)) => {
            let s = tok.span;
            p.bump();
            s
        }
        _ => {
            p.session.handler.struct_err(p.cur_span(), "expected `;` after declaration").emit();
            last_consumed_span(p, start)
        }
    };

    let id = p.fresh_id();
    Some(Decl { id, span: start.to(end), specs, inits })
}

fn recover_kr_decl_list(p: &mut Parser<'_>) {
    while let Some(tok) = p.peek() {
        match tok.kind {
            TokenKind::Punct(Punct::Semi) => {
                p.bump();
                return;
            }
            TokenKind::Punct(Punct::LBrace) => return,
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
// We store the resulting chain in the order consumed by
// `rcc_hir_lower::apply_declarator`: each element wraps the
// declaration-specifier base type into the next intermediate type. For
// `int (*fp[3])(int, int)` the chain is therefore
//
//     [Function(int, int), Pointer, Array(3)]
//
// because base `T` first becomes a function type, then a pointer to
// that function, then an array of those pointers. Array suffixes from a
// single direct-declarator are therefore reversed for construction:
// `a[10][20]` stores `[Array(20), Array(10)]`.

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
/// `type-name` is stricter than a declaration: it permits only
/// type-specifiers and type-qualifiers. Storage-class specifiers
/// (`typedef`, `static`, ...) and function specifiers (`inline`) are
/// diagnosed here and stripped from the returned AST so downstream HIR
/// lowering never has to guess whether a type-name node was really a
/// declaration in disguise.
pub fn parse_type_name(p: &mut Parser<'_>) -> TypeName {
    let start = p.cur_span();
    let mut specs = parse_decl_specs(p).unwrap_or_default();
    validate_type_name_specs(p, &mut specs, start);
    let declarator = parse_abstract_declarator(p);
    let end = last_consumed_span(p, start);
    TypeName { specs, declarator, span: start.to(end) }
}

/// Enforce the `specifier-qualifier-list` subset accepted inside a
/// C99 type-name (§6.7.6). The shared declaration-specifier parser is
/// intentionally broad because real declarations, K&R definitions,
/// and parameter declarations all have slightly different contextual
/// constraints; type-name callers need the narrow contract here.
fn validate_type_name_specs(p: &mut Parser<'_>, specs: &mut DeclSpecs, start: Span) {
    let span = specs.span;

    if specs.storage.take().is_some() {
        p.session
            .handler
            .struct_err(span, "storage-class specifier is not allowed in a type name")
            .code(codes::E0061)
            .emit();
    }

    if specs.func_specs.inline {
        specs.func_specs.inline = false;
        p.session
            .handler
            .struct_err(span, "`inline` function specifier is not allowed in a type name")
            .code(codes::E0061)
            .emit();
    }

    if specs.type_specs.is_empty() {
        let err_span = if span == rcc_span::DUMMY_SP { start } else { span };
        p.session
            .handler
            .struct_err(err_span, "expected type specifier in type name")
            .code(codes::E0061)
            .emit();
    }
}

/// Post-declarator hook that generalises the C99 "lexer hack"
/// (§6.7.7, §6.7.2p2 footnote) across every declaration site.
///
/// When a top-level init-declarator has just been parsed, this
/// function registers the declared name in the *innermost* scope of
/// the parser's [`ScopeStack`], choosing
///
/// - [`NameKind::Typedef`] when `specs.storage == Some(StorageClass::Typedef)`,
/// - [`NameKind::Ordinary`] otherwise.
///
/// The entry is inserted **immediately**, before the parser crosses
/// the declaration's terminating `;`, so every downstream call into
/// [`parse_decl_specs`] / [`parse_type_name`] / [`parse_declarator`]
/// (including the next init-declarator in the same
/// `init-declarator-list`) consults an up-to-date typedef table.
/// This is what makes the declaration-specifier slot's "is this
/// ident a typedef-name?" lookup resolve correctly even inside the
/// same translation unit that introduced the typedef.
///
/// Abstract declarators — parameter declarators that omit the name,
/// or the trailing declarator of a `type-name` — are a no-op: there
/// is nothing to register.
///
/// The scope push / pop that realises C99 block-local shadowing
/// lives in [`crate::stmt::parse_block`] and
/// [`crate::stmt::parse_for_stmt`]; this hook only manipulates the
/// innermost frame those helpers maintain. A nested block's
/// `int T;` therefore shadows an outer `typedef int T;` exactly as
/// §6.2.1 mandates, because the inner declaration lands in a
/// pushed frame and the outer frame is restored on `}`.
pub fn declare_declarator_name(p: &mut Parser<'_>, specs: &DeclSpecs, declarator: &Declarator) {
    let Some((sym, _)) = declarator.name else {
        return;
    };
    let kind = if specs.storage == Some(StorageClass::Typedef) {
        NameKind::Typedef
    } else {
        NameKind::Ordinary
    };
    p.scopes.declare(sym, kind);
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
    let mut attrs = crate::attr::parse_attributes(p);
    let pointer_prefix = parse_pointer_prefix(p);
    let atom = parse_declarator_atom(p, ctx)?;
    let mut suffixes = Vec::new();
    parse_declarator_suffixes(p, &mut suffixes);
    attrs.extend(crate::attr::parse_attributes(p));
    // `rcc_hir_lower::apply_declarator` consumes the chain in the
    // order needed to wrap the declaration-specifier base type into
    // the final C type. Pointer prefixes at this level come before
    // suffixes (`int *a[3]` is array of pointer; `int *f()` is function
    // returning pointer). Suffixes at one level are seen from the
    // identifier outward, so reverse them for type construction. If
    // the atom was nested, current-level suffixes wrap the nested
    // declarator from the outside (`int (*fp)(int)` => Function, Ptr).
    let mut chain = Vec::new();
    chain.extend(pointer_prefix.into_iter().rev());
    chain.extend(suffixes.into_iter().rev());
    if atom.nested {
        chain.extend(atom.chain);
    } else {
        debug_assert!(atom.chain.is_empty());
    }
    let span =
        if p.cursor == start_cursor { start } else { start.to(last_consumed_span(p, start)) };
    Some(Declarator { name: atom.name, derived: chain, span, attrs })
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
        let (slot, name): (&mut bool, &str) = match &tok.kind {
            TokenKind::Keyword(Keyword::Const) => (&mut quals.const_, "const"),
            TokenKind::Keyword(Keyword::Volatile) => (&mut quals.volatile, "volatile"),
            TokenKind::Keyword(Keyword::Restrict) => (&mut quals.restrict, "restrict"),
            TokenKind::Ident(sym) => match qualifier_alias_kind(p, *sym) {
                Some(QualifierAliasKind::Const) => (&mut quals.const_, "__const"),
                Some(QualifierAliasKind::Volatile) => (&mut quals.volatile, "__volatile"),
                Some(QualifierAliasKind::Restrict) => (&mut quals.restrict, "__restrict"),
                None => break,
            },
            _ => break,
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

/// Result of parsing a direct-declarator atom.
struct DirectDecl {
    /// Optional identifier (name + span).
    name: Option<(Symbol, Span)>,
    /// Chain contributed by a parenthesized nested declarator.
    chain: Vec<DerivedDeclarator>,
    /// Whether this atom was a parenthesized nested declarator.
    nested: bool,
}

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
                    DeclCtx::Concrete | DeclCtx::Param => Some(DirectDecl {
                        name: Some((sym, span)),
                        chain: Vec::new(),
                        nested: false,
                    }),
                    DeclCtx::Abstract => {
                        p.session
                            .handler
                            .struct_err(span, "abstract declarator cannot contain a name")
                            .code(codes::E0062)
                            .emit();
                        Some(DirectDecl { name: None, chain: Vec::new(), nested: false })
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
                            Some(DirectDecl { name: d.name, chain: d.derived, nested: true })
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
                    Some(DirectDecl { name: None, chain: Vec::new(), nested: false })
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
                _ => Some(DirectDecl { name: None, chain: Vec::new(), nested: false }),
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
            _ => Some(DirectDecl { name: None, chain: Vec::new(), nested: false }),
        },
    }
}

/// One-token lookahead used by [`parse_declarator_atom`] to decide
/// whether a `(` at the atom position of a Param / Abstract declarator
/// opens a nested declarator or a function suffix on an empty direct-
/// declarator. See the docstring on the caller for the full table.
fn looks_like_nested_declarator(p: &Parser<'_>, at: usize, ctx: DeclCtx) -> bool {
    let at = crate::attr::skip_attribute_groups_at(p, at);
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
            TokenKind::Ident(sym) if ident_is_type_qualifier_alias(p, *sym) => {
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
/// K&R identifier-list style is also recognised here: when the
/// first token after `(` is a plain identifier (not a typedef-name)
/// followed by `,` or `)`, the list is parsed as an identifier-list
/// (C99 §6.7.5.3p1 "identifier-list") and stored in `kr_names`.
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

    // K&R identifier-list: `(x, y, z)` — plain idents separated by
    // commas, none of which is a known typedef-name.
    if let Some(tok) = p.peek() {
        if let TokenKind::Ident(sym) = tok.kind {
            let is_typedef = p.scopes.lookup(sym) == Some(NameKind::Typedef);
            if !is_typedef {
                if let Some(next) = p.tokens.get(p.cursor + 1) {
                    if matches!(
                        next.kind,
                        TokenKind::Punct(Punct::Comma) | TokenKind::Punct(Punct::RParen)
                    ) {
                        return parse_kr_identifier_list(p);
                    }
                }
            }
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

/// Parse a K&R identifier list: `ident, ident, ..., ident )`.
/// Cursor is on the first identifier. Returns a `FunctionDeclarator`
/// with `kr_names` populated.
fn parse_kr_identifier_list(p: &mut Parser<'_>) -> FunctionDeclarator {
    let mut kr_names: Vec<(Symbol, Span)> = Vec::new();
    loop {
        match p.peek() {
            Some(tok) if matches!(tok.kind, TokenKind::Ident(_)) => {
                if let TokenKind::Ident(sym) = tok.kind {
                    let span = tok.span;
                    p.bump();
                    kr_names.push((sym, span));
                }
            }
            _ => break,
        }
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma))) {
            p.bump();
        } else {
            break;
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
    FunctionDeclarator { params: Vec::new(), is_void: false, variadic: false, kr_names }
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
                let fs = rec.fields.as_ref().expect("body defines fields");
                assert_eq!(fs.len(), 1, "one field decl `int x;`");
                assert!(matches!(fs[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert_eq!(fs[0].declarators.len(), 1);
                let fd = &fs[0].declarators[0];
                assert!(fd.bit_width.is_none());
                let d = fd.declarator.as_ref().expect("named field");
                let (sym, _) = d.name.as_ref().expect("field has a name");
                assert_eq!(parser.session.interner.get(*sym), "x");
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
    fn enum_tagged_parses() {
        let src = "enum E { A, B, C }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        match specs.type_specs.as_slice() {
            [TypeSpec::Enum(e)] => {
                assert!(e.tag.is_some());
                assert_eq!(parser.session.interner.get(e.tag.unwrap()), "E");
                let list = e.enumerators.as_ref().expect("body defines enumerators");
                assert_eq!(list.len(), 3);
                assert_eq!(parser.session.interner.get(list[0].name), "A");
                assert_eq!(parser.session.interner.get(list[1].name), "B");
                assert_eq!(parser.session.interner.get(list[2].name), "C");
                assert!(list.iter().all(|en| en.value.is_none()));
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
            ExprKind::IntLit(lit) => sess.interner.get(lit.text).to_string(),
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
        // `arr[10][20]` — construction order is inner bound first:
        // array[20] of int, then array[10] of that.
        let (d, _rem, diags, sess) = parse_decl("arr[10][20]");
        assert_name(&d, &sess, "arr");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a20), DerivedDeclarator::Array(a10)] => {
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
        // Chain (construction order): Function, then Pointer.
        let (d, _rem, diags, sess) = parse_decl("(*fp)(int)");
        assert_name(&d, &sess, "fp");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd), DerivedDeclarator::Pointer(_)] => {
                assert_eq!(fd.params.len(), 1);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
            }
            other => panic!("expected [Function, Pointer], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn array_of_pointers_to_functions_parses() {
        // The canonical task-19 acceptance case. `(*fp[3])(int, int)`
        // — fp is an array of 3 of pointers to functions taking
        // `(int, int)` and returning the base type. Construction chain
        // must be exactly [Function(int, int), Pointer, Array(3)].
        let (d, _rem, diags, sess) = parse_decl("(*fp[3])(int, int)");
        assert_name(&d, &sess, "fp");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd), DerivedDeclarator::Pointer(_), DerivedDeclarator::Array(arr)] =>
            {
                let s = arr.size.as_ref().expect("array size present");
                assert_eq!(int_lit_text(s, &sess), "3");
                assert!(!arr.has_static && !arr.star);
                assert_eq!(fd.params.len(), 2);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(matches!(fd.params[1].specs.type_specs.as_slice(), [TypeSpec::Int]));
            }
            other => panic!("expected [Function, Pointer, Array(3)], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn array_of_pointers_parses() {
        // `*p[3]`: postfix `[]` binds tighter than prefix `*`. p is
        // therefore an array-of-3 of pointers, not a pointer to an
        // array. Construction chain: [Pointer, Array(3)].
        let (d, _rem, diags, sess) = parse_decl("*p[3]");
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
    fn pointer_to_array_parses() {
        // `(*p)[3]`: p is a POINTER to array of 3. Parens force the
        // outer ordering. Construction chain: [Array(3), Pointer].
        let (d, _rem, diags, sess) = parse_decl("(*p)[3]");
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
        // `int (*)(int)` — function pointer. This canonical shape's
        // chain must be exactly
        // [Function(int), Pointer]: the abstract declarator reads
        // the nested `(*)`, then the outer `(int)` attaches as a
        // function suffix of the whole parenthesised atom.
        let (tn, _rem, diags, _sess) = parse_tn("int (*)(int)");
        assert!(tn.declarator.name.is_none());
        match tn.declarator.derived.as_slice() {
            [DerivedDeclarator::Function(fd), DerivedDeclarator::Pointer(_)] => {
                assert_eq!(fd.params.len(), 1);
                assert!(matches!(fd.params[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(fd.params[0].declarator.name.is_none());
            }
            other => panic!("expected [Function(int), Pointer], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn type_name_rejects_storage_class_specifier() {
        let (tn, _rem, diags, _sess) = parse_tn("static int");
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
        assert_eq!(tn.specs.storage, None, "invalid storage is stripped for HIR");
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
    }

    #[test]
    fn type_name_rejects_typedef_storage_class() {
        let (tn, _rem, diags, _sess) = parse_tn("typedef int");
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
        assert_eq!(tn.specs.storage, None, "invalid typedef storage is stripped for HIR");
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
    }

    #[test]
    fn type_name_rejects_inline_function_specifier() {
        let (tn, _rem, diags, _sess) = parse_tn("inline int");
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
        assert!(!tn.specs.func_specs.inline, "invalid inline is stripped for HIR");
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
    }

    #[test]
    fn type_name_rejects_missing_type_specifier() {
        let (tn, _rem, diags, _sess) = parse_tn("const");
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
        assert!(tn.specs.quals.const_, "qualifier is retained for recovery");
        assert!(tn.specs.type_specs.is_empty());
    }

    #[test]
    fn type_name_duplicate_qualifier_warns_but_recovers() {
        let (tn, _rem, diags, _sess) = parse_tn("const const int");
        assert_eq!(codes_of(&diags), vec!["W0004"], "{diags:?}");
        assert!(tn.specs.quals.const_);
        assert!(matches!(tn.specs.type_specs.as_slice(), [TypeSpec::Int]));
    }

    #[test]
    fn type_name_accepts_typedef_name_as_specifier() {
        let src = "T *";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let t_sym = sess.interner.intern("T");
        let mut parser = Parser::new(&mut sess, tokens);
        parser.scopes.declare(t_sym, NameKind::Typedef);

        let tn = parse_type_name(&mut parser);
        match tn.specs.type_specs.as_slice() {
            [TypeSpec::TypedefName(sym)] => assert_eq!(parser.session.interner.get(*sym), "T"),
            other => panic!("expected typedef-name type specifier, got {other:?}"),
        }
        assert!(matches!(tn.declarator.derived.as_slice(), [DerivedDeclarator::Pointer(_)]));
        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
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
        // parameter list) parses correctly. The nested recursion must
        // stay in parameter context so it does not require an ident.
        let (d, _rem, diags, sess) = parse_decl("f(int (*)(int))");
        assert_name(&d, &sess, "f");
        match d.derived.as_slice() {
            [DerivedDeclarator::Function(fd)] => {
                assert_eq!(fd.params.len(), 1);
                let p0 = &fd.params[0];
                assert!(p0.declarator.name.is_none());
                match p0.declarator.derived.as_slice() {
                    [DerivedDeclarator::Function(inner), DerivedDeclarator::Pointer(_)] => {
                        assert_eq!(inner.params.len(), 1);
                    }
                    other => panic!("expected [Function, Pointer] param, got {other:?}"),
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

    // ── Typedef-name disambiguation (C99 §6.7.7) ────────────────────
    //
    // `declare_declarator_name` is the post-declarator hook that
    // feeds the parser's scope stack so downstream declaration-
    // specifier slots pick up freshly-introduced typedef-names. The
    // tests below drive the helper directly by interleaving
    // `parse_decl_specs` / `parse_declarator` / `declare_declarator_
    // name` calls against a single token stream. The public
    // declaration parsers perform the same interleaving while parsing
    // full declarations and function definitions.

    /// Consume a `;` token at the cursor. Used in the §6.7.7 tests
    /// where we drive declarations manually and need to step past
    /// the terminator between them.
    fn bump_semi(parser: &mut Parser<'_>) {
        match parser.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Semi)) => {
                parser.bump();
            }
            other => panic!("expected `;` at cursor, got {other:?}"),
        }
    }

    #[test]
    fn declare_declarator_name_registers_typedef() {
        // Canonical acceptance case: `typedef int T; T x;`. After the
        // `T` declarator is registered, parsing specs over `T x`
        // must recognise `T` as a TypedefName rather than the
        // declaration's declarator-ident.
        let src = "typedef int T ; T x";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);

        let specs1 = parse_decl_specs(&mut parser).expect("first specs");
        assert_eq!(specs1.storage, Some(StorageClass::Typedef));
        assert!(matches!(specs1.type_specs.as_slice(), [TypeSpec::Int]));

        let declarator1 = parse_declarator(&mut parser).expect("T declarator");
        declare_declarator_name(&mut parser, &specs1, &declarator1);
        let t_sym = declarator1.name.expect("declarator named `T`").0;
        assert_eq!(parser.session.interner.get(t_sym), "T");
        assert!(parser.scopes.is_typedef(t_sym), "T must be a typedef after hook");

        bump_semi(&mut parser);

        // `T x`: T is now a typedef-name, so parse_decl_specs should
        // consume T as a TypedefName type specifier and stop at `x`.
        let specs2 = parse_decl_specs(&mut parser).expect("second specs");
        assert_eq!(specs2.storage, None);
        match specs2.type_specs.as_slice() {
            [TypeSpec::TypedefName(sym)] => assert_eq!(parser.session.interner.get(*sym), "T"),
            other => panic!("expected [TypedefName], got {other:?}"),
        }
        let rem = &parser.tokens[parser.cursor];
        assert!(matches!(rem.kind, TokenKind::Ident(_)), "cursor left on `x`");

        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn declare_declarator_name_registers_ordinary() {
        // Non-typedef storage class: `int x;` must register `x` as
        // Ordinary so a later `x y;` does NOT pick up `x` as a
        // typedef-name (it isn't one).
        let src = "int x ; x y";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);

        let specs1 = parse_decl_specs(&mut parser).expect("first specs");
        let declarator1 = parse_declarator(&mut parser).expect("x declarator");
        declare_declarator_name(&mut parser, &specs1, &declarator1);
        let x_sym = declarator1.name.expect("declarator named `x`").0;
        assert!(!parser.scopes.is_typedef(x_sym), "x is ordinary, not a typedef");
        assert_eq!(parser.scopes.lookup(x_sym), Some(NameKind::Ordinary));

        bump_semi(&mut parser);

        // `x y`: parse_decl_specs must NOT swallow `x` as a type —
        // an ordinary ident in a specifier slot stops the loop.
        let at_x = parser.cursor;
        let specs2 = parse_decl_specs(&mut parser).expect("second specs");
        assert!(specs2.type_specs.is_empty(), "no type consumed");
        assert_eq!(parser.cursor, at_x, "ordinary ident not consumed");

        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn inner_ordinary_shadows_outer_typedef() {
        // C99 §6.2.1 block-scope shadowing. Source analogue:
        //
        //     typedef int T;     // outer: T is typedef
        //     { int T; T x; }    // inner: T is ordinary, shadowing
        //
        // At `T x;` inside the block, the parser must NOT treat T
        // as a type (per the task-21 acceptance: T is shadowed).
        // After the block closes, outer scope restores T as typedef,
        // so a trailing `T z;` at file scope again parses as a
        // declaration with TypedefName(T).
        let src = "typedef int T ; int T ; T x ; T z";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);

        // ── outer `typedef int T ;` ──────────────────────────────
        let outer_specs = parse_decl_specs(&mut parser).expect("outer typedef specs");
        assert_eq!(outer_specs.storage, Some(StorageClass::Typedef));
        let outer_decl = parse_declarator(&mut parser).expect("T declarator");
        declare_declarator_name(&mut parser, &outer_specs, &outer_decl);
        let t_sym = outer_decl.name.unwrap().0;
        assert!(parser.scopes.is_typedef(t_sym));
        bump_semi(&mut parser);

        // ── enter block scope ────────────────────────────────────
        let start_depth = parser.scopes.depth();
        parser.scopes.push();

        // Inside the block: `int T ;` — `int` is the base, `T` is
        // the declarator (the existing specifier-list logic stops
        // at `T` because the base is already `int`, not because T
        // is an ident-typedef at this slot).
        let inner_specs = parse_decl_specs(&mut parser).expect("inner int specs");
        assert_eq!(inner_specs.storage, None);
        assert!(matches!(inner_specs.type_specs.as_slice(), [TypeSpec::Int]));
        let inner_decl = parse_declarator(&mut parser).expect("T declarator inner");
        declare_declarator_name(&mut parser, &inner_specs, &inner_decl);
        assert_eq!(parser.scopes.lookup(t_sym), Some(NameKind::Ordinary), "inner T is ordinary");
        assert!(!parser.scopes.is_typedef(t_sym), "inner T is NOT a typedef");
        bump_semi(&mut parser);

        // `T x` inside the block: parse_decl_specs must leave the
        // cursor pinned on `T` — T is now Ordinary in the innermost
        // frame, which takes precedence over the outer Typedef
        // binding (C99 §6.2.1p4 inner scope hides outer).
        let at_t = parser.cursor;
        let inner_use_specs = parse_decl_specs(&mut parser).expect("specs over shadowed T");
        assert!(inner_use_specs.type_specs.is_empty(), "shadowed T must not be a type");
        assert_eq!(parser.cursor, at_t, "cursor did not advance past shadowed T");

        // Step manually past `T x ;` so the outer parse can resume.
        // parse_decl_specs consumed nothing, so `T` is still there:
        // the HIR pass would reject this inner line; the parser just
        // has to make forward progress for the rest of the suite.
        parser.bump(); // T
        parser.bump(); // x
        bump_semi(&mut parser);

        // ── leave block scope ────────────────────────────────────
        parser.scopes.pop();
        assert_eq!(parser.scopes.depth(), start_depth, "scope push/pop balanced");
        assert!(parser.scopes.is_typedef(t_sym), "outer scope restores T as typedef");

        // After the block, `T z` at outer scope parses as a
        // declaration with TypedefName(T) again.
        let outer_use_specs = parse_decl_specs(&mut parser).expect("outer use specs");
        match outer_use_specs.type_specs.as_slice() {
            [TypeSpec::TypedefName(sym)] => assert_eq!(*sym, t_sym),
            other => panic!("expected [TypedefName(T)], got {other:?}"),
        }
        let rem = &parser.tokens[parser.cursor];
        assert!(matches!(rem.kind, TokenKind::Ident(_)), "cursor on `z`");

        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    #[test]
    fn declare_declarator_name_ignores_abstract_declarator() {
        // Abstract declarators have no name — the hook is a no-op.
        // Using a synthesised empty declarator keeps the assertion
        // crisp without depending on parser behaviour.
        let src = "int";
        let (mut sess, fid, _cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);

        let specs = DeclSpecs { storage: Some(StorageClass::Typedef), ..DeclSpecs::default() };
        let abstract_decl = Declarator {
            name: None,
            derived: Vec::new(),
            span: rcc_span::DUMMY_SP,
            attrs: Vec::new(),
        };

        let before = parser.scopes.depth();
        declare_declarator_name(&mut parser, &specs, &abstract_decl);
        assert_eq!(parser.scopes.depth(), before, "hook does not push scopes");
        // No typedef registered: cursor-unrelated behavioural assertion.
    }

    #[test]
    fn declare_then_multiple_use_sites_all_see_typedef() {
        // Harbison-Steele Table 4-2 row: once `T` is introduced,
        // every subsequent declaration-specifier slot sees it as a
        // type. Exercises two downstream uses to confirm the table
        // entry is persistent across many lookups, not consumed.
        let src = "typedef unsigned T ; T a ; const T b";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);

        let s1 = parse_decl_specs(&mut parser).expect("typedef specs");
        let d1 = parse_declarator(&mut parser).expect("T declarator");
        declare_declarator_name(&mut parser, &s1, &d1);
        bump_semi(&mut parser);

        let s2 = parse_decl_specs(&mut parser).expect("first use");
        assert!(
            matches!(s2.type_specs.as_slice(), [TypeSpec::TypedefName(_)]),
            "first use sees T as typedef: {:?}",
            s2.type_specs
        );
        let d2 = parse_declarator(&mut parser).expect("a declarator");
        declare_declarator_name(&mut parser, &s2, &d2);
        bump_semi(&mut parser);

        // `const T b`: qualifier before the typedef-name is legal
        // per C99 §6.7p4 — both belong to the specifier run.
        let s3 = parse_decl_specs(&mut parser).expect("second use");
        assert!(s3.quals.const_);
        assert!(
            matches!(s3.type_specs.as_slice(), [TypeSpec::TypedefName(_)]),
            "second use sees T as typedef: {:?}",
            s3.type_specs
        );

        assert!(cap.diagnostics().is_empty(), "clean: {:?}", cap.diagnostics());
    }

    // ── Struct / union bodies (C99 §6.7.2.1) ────────────────────────
    //
    // `parse_decl_specs` dispatches to `parse_record_spec` as soon as
    // it sees `struct` / `union`. The tests below drive the whole
    // specifier-list entry point so they exercise the full path the
    // declaration parser takes when a top-level declarator is
    // followed by a function body.

    /// Parse a specifier list and return the sole record specifier.
    /// Panics if the specs hold anything other than one
    /// `TypeSpec::Record`.
    fn parse_record(src: &str) -> (RecordSpec, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        let rec = match specs.type_specs.as_slice() {
            [TypeSpec::Record(r)] => r.clone(),
            other => panic!("expected single Record spec, got {other:?}"),
        };
        (rec, cap.diagnostics(), sess)
    }

    fn parse_enum(src: &str) -> (EnumSpec, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let specs = parse_decl_specs(&mut parser).expect("parser returns specs");
        let e = match specs.type_specs.as_slice() {
            [TypeSpec::Enum(e)] => e.clone(),
            other => panic!("expected single Enum spec, got {other:?}"),
        };
        (e, cap.diagnostics(), sess)
    }

    #[test]
    fn struct_anonymous_with_fields_parses() {
        // `struct { int a; }` — no tag, body present. Fields = Some.
        let (rec, diags, sess) = parse_record("struct { int a; }");
        assert_eq!(rec.kind, RecordKind::Struct);
        assert!(rec.tag.is_none());
        let fs = rec.fields.as_ref().expect("body defines fields");
        assert_eq!(fs.len(), 1);
        assert!(matches!(fs[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
        let (sym, _) =
            fs[0].declarators[0].declarator.as_ref().unwrap().name.as_ref().expect("named field");
        assert_eq!(sess.interner.get(*sym), "a");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_bare_reference_has_no_fields() {
        // `struct S` — forward reference, no body. Fields = None.
        let (rec, diags, sess) = parse_record("struct S");
        assert_eq!(rec.kind, RecordKind::Struct);
        assert_eq!(sess.interner.get(rec.tag.expect("tag")), "S");
        assert!(rec.fields.is_none(), "no body means fields=None");
        assert!(diags.is_empty());
    }

    #[test]
    fn struct_tagged_with_two_fields_parses() {
        // `struct S { int a; int b; }` — tagged, two separate field
        // decls. Each carries one declarator and no bitfield.
        let (rec, diags, sess) = parse_record("struct S { int a; int b; }");
        let fs = rec.fields.as_ref().expect("body");
        assert_eq!(fs.len(), 2);
        let names: Vec<String> = fs
            .iter()
            .map(|f| {
                let (s, _) = f.declarators[0].declarator.as_ref().unwrap().name.as_ref().unwrap();
                sess.interner.get(*s).to_string()
            })
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert!(fs.iter().all(|f| f.declarators[0].bit_width.is_none()));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_named_bitfield_parses() {
        // `struct S { int x : 3; }` — named 3-bit bitfield. Width is
        // stored as an Expr; constant-folding deferred to HIR.
        let (rec, diags, sess) = parse_record("struct S { int x : 3; }");
        let fs = rec.fields.as_ref().expect("body");
        assert_eq!(fs.len(), 1);
        let fd = &fs[0].declarators[0];
        let (sym, _) = fd.declarator.as_ref().unwrap().name.as_ref().expect("named");
        assert_eq!(sess.interner.get(*sym), "x");
        let w = fd.bit_width.as_ref().expect("bitfield width present");
        assert_eq!(int_lit_text(w, &sess), "3");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_anonymous_bitfield_parses() {
        // `struct S { int : 3; }` — anonymous bitfield: no declarator,
        // only a width. Used in C for manual padding.
        let (rec, diags, sess) = parse_record("struct S { int : 3; }");
        let fs = rec.fields.as_ref().expect("body");
        let fd = &fs[0].declarators[0];
        assert!(fd.declarator.is_none(), "anonymous bitfield has no declarator");
        let w = fd.bit_width.as_ref().expect("width present");
        assert_eq!(int_lit_text(w, &sess), "3");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_flexible_array_member_parses() {
        // `struct S { int x[]; }` — flexible array member (C99
        // §6.7.2.1p16). Parsed as an ordinary field with an empty-
        // size array in the declarator chain; context validation
        // (must be last, must not be alone) is HIR's job.
        let (rec, diags, sess) = parse_record("struct S { int x[]; }");
        let fs = rec.fields.as_ref().expect("body");
        let fd = &fs[0].declarators[0];
        let d = fd.declarator.as_ref().expect("named");
        let (sym, _) = d.name.as_ref().expect("name");
        assert_eq!(sess.interner.get(*sym), "x");
        match d.derived.as_slice() {
            [DerivedDeclarator::Array(a)] => {
                assert!(a.size.is_none(), "flexible array: empty []");
                assert!(!a.has_static && !a.star);
            }
            other => panic!("expected [Array()], got {other:?}"),
        }
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_recursive_self_pointer_parses() {
        // `struct Node { struct Node *next; }` — canonical linked-
        // list shape. The inner `struct Node` is a bare tag reference
        // (no body); the declarator chain is [Pointer]. Name
        // resolution is HIR's concern; the parser only has to keep
        // the two `struct Node`s both as Record specs and distinguish
        // the inner (fields=None) from the outer (fields=Some).
        let (rec, diags, sess) = parse_record("struct Node { struct Node *next; }");
        let fs = rec.fields.as_ref().expect("body");
        assert_eq!(fs.len(), 1);
        match fs[0].specs.type_specs.as_slice() {
            [TypeSpec::Record(inner)] => {
                assert_eq!(inner.kind, RecordKind::Struct);
                let tag = inner.tag.expect("inner tag");
                assert_eq!(sess.interner.get(tag), "Node");
                assert!(inner.fields.is_none(), "inner is a bare reference");
            }
            other => panic!("expected inner Record specifier, got {other:?}"),
        }
        let fd = &fs[0].declarators[0];
        let d = fd.declarator.as_ref().expect("declarator");
        assert!(matches!(d.derived.as_slice(), [DerivedDeclarator::Pointer(_)]));
        let (sym, _) = d.name.as_ref().expect("name");
        assert_eq!(sess.interner.get(*sym), "next");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn union_with_two_fields_parses() {
        // `union U { int i; float f; }` — same shape as struct with
        // kind=Union. Two alternative members, each a simple field.
        let (rec, diags, sess) = parse_record("union U { int i; float f; }");
        assert_eq!(rec.kind, RecordKind::Union);
        let fs = rec.fields.as_ref().expect("body");
        assert_eq!(fs.len(), 2);
        assert!(matches!(fs[0].specs.type_specs.as_slice(), [TypeSpec::Int]));
        assert!(matches!(fs[1].specs.type_specs.as_slice(), [TypeSpec::Float]));
        let (sym_i, _) = fs[0].declarators[0].declarator.as_ref().unwrap().name.as_ref().unwrap();
        let (sym_f, _) = fs[1].declarators[0].declarator.as_ref().unwrap().name.as_ref().unwrap();
        assert_eq!(sess.interner.get(*sym_i), "i");
        assert_eq!(sess.interner.get(*sym_f), "f");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn struct_multi_declarator_field_parses() {
        // `struct S { int a, b; }` — one specifier + two declarators.
        // Both land in the same FieldDecl with declarators.len() == 2.
        let (rec, diags, sess) = parse_record("struct S { int a, b; }");
        let fs = rec.fields.as_ref().expect("body");
        assert_eq!(fs.len(), 1, "single field-decl with two declarators");
        assert_eq!(fs[0].declarators.len(), 2);
        let n0 = fs[0].declarators[0].declarator.as_ref().unwrap().name.as_ref().unwrap().0;
        let n1 = fs[0].declarators[1].declarator.as_ref().unwrap().name.as_ref().unwrap().0;
        assert_eq!(sess.interner.get(n0), "a");
        assert_eq!(sess.interner.get(n1), "b");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    // ── Enum bodies (C99 §6.7.2.2) ─────────────────────────────────

    #[test]
    fn enum_three_implicit_values_parses() {
        // `enum { A, B, C }` — anonymous enum, three enumerators,
        // none with explicit values. Acceptance basic.
        let (e, diags, sess) = parse_enum("enum { A, B, C }");
        assert!(e.tag.is_none());
        let list = e.enumerators.as_ref().expect("body");
        let names: Vec<String> =
            list.iter().map(|en| sess.interner.get(en.name).to_string()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
        assert!(list.iter().all(|en| en.value.is_none()));
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn enum_mixed_explicit_and_implicit_values_parses() {
        // Canonical acceptance shape: `enum { A = 1, B, C = 10 }` —
        // middle enumerator has no explicit value; first and last do.
        let (e, diags, sess) = parse_enum("enum { A = 1, B, C = 10 }");
        let list = e.enumerators.as_ref().expect("body");
        assert_eq!(list.len(), 3);
        let a = &list[0];
        let b = &list[1];
        let c = &list[2];
        assert_eq!(sess.interner.get(a.name), "A");
        assert_eq!(int_lit_text(a.value.as_ref().expect("A=1"), &sess), "1");
        assert_eq!(sess.interner.get(b.name), "B");
        assert!(b.value.is_none(), "B carries no explicit value");
        assert_eq!(sess.interner.get(c.name), "C");
        assert_eq!(int_lit_text(c.value.as_ref().expect("C=10"), &sess), "10");
        assert!(diags.is_empty(), "clean: {diags:?}");
    }

    #[test]
    fn enum_trailing_comma_is_accepted() {
        // `enum { A, B, }` — trailing `,` permitted by §6.7.2.2p1.
        let (e, diags, sess) = parse_enum("enum { A, B, }");
        let list = e.enumerators.as_ref().expect("body");
        assert_eq!(list.len(), 2);
        assert_eq!(sess.interner.get(list[0].name), "A");
        assert_eq!(sess.interner.get(list[1].name), "B");
        assert!(diags.is_empty(), "trailing comma is legal: {diags:?}");
    }

    #[test]
    fn enum_empty_body_errors_e0061() {
        // `enum {}` — empty body is a constraint violation per
        // §6.7.2.2p1 (the enumerator-list is non-empty by grammar).
        let (e, diags, _sess) = parse_enum("enum {}");
        assert!(e.enumerators.as_ref().expect("body").is_empty());
        assert_eq!(codes_of(&diags), vec!["E0061"], "{diags:?}");
    }

    #[test]
    fn enum_bare_reference_has_no_body() {
        // `enum E` — forward reference only. Enumerators = None.
        let (e, diags, sess) = parse_enum("enum E");
        assert_eq!(sess.interner.get(e.tag.expect("tag")), "E");
        assert!(e.enumerators.is_none());
        assert!(diags.is_empty());
    }

    #[test]
    fn enum_tagged_with_body_parses() {
        // `enum E { A, B }` — both tag and body present.
        let (e, diags, sess) = parse_enum("enum E { A, B }");
        assert_eq!(sess.interner.get(e.tag.expect("tag")), "E");
        let list = e.enumerators.as_ref().expect("body");
        assert_eq!(list.len(), 2);
        assert_eq!(sess.interner.get(list[0].name), "A");
        assert_eq!(sess.interner.get(list[1].name), "B");
        assert!(diags.is_empty());
    }

    // ── External declarations / function definitions (§6.9) ─────────

    use rcc_ast::{ExternalDecl, StmtKind};

    fn parse_external(src: &str) -> (ExternalDecl, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let ed = parse_external_decl(&mut parser).expect("parse_external_decl returns Some");
        (ed, cap.diagnostics(), sess)
    }

    #[test]
    fn function_def_int_main_void_return_0() {
        // `int main(void) { return 0; }` → FunctionDef.
        let src = "int main(void) { return 0; }";
        let (ed, diags, sess) = parse_external(src);
        assert!(diags.is_empty(), "clean parse: {diags:?}");
        match ed {
            ExternalDecl::Function(fd) => {
                assert!(matches!(fd.specs.type_specs.as_slice(), [TypeSpec::Int]));
                let (sym, _) = fd.declarator.name.expect("function has a name");
                assert_eq!(sess.interner.get(sym), "main");
                assert!(fd.kr_decls.is_empty());
                assert_eq!(fd.body.items.len(), 1, "body has one item");
            }
            ExternalDecl::Decl(_) => panic!("expected FunctionDef, got Decl"),
        }
    }

    #[test]
    fn prototype_declaration_int_f_int() {
        // `int f(int);` → Decl.
        let src = "int f(int);";
        let (ed, diags, sess) = parse_external(src);
        assert!(diags.is_empty(), "clean parse: {diags:?}");
        match ed {
            ExternalDecl::Decl(decl) => {
                assert!(matches!(decl.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert_eq!(decl.inits.len(), 1);
                let d = &decl.inits[0].declarator;
                let (sym, _) = d.name.expect("declarator has a name");
                assert_eq!(sess.interner.get(sym), "f");
            }
            ExternalDecl::Function(_) => panic!("expected Decl, got FunctionDef"),
        }
    }

    #[test]
    fn parameterless_void_f_void_definition() {
        // `void f(void) { }` → FunctionDef with empty body.
        let src = "void f(void) { }";
        let (ed, diags, sess) = parse_external(src);
        assert!(diags.is_empty(), "clean parse: {diags:?}");
        match ed {
            ExternalDecl::Function(fd) => {
                assert!(matches!(fd.specs.type_specs.as_slice(), [TypeSpec::Void]));
                let (sym, _) = fd.declarator.name.expect("function has a name");
                assert_eq!(sess.interner.get(sym), "f");
                assert!(fd.body.items.is_empty(), "empty body");
            }
            ExternalDecl::Decl(_) => panic!("expected FunctionDef, got Decl"),
        }
    }

    #[test]
    fn variable_declaration_with_init() {
        // `int x = 0;` → Decl with initializer.
        let src = "int x = 0;";
        let (ed, diags, sess) = parse_external(src);
        assert!(diags.is_empty(), "clean parse: {diags:?}");
        match ed {
            ExternalDecl::Decl(decl) => {
                assert!(matches!(decl.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert_eq!(decl.inits.len(), 1);
                let d = &decl.inits[0].declarator;
                let (sym, _) = d.name.expect("declarator has a name");
                assert_eq!(sess.interner.get(sym), "x");
                assert!(decl.inits[0].init.is_some(), "has initializer");
            }
            ExternalDecl::Function(_) => panic!("expected Decl, got FunctionDef"),
        }
    }

    #[test]
    fn function_def_then_variable_decl() {
        // Two external declarations side-by-side parse cleanly.
        let src = "int main(void) { return 0; } int x = 0;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let ed1 = parse_external_decl(&mut parser).expect("first external decl");
        let ed2 = parse_external_decl(&mut parser).expect("second external decl");
        assert!(cap.diagnostics().is_empty(), "clean parse: {:?}", cap.diagnostics());
        assert!(matches!(ed1, ExternalDecl::Function(_)), "first is FunctionDef");
        assert!(matches!(ed2, ExternalDecl::Decl(_)), "second is Decl");
    }

    #[test]
    fn function_def_body_return_stmt() {
        // Verify that `return 0;` inside the body is a Return statement.
        let src = "int main(void) { return 0; }";
        let (ed, diags, _sess) = parse_external(src);
        assert!(diags.is_empty(), "clean parse: {diags:?}");
        match ed {
            ExternalDecl::Function(fd) => {
                assert_eq!(fd.body.items.len(), 1);
                match &fd.body.items[0] {
                    rcc_ast::BlockItem::Stmt(s) => {
                        assert!(
                            matches!(s.kind, StmtKind::Return(Some(_))),
                            "expected Return, got {:?}",
                            s.kind
                        );
                    }
                    other => panic!("expected Stmt, got {other:?}"),
                }
            }
            ExternalDecl::Decl(_) => panic!("expected FunctionDef"),
        }
    }

    // ── K&R-style function definitions (§6.9.1p6) ──────────────────

    #[test]
    fn kr_function_def_parses_with_w0005() {
        // `int f(x, y) int x; double y; { return x; }` → FunctionDef
        // with kr_decls populated and W0005 warning emitted.
        let src = "int f(x, y) int x; double y; { return x; }";
        let (ed, diags, sess) = parse_external(src);
        let warning_codes: Vec<_> = diags
            .iter()
            .filter(|d| d.level == rcc_errors::Level::Warning)
            .filter_map(|d| d.code)
            .collect();
        assert_eq!(warning_codes, vec!["W0005"], "expected W0005: {diags:?}");
        // Check the help text.
        let w = diags.iter().find(|d| d.code == Some("W0005")).unwrap();
        assert!(
            w.help.iter().any(|h| h.contains("prototype")),
            "help should mention prototype syntax: {w:?}"
        );
        match ed {
            ExternalDecl::Function(fd) => {
                let (sym, _) = fd.declarator.name.expect("function has a name");
                assert_eq!(sess.interner.get(sym), "f");
                // kr_decls should have 2 declarations (int x; double y;).
                assert_eq!(fd.kr_decls.len(), 2, "expected 2 K&R decls, got {:?}", fd.kr_decls);
                assert_eq!(fd.body.items.len(), 1, "body has one item");
            }
            ExternalDecl::Decl(_) => panic!("expected FunctionDef, got Decl"),
        }
    }

    #[test]
    fn kr_function_def_single_param() {
        // Single K&R parameter.
        let src = "int g(n) int n; { return n; }";
        let (ed, diags, sess) = parse_external(src);
        assert!(diags.iter().any(|d| d.code == Some("W0005")), "expected W0005: {diags:?}");
        // No errors (only warning).
        assert!(
            !diags.iter().any(|d| d.level == rcc_errors::Level::Error),
            "expected no errors: {diags:?}"
        );
        match ed {
            ExternalDecl::Function(fd) => {
                let (sym, _) = fd.declarator.name.expect("function has a name");
                assert_eq!(sess.interner.get(sym), "g");
                assert_eq!(fd.kr_decls.len(), 1);
            }
            ExternalDecl::Decl(_) => panic!("expected FunctionDef, got Decl"),
        }
    }

    #[test]
    fn kr_unknown_param_emits_e0063() {
        // K&R decl referencing a name NOT in the identifier list → E0063.
        let src = "int f(x) int z; { return 0; }";
        let (ed, diags, _sess) = parse_external(src);
        let error_codes: Vec<_> = diags
            .iter()
            .filter(|d| d.level == rcc_errors::Level::Error)
            .filter_map(|d| d.code)
            .collect();
        assert_eq!(error_codes, vec!["E0063"], "expected E0063: {diags:?}");
        // Should still produce a FunctionDef (recovery).
        assert!(matches!(ed, ExternalDecl::Function(_)));
    }
}
