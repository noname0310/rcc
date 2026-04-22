//! Diagnostic emitters: where rendered diagnostics go.

use std::sync::{Arc, Mutex};

use crate::{Diagnostic, Level};

/// Receives fully-built diagnostics and renders them.
pub trait Emitter: Send {
    /// Render a single diagnostic.
    fn emit(&mut self, d: &Diagnostic);
}

/// No-op terminal emitter. A richer `ariadne`-based implementation is planned
/// in M0 follow-up; this placeholder keeps the pipeline buildable.
#[derive(Default)]
pub struct StderrEmitter;

impl Emitter for StderrEmitter {
    fn emit(&mut self, d: &Diagnostic) {
        let tag = match d.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
            Level::Help => "help",
            Level::Bug => "internal compiler error",
        };
        let code = d.code.map(|c| format!("[{c}]")).unwrap_or_default();
        eprintln!("{tag}{code}: {}", d.message);
        for l in &d.labels {
            let pointer = if l.primary { "-->" } else { "   " };
            eprintln!(
                "  {pointer} {:?} @ {}..{}  {}",
                l.span.file, l.span.lo.0, l.span.hi.0, l.message
            );
        }
        for n in &d.notes {
            eprintln!("  = note: {n}");
        }
        for h in &d.help {
            eprintln!("  = help: {h}");
        }
    }
}

/// Collects every emitted diagnostic into a shared `Vec`. Used by tests.
#[derive(Default, Clone)]
pub struct CaptureEmitter {
    inner: Arc<Mutex<Vec<Diagnostic>>>,
}

impl CaptureEmitter {
    /// Build a new capture emitter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clone of every diagnostic captured so far.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner.lock().unwrap().clone()
    }
}

impl Emitter for CaptureEmitter {
    fn emit(&mut self, d: &Diagnostic) {
        self.inner.lock().unwrap().push(d.clone());
    }
}
