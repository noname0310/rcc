//! Source file identity and contents.

use std::path::PathBuf;
use std::sync::Arc;

/// Opaque identifier for a `SourceFile` in the `SourceMap`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

impl FileId {
    /// Sentinel id used by `DUMMY_SP`. Never refers to a real file.
    pub const DUMMY: FileId = FileId(u32::MAX);
}

/// A single source file owned by the `SourceMap`.
#[derive(Debug)]
pub struct SourceFile {
    /// Id inside the source map.
    pub id: FileId,
    /// Logical file name (may be synthetic, e.g. `<stdin>`).
    pub name: PathBuf,
    /// File contents.
    pub src: Arc<str>,
    /// Cumulative byte offsets of every line start; `line_starts[0] == 0`.
    pub line_starts: Vec<u32>,
}

impl SourceFile {
    /// Build a new source file and precompute line starts.
    pub fn new(id: FileId, name: PathBuf, src: Arc<str>) -> Self {
        let mut line_starts = Vec::with_capacity(64);
        line_starts.push(0);
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                let next = (i as u32).checked_add(1).expect("source file too large");
                line_starts.push(next);
            }
        }
        Self { id, name, src, line_starts }
    }
}
