//! `rcc_preprocess`: C preprocessor.
//!
//! Implements C99 translation phases 1–4: line splicing, pp-tokenisation
//! (delegated to `rcc_lexer`), macro expansion, and directive handling.
//! Output is a "clean" pp-token stream consumed by `rcc_parse`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_data_structures::FxHashMap;
use rcc_lexer::PpToken;
use rcc_session::Session;
use rcc_span::{FileId, Symbol};

pub mod directive;
pub mod guard;
pub mod include;
pub mod line_stream;
pub mod macros;

pub use directive::{parse_directive, ConditionalKind, Directive};
pub use guard::detect_guard;
pub use include::{detect_pragma_once, resolve_header, strip_header_delimiters};
pub use line_stream::LineStream;
pub use macros::{HideSet, MacroDef, MacroKind, MacroTable};

/// Entry point: preprocess the file `root` in `session` and return the
/// expanded pp-token stream that `rcc_parse` should consume.
pub fn preprocess(session: &mut Session, root: FileId) -> Vec<PpToken> {
    Preprocessor::new(session).run(root)
}

/// Stateful preprocessor. One per compilation unit.
pub struct Preprocessor<'a> {
    /// Compilation session (source map, diagnostics, options).
    pub session: &'a mut Session,
    /// All known macros.
    pub macros: MacroTable,
    /// Include-guard cache: file -> guard symbol.
    pub include_guards: FxHashMap<FileId, Symbol>,
    /// Files that should be processed at most once (`#pragma once`).
    pub pragma_once: FxHashMap<FileId, ()>,
}

impl<'a> Preprocessor<'a> {
    /// Build a new preprocessor.
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            macros: MacroTable::default(),
            include_guards: FxHashMap::default(),
            pragma_once: FxHashMap::default(),
        }
    }

    /// Run preprocessing and return the expanded token stream.
    ///
    /// M1 scope: pass-through mode — emits the raw `rcc_lexer` output with no
    /// directive handling. Full implementation arrives in M5.
    pub fn run(&mut self, root: FileId) -> Vec<PpToken> {
        let src = self.session.source_map.read().unwrap().file(root).src.clone();
        rcc_lexer::tokenize(root, &src).collect()
    }
}
