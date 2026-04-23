//! Macro table, macro definitions, and the *hide set* tracked per expansion.

use rcc_data_structures::{FxHashMap, FxHashSet};

/// Canonical spelling of the variadic pseudo-parameter.
///
/// C99 §6.10.3p5 reserves the identifier `__VA_ARGS__` for use as a
/// stand-in for the trailing comma-separated arguments of a variadic
/// function-like macro. The expander interns this string once per
/// run and compares body identifiers against the resulting
/// [`rcc_span::Symbol`] to decide whether to substitute the variadic
/// slot or (outside a variadic body) emit E0026.
pub const VA_ARGS_NAME: &str = "__VA_ARGS__";
use rcc_errors::{
    codes::{E0022, E0027},
    Diagnostic, Label, Level,
};
use rcc_lexer::PpToken;
use rcc_span::{Interner, SourceMap, Span, Symbol};

/// Dynamic built-in macros whose replacement depends on the use site
/// (C99 §6.10.8p1). Static predefined macros (`__STDC__`,
/// `__STDC_VERSION__`, `__STDC_HOSTED__`, `__DATE__`, `__TIME__`) are
/// modelled as ordinary [`MacroKind::ObjectLike`] definitions installed
/// at preprocessor start-up — their replacement lists are frozen at
/// that moment and thereafter expanded like any user `#define`. Only
/// use-site-varying ones need a sentinel kind.
///
/// `__func__` is explicitly **not** in this enum: it is not a macro at
/// all but a C99 §6.4.2.2 predeclared identifier, materialised by the
/// parser (phase 7) when entering each function-definition body. The
/// preprocessor treats `__func__` as any other identifier.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BuiltinMacro {
    /// `__FILE__` — expands to a narrow string literal naming the
    /// current source file (C99 §6.10.8p1).
    File,
    /// `__LINE__` — expands to a decimal pp-number giving the current
    /// presumed physical line number (C99 §6.10.8p1).
    Line,
}

/// Object-like vs function-like distinction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacroKind {
    /// `#define NAME replacement`
    ObjectLike,
    /// `#define NAME(params) replacement`
    FunctionLike {
        /// Formal parameter names (in declaration order).
        params: Vec<Symbol>,
        /// Whether the parameter list ends with `...`.
        variadic: bool,
    },
    /// A dynamic built-in whose replacement is synthesised by
    /// [`crate::expand::Expander`] at every use site. The empty `body`
    /// on such a [`MacroDef`] is ignored by the expander.
    Builtin(BuiltinMacro),
}

/// A single macro definition.
#[derive(Clone, Debug)]
pub struct MacroDef {
    /// Macro name.
    pub name: Symbol,
    /// Object-like vs function-like.
    pub kind: MacroKind,
    /// Replacement-list tokens.
    pub body: Vec<PpToken>,
    /// Where it was defined.
    pub def_span: Span,
    /// True for C99 §6.10.8 predefined macros (and any implementation-
    /// supplied ones); such entries are protected from user `#define`
    /// or `#undef` redefinition per §6.10.8p2 — attempting either is
    /// diagnosed as E0027. CLI `-D` flags and user `#define`s install
    /// entries with this flag cleared.
    pub is_predefined: bool,
}

/// Per-expansion set of macro names that must not be re-expanded; this is
/// the classical Prosser *hide set*.
pub type HideSet = FxHashSet<Symbol>;

/// Name -> definition table. Later passes (conditional `#undef`) may remove entries.
#[derive(Default, Debug)]
pub struct MacroTable {
    map: FxHashMap<Symbol, MacroDef>,
}

impl MacroTable {
    /// Define or redefine a macro. C99 §6.10.3p2: redefinition must match.
    pub fn define(&mut self, def: MacroDef) {
        self.map.insert(def.name, def);
    }

    /// Remove a definition. Returns whether it existed. Callers that
    /// must honour the C99 §6.10.8p2 "no `#undef` of a predefined
    /// macro" rule should go through [`undef_user`] instead.
    pub fn undef(&mut self, name: Symbol) -> bool {
        self.map.remove(&name).is_some()
    }

    /// Look up a definition.
    pub fn get(&self, name: Symbol) -> Option<&MacroDef> {
        self.map.get(&name)
    }

    /// Whether a macro with this name is currently defined.
    ///
    /// Used by the include-guard fast-path (task 04-04): on a repeat
    /// `#include`, the preprocessor checks `is_defined(guard_sym)` to
    /// decide whether the body would expand to nothing under `#ifndef
    /// guard_sym`. Once task 04-06 wires real `#define` processing
    /// this becomes the authoritative predicate for `#ifdef`/`#ifndef`
    /// as well.
    pub fn is_defined(&self, name: Symbol) -> bool {
        self.map.contains_key(&name)
    }

    /// Iterate every definition.
    pub fn iter(&self) -> impl Iterator<Item = &MacroDef> {
        self.map.values()
    }
}

/// Install a `#define` (object-like **or** function-like) in
/// `macros`, enforcing C99 §6.10.3p1's "benign redefinition" rule.
///
/// - No existing definition → insert and return `Ok(())`.
/// - Existing definition that is *identical* to the new one →
///   silently accept (no insert required — the entries are
///   interchangeable) and return `Ok(())`.
/// - Existing definition that *differs* from the new one → return an
///   E0022 diagnostic with primary label on the new definition and a
///   secondary label on the previous one; the table is left
///   unchanged.
///
/// Two definitions are identical iff their
/// [`MacroKind`] values match exactly (object-like vs function-like;
/// same parameter names in the same order; same variadicity) **and**
/// their replacement lists have the same token count, ordering,
/// spellings (as source slices), and whitespace separation between
/// tokens. Whitespace counts from
/// [`PpToken::leading_ws`](rcc_lexer::PpToken::leading_ws) so that
/// `#define X 1+2` and `#define X 1 + 2` are correctly distinguished.
/// `source_map` provides the text for spelling comparison; `interner`
/// resolves the macro name for diagnostic messages.
pub fn define_macro(
    def: MacroDef,
    macros: &mut MacroTable,
    source_map: &SourceMap,
    interner: &Interner,
) -> Result<(), Diagnostic> {
    if let Some(existing) = macros.get(def.name) {
        if existing.is_predefined {
            return Err(predefined_tamper_diagnostic(
                existing,
                def.def_span,
                def.name,
                interner,
                PredefinedOp::Define,
            ));
        }
        if definitions_equivalent(existing, &def, source_map) {
            return Ok(());
        }
        return Err(redefinition_diagnostic(existing, &def, interner));
    }
    macros.define(def);
    Ok(())
}

/// Which directive is tampering with a predefined macro.
#[derive(Copy, Clone, Debug)]
enum PredefinedOp {
    Define,
    Undef,
}

/// Remove a user-defined macro, honouring C99 §6.10.8p2: predefined
/// macros (those with [`MacroDef::is_predefined`] set) may not be the
/// subject of `#undef` and yield E0027 if attempted. Returns
/// `Ok(true)` when the entry existed and was removed, `Ok(false)` for
/// `#undef` of a name that is not currently defined (which is
/// explicitly legal per §6.10.5p2).
pub fn undef_user(
    name: Symbol,
    directive_span: Span,
    macros: &mut MacroTable,
    interner: &Interner,
) -> Result<bool, Diagnostic> {
    if let Some(existing) = macros.get(name) {
        if existing.is_predefined {
            return Err(predefined_tamper_diagnostic(
                existing,
                directive_span,
                name,
                interner,
                PredefinedOp::Undef,
            ));
        }
    }
    Ok(macros.undef(name))
}

/// Back-compat alias retained while call sites migrate. Accepts any
/// [`MacroDef`] (object-like or function-like); new code should call
/// [`define_macro`] directly.
#[doc(hidden)]
pub fn define_object_like(
    def: MacroDef,
    macros: &mut MacroTable,
    source_map: &SourceMap,
    interner: &Interner,
) -> Result<(), Diagnostic> {
    define_macro(def, macros, source_map, interner)
}

/// Compare two definitions for C99 §6.10.3p1 "identical" status:
/// matching [`MacroKind`] (object-like vs function-like, including
/// parameter names in order and variadicity) plus identical
/// replacement lists per [`bodies_equivalent`].
fn definitions_equivalent(a: &MacroDef, b: &MacroDef, source_map: &SourceMap) -> bool {
    if a.kind != b.kind {
        return false;
    }
    bodies_equivalent(&a.body, &b.body, source_map)
}

/// Compare two replacement-lists for C99 §6.10.3p1 "identical" status.
///
/// Token kinds, source spellings, and leading-whitespace flags are all
/// compared pairwise; whitespace *quantity* is irrelevant (the lexer
/// already collapses horizontal whitespace into a single `leading_ws`
/// bit per token).
fn bodies_equivalent(a: &[PpToken], b: &[PpToken], source_map: &SourceMap) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        if x.kind != y.kind {
            return false;
        }
        if x.leading_ws != y.leading_ws {
            return false;
        }
        if token_text(x, source_map) != token_text(y, source_map) {
            return false;
        }
    }
    true
}

fn token_text<'a>(tok: &PpToken, source_map: &'a SourceMap) -> &'a str {
    let src = &source_map.file(tok.span.file).src;
    &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]
}

fn predefined_tamper_diagnostic(
    existing: &MacroDef,
    directive_span: Span,
    name: Symbol,
    interner: &Interner,
    op: PredefinedOp,
) -> Diagnostic {
    let name_str = interner.get(name);
    let (message, primary_label) = match op {
        PredefinedOp::Define => (
            format!("cannot redefine predefined macro `{name_str}`"),
            "redefinition attempted here",
        ),
        PredefinedOp::Undef => {
            (format!("cannot `#undef` predefined macro `{name_str}`"), "`#undef` attempted here")
        }
    };
    Diagnostic {
        level: Level::Error,
        code: Some(E0027),
        message,
        labels: vec![
            Label { span: directive_span, message: primary_label.into(), primary: true },
            Label { span: existing.def_span, message: "predefined here".into(), primary: false },
        ],
        notes: vec!["C99 §6.10.8p2: the predefined macros listed in §6.10.8 \
             shall not be the subject of a `#define` or `#undef` \
             preprocessing directive"
            .into()],
        help: vec!["rename your macro so it does not collide with a \
             predefined identifier"
            .into()],
    }
}

fn redefinition_diagnostic(
    previous: &MacroDef,
    current: &MacroDef,
    interner: &Interner,
) -> Diagnostic {
    let name = interner.get(current.name);
    Diagnostic {
        level: Level::Error,
        code: Some(E0022),
        message: format!("macro `{name}` redefined with a different body"),
        labels: vec![
            Label { span: current.def_span, message: "new definition here".into(), primary: true },
            Label {
                span: previous.def_span,
                message: "previous definition here".into(),
                primary: false,
            },
        ],
        notes: vec!["C99 §6.10.3p1 requires the replacement list to be \
             identical in token count, ordering, spelling, and \
             whitespace separation"
            .into()],
        help: vec!["use `#undef` before redefining with a different body".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_lexer::tokenize;
    use rcc_span::BytePos;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Tokenise the replacement list *only* from a `#define NAME <body>`
    /// source — i.e. everything after `NAME ` up to (but excluding) the
    /// trailing newline, in that file's `SourceMap` entry.
    ///
    /// Returns `(source_map, FileId, name_symbol, body_tokens,
    /// def_span)` ready to feed to [`define_object_like`].
    fn seed_define(
        source_map: &mut SourceMap,
        interner: &mut Interner,
        name_str: &str,
        src: &str,
    ) -> MacroDef {
        let id = source_map.add_file(PathBuf::from(format!("<{name_str}>")), Arc::from(src));
        let tokens: Vec<PpToken> = tokenize(id, src).collect();
        // Skip `#`, `define`, NAME; stop at the first `Newline`.
        let mut body = Vec::new();
        let mut iter = tokens.into_iter();
        // Drop `#`
        iter.next();
        // Drop `define`
        iter.next();
        // Drop NAME
        iter.next();
        for tok in iter {
            if tok.kind == rcc_lexer::PpTokenKind::Newline {
                break;
            }
            body.push(tok);
        }
        let file_len = src.len() as u32;
        let def_span = Span::new(id, BytePos(0), BytePos(file_len));
        MacroDef {
            name: interner.intern(name_str),
            kind: MacroKind::ObjectLike,
            body,
            def_span,
            is_predefined: false,
        }
    }

    #[test]
    fn first_definition_is_inserted() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();
        let def = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        let sym = interner.intern("FOO");

        define_object_like(def, &mut macros, &sm, &interner).expect("fresh define must succeed");

        let stored = macros.get(sym).expect("FOO must be registered");
        assert!(matches!(stored.kind, MacroKind::ObjectLike));
        assert_eq!(stored.body.len(), 1, "`42` is one pp-number");
        assert_eq!(token_text(&stored.body[0], &sm), "42");
    }

    #[test]
    fn redefinition_with_identical_body_is_benign() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let first = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        define_object_like(first, &mut macros, &sm, &interner).unwrap();

        let second = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        define_object_like(second, &mut macros, &sm, &interner)
            .expect("identical replacement-list must be accepted");
    }

    #[test]
    fn redefinition_with_different_body_is_e0022() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let first = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        let first_span = first.def_span;
        define_object_like(first, &mut macros, &sm, &interner).unwrap();

        let second = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 43\n");
        let second_span = second.def_span;
        let diag = define_object_like(second, &mut macros, &sm, &interner)
            .expect_err("different body must be rejected");

        assert_eq!(diag.code, Some(E0022));
        assert_eq!(diag.labels.len(), 2, "both defs must be labelled");
        let primary =
            diag.labels.iter().find(|l| l.primary).expect("new definition is the primary label");
        let secondary = diag
            .labels
            .iter()
            .find(|l| !l.primary)
            .expect("previous definition is a secondary label");
        assert_eq!(primary.span, second_span, "primary must point at the redefinition");
        assert_eq!(secondary.span, first_span, "secondary must point at the original");
    }

    #[test]
    fn redefinition_with_whitespace_only_difference_is_diagnosed() {
        // `1+2` vs `1 + 2` differ in whitespace separation per §6.10.3p1.
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let a = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 1+2\n");
        define_object_like(a, &mut macros, &sm, &interner).unwrap();

        let b = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 1 + 2\n");
        let diag = define_object_like(b, &mut macros, &sm, &interner)
            .expect_err("whitespace-separation difference must be rejected");
        assert_eq!(diag.code, Some(E0022));
    }

    #[test]
    fn undef_removes_definition() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let def = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        let sym = def.name;
        define_object_like(def, &mut macros, &sm, &interner).unwrap();
        assert!(macros.is_defined(sym));

        assert!(macros.undef(sym), "undef of a defined macro must return true");
        assert!(!macros.is_defined(sym));
    }

    #[test]
    fn undef_unknown_name_is_not_an_error() {
        // C99 §6.10.5p2: `#undef` of a name that is not currently
        // defined has no effect and is NOT a diagnosable error. The
        // `bool` return is a convenience signal for callers, not an
        // error flag.
        let mut macros = MacroTable::default();
        let mut interner = Interner::new();
        let sym = interner.intern("NEVER_DEFINED");
        assert!(!macros.undef(sym), "undef of an undefined macro returns false silently");
    }

    #[test]
    fn undef_then_redefine_with_different_body_is_ok() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let a = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        let sym = a.name;
        define_object_like(a, &mut macros, &sm, &interner).unwrap();
        macros.undef(sym);

        let b = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 43\n");
        define_object_like(b, &mut macros, &sm, &interner)
            .expect("after #undef, any body is fresh again");
        assert_eq!(token_text(&macros.get(sym).unwrap().body[0], &sm), "43");
    }

    // ── Function-like benign-redefinition tests (task 04-07) ────────

    /// Build a function-like [`MacroDef`] directly without going
    /// through the directive parser; used by the tests below to keep
    /// the coverage of [`define_macro`] self-contained.
    fn seed_fn_define(
        source_map: &mut SourceMap,
        interner: &mut Interner,
        name_str: &str,
        params: &[&str],
        variadic: bool,
        src: &str,
    ) -> MacroDef {
        let id = source_map.add_file(PathBuf::from(format!("<{name_str}>")), Arc::from(src));
        let tokens: Vec<PpToken> = tokenize(id, src).collect();
        // Tokens after `#`, `define`, NAME, `(`, params…, `)` up to newline.
        let mut body = Vec::new();
        let mut iter = tokens.into_iter().peekable();
        // `#`, `define`, NAME
        iter.next();
        iter.next();
        iter.next();
        // `(` and everything up to matching `)`
        assert!(
            matches!(iter.next(), Some(t) if matches!(t.kind, rcc_lexer::PpTokenKind::Punct(rcc_lexer::Punct::LParen)))
        );
        for tok in iter.by_ref() {
            if matches!(tok.kind, rcc_lexer::PpTokenKind::Punct(rcc_lexer::Punct::RParen)) {
                break;
            }
        }
        for tok in iter {
            if tok.kind == rcc_lexer::PpTokenKind::Newline {
                break;
            }
            body.push(tok);
        }
        let file_len = src.len() as u32;
        let def_span = Span::new(id, BytePos(0), BytePos(file_len));
        let param_syms = params.iter().map(|p| interner.intern(p)).collect();
        MacroDef {
            name: interner.intern(name_str),
            kind: MacroKind::FunctionLike { params: param_syms, variadic },
            body,
            def_span,
            is_predefined: false,
        }
    }

    #[test]
    fn function_like_benign_redefinition_is_silent() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let a = seed_fn_define(
            &mut sm,
            &mut interner,
            "MAX",
            &["a", "b"],
            false,
            "#define MAX(a,b) ((a)>(b)?(a):(b))\n",
        );
        define_macro(a, &mut macros, &sm, &interner).unwrap();

        let b = seed_fn_define(
            &mut sm,
            &mut interner,
            "MAX",
            &["a", "b"],
            false,
            "#define MAX(a,b) ((a)>(b)?(a):(b))\n",
        );
        define_macro(b, &mut macros, &sm, &interner)
            .expect("identical function-like redefinition must be benign");
    }

    #[test]
    fn function_like_redefinition_with_different_param_name_is_e0022() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let a = seed_fn_define(
            &mut sm,
            &mut interner,
            "MAX",
            &["a", "b"],
            false,
            "#define MAX(a,b) ((a)>(b)?(a):(b))\n",
        );
        define_macro(a, &mut macros, &sm, &interner).unwrap();

        // Rename parameter but keep an equivalent-looking body; the
        // §6.10.3p1 comparison demands identical parameter *names*.
        let b = seed_fn_define(
            &mut sm,
            &mut interner,
            "MAX",
            &["x", "b"],
            false,
            "#define MAX(x,b) ((x)>(b)?(x):(b))\n",
        );
        let diag = define_macro(b, &mut macros, &sm, &interner)
            .expect_err("parameter-name change must be rejected");
        assert_eq!(diag.code, Some(E0022));
    }

    #[test]
    fn function_like_vs_object_like_same_name_is_e0022() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        // Object-like FOO.
        let a = seed_define(&mut sm, &mut interner, "FOO", "#define FOO 42\n");
        define_macro(a, &mut macros, &sm, &interner).unwrap();

        // Function-like FOO with (allegedly) the same body. Kind
        // mismatch alone must trigger E0022.
        let b = seed_fn_define(&mut sm, &mut interner, "FOO", &[], false, "#define FOO() 42\n");
        let diag = define_macro(b, &mut macros, &sm, &interner)
            .expect_err("object-like vs function-like mismatch must be rejected");
        assert_eq!(diag.code, Some(E0022));
    }

    #[test]
    fn function_like_variadic_mismatch_is_e0022() {
        let mut sm = SourceMap::new();
        let mut interner = Interner::new();
        let mut macros = MacroTable::default();

        let a = seed_fn_define(&mut sm, &mut interner, "V", &[], false, "#define V() 0\n");
        define_macro(a, &mut macros, &sm, &interner).unwrap();

        let b = seed_fn_define(&mut sm, &mut interner, "V", &[], true, "#define V(...) 0\n");
        let diag = define_macro(b, &mut macros, &sm, &interner)
            .expect_err("variadic-ness mismatch must be rejected");
        assert_eq!(diag.code, Some(E0022));
    }
}
