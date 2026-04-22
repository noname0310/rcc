//! `rcc_span`: source positions, spans, source map, and symbol interner.
//!
//! Analogous to `rustc_span`. All downstream crates depend on this for
//! diagnostics and AST/HIR node provenance.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod file;
mod pos;
mod source_map;
mod symbol;

pub use file::{FileId, SourceFile};
pub use pos::{BytePos, Span, DUMMY_SP};
pub use source_map::{LineCol, SourceMap};
pub use symbol::{Interner, Symbol};
