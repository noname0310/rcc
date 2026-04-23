//! Macro table, macro definitions, and the *hide set* tracked per expansion.

use rcc_data_structures::{FxHashMap, FxHashSet};
use rcc_errors::{codes::E0022, Diagnostic, Label, Level};
use rcc_lexer::PpToken;
use rcc_span::{Interner, SourceMap, Span, Symbol};

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

    /// Remove a definition. Returns whether it existed.
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

/// Install an object-like `#define` in `macros`, enforcing C99
/// §6.10.3p1's "benign redefinition" rule.
///
/// - No existing definition → insert and return `Ok(())`.
/// - Existing definition whose replacement list is *identical* to the
///   new one → silently accept (no insert required — the entries are
///   interchangeable) and return `Ok(())`.
/// - Existing definition with a *differing* replacement list → return
///   an E0022 diagnostic with primary label on the new definition and
///   a secondary label on the previous one; the table is left
///   unchanged.
///
/// Two replacement lists are identical iff they have the same token
/// count, ordering, spellings (as source slices), and whitespace
/// separation between tokens. Whitespace counts from
/// [`PpToken::leading_ws`](rcc_lexer::PpToken::leading_ws) so that
/// `#define X 1+2` and `#define X 1 + 2` are correctly distinguished.
/// `source_map` provides the text for spelling comparison; `interner`
/// resolves the macro name for diagnostic messages.
pub fn define_object_like(
    def: MacroDef,
    macros: &mut MacroTable,
    source_map: &SourceMap,
    interner: &Interner,
) -> Result<(), Diagnostic> {
    if let Some(existing) = macros.get(def.name) {
        if bodies_equivalent(&existing.body, &def.body, source_map) {
            return Ok(());
        }
        return Err(redefinition_diagnostic(existing, &def, interner));
    }
    macros.define(def);
    Ok(())
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
        MacroDef { name: interner.intern(name_str), kind: MacroKind::ObjectLike, body, def_span }
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
}
