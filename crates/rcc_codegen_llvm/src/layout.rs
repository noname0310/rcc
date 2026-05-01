//! ABI / type layout shared with CFG lowering.
//!
//! LLVM codegen intentionally reuses `rcc_hir::LayoutCx` so that
//! `sizeof`, CFG lowering, and backend object layout cannot silently drift.

pub use rcc_hir::{LayoutCx, LayoutError, LayoutResult};
