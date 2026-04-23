//! Per-[`FileId`] line overrides installed by `#line` directives.
//!
//! C99 §6.10.4 specifies two forms:
//!
//! ```text
//!     # line  digit-sequence                  new-line
//!     # line  digit-sequence  " s-char-seq "  new-line
//! ```
//!
//! Both "cause the implementation to behave as if the following
//! sequence of source lines begins with a source line that has a line
//! number as specified by the digit sequence" (§6.10.4p2). The
//! second form also "sets the presumed name of the source file"
//! (§6.10.4p3). Neither form affects the actual byte contents of the
//! underlying file — the lexer keeps producing physical positions
//! verbatim — so we only have to remember what `__LINE__` /
//! `__FILE__` should *say* at each physical offset.
//!
//! ## Shape of the map
//!
//! For every real [`FileId`] we keep a `Vec<LineOverride>` sorted by
//! the physical line at which the override starts. Lookup is a
//! `binary_search` followed by a linear offset: if override `k` says
//! "physical line `S` is logical line `L` in file `F`", then for any
//! physical line `P >= S` the effective logical line is
//! `L + (P - S)` and the effective file is `F`. Physical lines
//! before the first override (or any line in a file with no
//! overrides) fall through to the real `SourceMap` values.
//!
//! Directives that only renumber (`#line N`, no filename) inherit
//! the previous override's file id — or the real file id if no
//! override has run yet — so `__FILE__` only changes at an explicit
//! second form.
//!
//! ## Virtual files
//!
//! The spec allows `#line N "name"` to drop us into a file the
//! `SourceMap` has never heard of (e.g. generated code whose
//! pre-generation source is what the author cares about). We create
//! a virtual [`rcc_span::SourceFile`] with empty contents whose
//! `name` is the override spelling; its `FileId` is what we store
//! in the override so `__FILE__` expansion can dereference it the
//! same way it does for a real file. No tokens ever originate from
//! these virtual files, so the empty `src` never gets indexed.

use rcc_data_structures::FxHashMap;
use rcc_span::{BytePos, FileId, SourceMap};

/// One `#line` override installed at a particular physical line.
#[derive(Copy, Clone, Debug)]
pub struct LineOverride {
    /// Physical line (1-based, as produced by
    /// [`SourceMap::lookup_line_col`]) in the *real* file at which
    /// this override first takes effect. Always the line *after* the
    /// directive itself (§6.10.4p2).
    pub start_physical_line: u32,
    /// Logical line number reported at `start_physical_line`.
    pub logical_line: u32,
    /// File id to report for `__FILE__` at/after `start_physical_line`.
    /// Either a freshly synthesised virtual `SourceFile` (if the
    /// directive named a filename) or an inherited id from the
    /// previous override / the real file.
    pub file_id: FileId,
}

/// Registry of `#line` overrides per real `FileId`.
#[derive(Debug, Default)]
pub struct LineMap {
    entries: FxHashMap<FileId, Vec<LineOverride>>,
}

impl LineMap {
    /// Build an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a new override to the given file's history. The caller
    /// is responsible for producing overrides in monotonic
    /// `start_physical_line` order — the map does not resort.
    pub fn push(&mut self, file: FileId, ov: LineOverride) {
        self.entries.entry(file).or_default().push(ov);
    }

    /// Return the active override for `(file, physical_line)`, if any.
    /// "Active" means the greatest-`start_physical_line` entry with
    /// `start_physical_line <= physical_line`.
    pub fn active(&self, file: FileId, physical_line: u32) -> Option<&LineOverride> {
        let v = self.entries.get(&file)?;
        // `partition_point` gives the number of entries with start <=
        // physical_line; subtract one for the index. No match means
        // the very first entry is still in the future.
        let n = v.partition_point(|ov| ov.start_physical_line <= physical_line);
        if n == 0 {
            None
        } else {
            Some(&v[n - 1])
        }
    }

    /// Resolve the effective `__LINE__` value for a source position.
    /// Falls back to the raw `SourceMap` line when no override
    /// applies.
    pub fn effective_line(&self, sm: &SourceMap, file: FileId, pos: BytePos) -> u32 {
        let physical = sm.lookup_line_col(file, pos).line;
        match self.active(file, physical) {
            Some(ov) => ov.logical_line.saturating_add(physical - ov.start_physical_line),
            None => physical,
        }
    }

    /// Resolve the effective `__FILE__` file id for a source position.
    /// Falls back to the argument `file` when no override applies.
    pub fn effective_file(&self, sm: &SourceMap, file: FileId, pos: BytePos) -> FileId {
        let physical = sm.lookup_line_col(file, pos).line;
        match self.active(file, physical) {
            Some(ov) => ov.file_id,
            None => file,
        }
    }

    /// Return the file id that `__FILE__` should currently name for
    /// `file`, assuming we are about to install a new override at
    /// `physical_line` without a filename of its own. This is the
    /// "inherited file" used to fill [`LineOverride::file_id`] when
    /// the directive is the bare `#line N` form.
    pub fn inherited_file(&self, file: FileId, physical_line: u32) -> FileId {
        self.active(file, physical_line).map(|ov| ov.file_id).unwrap_or(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn sm_with(src: &str) -> (SourceMap, FileId) {
        let mut sm = SourceMap::new();
        let id = sm.add_file(PathBuf::from("<unit>"), Arc::from(src));
        (sm, id)
    }

    #[test]
    fn empty_map_returns_physical_line_and_file() {
        let (sm, id) = sm_with("a\nb\nc\n");
        let map = LineMap::new();
        // Position at the start of line 2.
        let pos = BytePos(2);
        assert_eq!(map.effective_line(&sm, id, pos), 2);
        assert_eq!(map.effective_file(&sm, id, pos), id);
    }

    #[test]
    fn override_renumbers_from_its_start_line() {
        // File has four physical lines. The override kicks in on
        // physical line 3 and renumbers it to 100.
        let (sm, id) = sm_with("a\nb\nc\nd\n");
        let mut map = LineMap::new();
        map.push(id, LineOverride { start_physical_line: 3, logical_line: 100, file_id: id });
        // Line 1 and 2 fall through.
        assert_eq!(map.effective_line(&sm, id, BytePos(0)), 1);
        assert_eq!(map.effective_line(&sm, id, BytePos(2)), 2);
        // Line 3 is renumbered to 100; line 4 picks up the +1 step.
        assert_eq!(map.effective_line(&sm, id, BytePos(4)), 100);
        assert_eq!(map.effective_line(&sm, id, BytePos(6)), 101);
    }

    #[test]
    fn second_override_wins_after_its_start_line() {
        let (sm, id) = sm_with("a\nb\nc\nd\ne\n");
        let mut map = LineMap::new();
        map.push(id, LineOverride { start_physical_line: 2, logical_line: 100, file_id: id });
        map.push(id, LineOverride { start_physical_line: 4, logical_line: 50, file_id: id });
        assert_eq!(map.effective_line(&sm, id, BytePos(2)), 100); // line 2
        assert_eq!(map.effective_line(&sm, id, BytePos(4)), 101); // line 3
        assert_eq!(map.effective_line(&sm, id, BytePos(6)), 50); // line 4
        assert_eq!(map.effective_line(&sm, id, BytePos(8)), 51); // line 5
    }

    #[test]
    fn inherited_file_returns_previous_override_or_real_file() {
        let mut sm = SourceMap::new();
        let real = sm.add_file(PathBuf::from("real.c"), Arc::from("a\nb\nc\n"));
        let virt = sm.add_file(PathBuf::from("virt.c"), Arc::from(""));
        let mut map = LineMap::new();
        // No override yet: inherited file is the real file.
        assert_eq!(map.inherited_file(real, 2), real);
        map.push(real, LineOverride { start_physical_line: 2, logical_line: 1, file_id: virt });
        // Installed override's file id is what a later bare `#line N`
        // should inherit.
        assert_eq!(map.inherited_file(real, 4), virt);
    }
}
