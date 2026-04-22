//! Scoped name classification table used to resolve the typedef-name
//! vs ordinary-identifier ambiguity during parsing.

use rcc_data_structures::FxHashMap;
use rcc_span::Symbol;

/// Classification of a name at a given scope.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NameKind {
    /// An ordinary identifier (object, function, enumerator, ...).
    Ordinary,
    /// A `typedef-name`.
    Typedef,
}

/// A single lexical scope.
#[derive(Default, Debug)]
pub struct Scope {
    /// Classification of each locally declared name.
    pub names: FxHashMap<Symbol, NameKind>,
}

/// Stack of nested scopes. Innermost last.
#[derive(Default, Debug)]
pub struct ScopeStack {
    frames: Vec<Scope>,
}

impl ScopeStack {
    /// Start with the single file-scope frame.
    pub fn new() -> Self {
        Self { frames: vec![Scope::default()] }
    }

    /// Push a new scope.
    pub fn push(&mut self) {
        self.frames.push(Scope::default());
    }

    /// Pop the innermost scope.
    pub fn pop(&mut self) {
        self.frames.pop();
    }

    /// Record `sym` as `kind` in the innermost scope.
    pub fn declare(&mut self, sym: Symbol, kind: NameKind) {
        self.frames.last_mut().expect("no scope").names.insert(sym, kind);
    }

    /// Resolve a name through the scope chain (innermost first).
    pub fn lookup(&self, sym: Symbol) -> Option<NameKind> {
        for frame in self.frames.iter().rev() {
            if let Some(&k) = frame.names.get(&sym) {
                return Some(k);
            }
        }
        None
    }

    /// Is `sym` currently a `typedef-name`?
    pub fn is_typedef(&self, sym: Symbol) -> bool {
        matches!(self.lookup(sym), Some(NameKind::Typedef))
    }
}
