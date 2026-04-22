//! `rcc_hir_lower`: AST -> HIR lowering.
//!
//! Analogous to `rustc_ast_lowering`. Responsibilities:
//!
//! 1. Resolve identifiers against three *separate* C name spaces
//!    (ordinary / tag / label).
//! 2. Flatten declarators (`int (*fp[3])(int,int)`) into `Ty`.
//! 3. Expand `typedef` references.
//! 4. Assign `DefId`s and `HirId`s.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_ast::TranslationUnit;
use rcc_data_structures::FxHashMap;
use rcc_hir::{DefId, HirCrate, TyCtxt};
use rcc_session::Session;
use rcc_span::Symbol;

/// Entry point: lower an AST into a fresh `HirCrate`.
///
/// M2 scope: interface only. Implementation in M2-follow-up.
pub fn lower(_ast: &TranslationUnit, _tcx: &mut TyCtxt, _session: &mut Session) -> HirCrate {
    HirCrate::default()
}

/// Per-crate resolution tables built while lowering.
#[derive(Default, Debug)]
pub struct Resolver {
    /// Ordinary namespace: (name) -> `DefId`.
    pub ordinary: FxHashMap<Symbol, DefId>,
    /// Tag namespace: `struct`/`union`/`enum` tags.
    pub tags: FxHashMap<Symbol, DefId>,
    /// Labels are strictly per-function; populated then flushed per body.
    pub labels: FxHashMap<Symbol, rcc_hir::HirStmtId>,
}
