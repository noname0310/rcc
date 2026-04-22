//! Fluent builder for `Diagnostic`.

use rcc_span::Span;

use crate::{Diagnostic, Handler, Label, Level};

/// Fluent builder. On drop, the diagnostic is emitted to the `Handler`.
#[must_use = "diagnostics must be emitted with `.emit()` or let the builder drop"]
pub struct DiagnosticBuilder<'a> {
    handler: &'a mut Handler,
    diag: Option<Diagnostic>,
}

impl<'a> DiagnosticBuilder<'a> {
    /// Start a new diagnostic.
    pub fn new(handler: &'a mut Handler, level: Level, message: impl Into<String>) -> Self {
        Self {
            handler,
            diag: Some(Diagnostic {
                level,
                code: None,
                message: message.into(),
                labels: Vec::new(),
                notes: Vec::new(),
                help: Vec::new(),
            }),
        }
    }

    /// Set a stable error code (e.g. `"E0001"`).
    pub fn code(mut self, code: &'static str) -> Self {
        self.diag.as_mut().unwrap().code = Some(code);
        self
    }

    /// Attach a primary labelled span.
    pub fn primary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.diag.as_mut().unwrap().labels.push(Label {
            span,
            message: message.into(),
            primary: true,
        });
        self
    }

    /// Attach a secondary labelled span.
    pub fn label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.diag.as_mut().unwrap().labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }

    /// Attach a free-form note.
    pub fn note(mut self, message: impl Into<String>) -> Self {
        self.diag.as_mut().unwrap().notes.push(message.into());
        self
    }

    /// Attach a help / suggestion line.
    pub fn help(mut self, message: impl Into<String>) -> Self {
        self.diag.as_mut().unwrap().help.push(message.into());
        self
    }

    /// Finalize and emit immediately.
    pub fn emit(mut self) {
        let d = self.diag.take().expect("diagnostic already emitted");
        self.handler.emit(&d);
    }
}

impl Drop for DiagnosticBuilder<'_> {
    fn drop(&mut self) {
        if let Some(d) = self.diag.take() {
            self.handler.emit(&d);
        }
    }
}
