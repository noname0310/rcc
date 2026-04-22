//! Byte positions and spans.

use crate::file::FileId;

/// A byte offset into a single `SourceFile`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytePos(pub u32);

impl BytePos {
    /// Byte position zero.
    pub const ZERO: BytePos = BytePos(0);
}

/// A half-open byte range `[lo, hi)` within a specific source file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    /// Source file the span lives in.
    pub file: FileId,
    /// Inclusive start byte.
    pub lo: BytePos,
    /// Exclusive end byte.
    pub hi: BytePos,
}

impl Span {
    /// Build a new span.
    pub fn new(file: FileId, lo: BytePos, hi: BytePos) -> Self {
        debug_assert!(lo.0 <= hi.0, "span lo <= hi");
        Self { file, lo, hi }
    }

    /// Length of the span in bytes.
    pub fn len(&self) -> u32 {
        self.hi.0 - self.lo.0
    }

    /// Whether the span covers no bytes.
    pub fn is_empty(&self) -> bool {
        self.lo == self.hi
    }

    /// Smallest span covering `self` and `other`. Both spans must belong
    /// to the same file.
    pub fn to(self, other: Span) -> Span {
        debug_assert_eq!(self.file, other.file, "cannot merge spans from different files");
        Span {
            file: self.file,
            lo: BytePos(self.lo.0.min(other.lo.0)),
            hi: BytePos(self.hi.0.max(other.hi.0)),
        }
    }
}

/// A placeholder span for synthetic/compiler-generated nodes.
pub const DUMMY_SP: Span = Span { file: FileId::DUMMY, lo: BytePos::ZERO, hi: BytePos::ZERO };
