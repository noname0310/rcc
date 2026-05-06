//! Source map: registry of source files + position → (file, line, col) lookup.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::file::{FileId, SourceFile};
use crate::pos::BytePos;

/// 1-based (line, column) pair. Column is a byte offset within the line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LineCol {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column (bytes from start of line + 1).
    pub col: u32,
}

/// Collection of all `SourceFile`s loaded during a compilation.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Create an empty source map.
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Register a new in-memory source file and return its id.
    pub fn add_file(&mut self, name: PathBuf, src: Arc<str>) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(id, name, src));
        id
    }

    /// Load a file from disk.
    pub fn load_file(&mut self, path: &Path) -> std::io::Result<FileId> {
        let bytes = std::fs::read(path)?;
        let src: Arc<str> = match String::from_utf8(bytes) {
            Ok(src) => Arc::from(src),
            Err(err) => {
                // C source files are byte streams.  Real-world generated
                // headers can contain high-bit bytes in comments or string
                // tables; keep loading them with a Latin-1 byte mapping so
                // lexing/preprocessing can continue instead of rejecting the
                // file before phase 1.
                Arc::from(err.into_bytes().into_iter().map(char::from).collect::<String>())
            }
        };
        Ok(self.add_file(path.to_path_buf(), src))
    }

    /// Look up a source file by id.
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    /// Iterate every registered source file.
    pub fn files(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }

    /// Resolve a byte position to a 1-based (line, column) pair.
    pub fn lookup_line_col(&self, file: FileId, pos: BytePos) -> LineCol {
        let f = self.file(file);
        let p = pos.0;
        // Binary search for the greatest line_start <= p.
        let line_idx = match f.line_starts.binary_search(&p) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = f.line_starts[line_idx];
        LineCol { line: (line_idx as u32) + 1, col: p - line_start + 1 }
    }
}
