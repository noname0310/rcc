//! `rcc_data_structures`: shared data structures across the compiler.
//!
//! Analogous to `rustc_data_structures`. Keeps a single source of truth for
//! the hasher, index-vec, and arena abstractions used by every other crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

pub mod idx;

pub use idx::{Idx, IndexVec};
