//! ABI / type layout shared with CFG lowering.
//!
//! LLVM codegen intentionally reuses `rcc_hir::LayoutCx` so that
//! `sizeof`, CFG lowering, and backend object layout cannot silently drift.

pub use rcc_hir::{Layout, LayoutCx, LayoutError, LayoutResult};

/// Pointer layout for the baseline LP64 / SysV x86-64 target.
///
/// The size and ABI alignment are in bytes and must stay in sync with
/// the LLVM module data layout installed by `backend::CodegenCx`.
pub const BASELINE_POINTER_LAYOUT: Layout = Layout { size: 8, align: 8 };
