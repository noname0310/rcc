//! `Handler`: owns emitters and tracks diagnostic counts.

use rcc_span::Span;

use crate::builder::DiagnosticBuilder;
use crate::emitter::Emitter;
use crate::{Diagnostic, Level};

/// Central diagnostic sink. Crates receive `&mut Handler` (usually through
/// `Session`) and build diagnostics via [`DiagnosticBuilder`].
pub struct Handler {
    emitter: Box<dyn Emitter>,
    error_count: u32,
    warning_count: u32,
}

impl Handler {
    /// Build a handler around a custom emitter.
    pub fn with_emitter(emitter: Box<dyn Emitter>) -> Self {
        Self { emitter, error_count: 0, warning_count: 0 }
    }

    /// Number of `Level::Error` or `Level::Bug` diagnostics emitted so far.
    pub fn error_count(&self) -> u32 {
        self.error_count
    }

    /// Number of `Level::Warning` diagnostics emitted so far.
    pub fn warning_count(&self) -> u32 {
        self.warning_count
    }

    /// Whether compilation should be considered failed.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// Low-level emit. Prefer the builder API.
    pub fn emit(&mut self, d: &Diagnostic) {
        match d.level {
            Level::Error | Level::Bug => self.error_count += 1,
            Level::Warning => self.warning_count += 1,
            Level::Note | Level::Help => {}
        }
        self.emitter.emit(d);
    }

    /// Start an error diagnostic.
    pub fn struct_err(&mut self, span: Span, msg: impl Into<String>) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder::new(self, Level::Error, msg).primary(span, "")
    }

    /// Start a warning diagnostic.
    pub fn struct_warn(&mut self, span: Span, msg: impl Into<String>) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder::new(self, Level::Warning, msg).primary(span, "")
    }

    /// Start a plain (unspanned) error.
    pub fn err(&mut self, msg: impl Into<String>) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder::new(self, Level::Error, msg)
    }
}
