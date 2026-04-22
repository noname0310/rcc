//! Interned strings: `Symbol` and `Interner`.

use rustc_hash::FxHashMap;

/// Interned string id. Cheap to copy; equality == string equality.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(pub u32);

/// String interner. Not thread-safe; each `Session` owns its own.
#[derive(Debug, Default)]
pub struct Interner {
    names: Vec<String>,
    lookup: FxHashMap<String, Symbol>,
}

impl Interner {
    /// Empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `s`, returning a stable `Symbol`.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.lookup.get(s) {
            return sym;
        }
        let sym = Symbol(self.names.len() as u32);
        self.names.push(s.to_owned());
        self.lookup.insert(s.to_owned(), sym);
        sym
    }

    /// Resolve a `Symbol` back to its string. Panics if out of range.
    pub fn get(&self, sym: Symbol) -> &str {
        &self.names[sym.0 as usize]
    }
}
