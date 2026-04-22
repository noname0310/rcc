//! Macro table, macro definitions, and the *hide set* tracked per expansion.

use rcc_data_structures::{FxHashMap, FxHashSet};
use rcc_lexer::PpToken;
use rcc_span::{Span, Symbol};

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

    /// Iterate every definition.
    pub fn iter(&self) -> impl Iterator<Item = &MacroDef> {
        self.map.values()
    }
}
