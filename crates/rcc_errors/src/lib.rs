//! `rcc_errors`: diagnostic system for the rcc C compiler.
//!
//! Analogous to `rustc_errors`. Front-ends build `Diagnostic`s through a
//! `DiagnosticBuilder` and hand them to a `Handler`, which fans them out to
//! one or more `Emitter`s. Emitters format and print (or capture) them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_span::Span;

mod builder;
pub mod codes;
mod emitter;
mod handler;

pub use builder::DiagnosticBuilder;
pub use emitter::{CaptureEmitter, Emitter, StderrEmitter};
pub use handler::Handler;

/// Severity level of a `Diagnostic`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// A fatal or non-fatal error; counted toward the error count.
    Error,
    /// A warning; does not fail the compilation by itself.
    Warning,
    /// An informational note attached (usually) to another diagnostic.
    Note,
    /// A suggestion to the user.
    Help,
    /// Internal compiler error.
    Bug,
}

/// A span with an optional message, rendered as a labelled arrow.
#[derive(Clone, Debug)]
pub struct Label {
    /// Primary span the label points at.
    pub span: Span,
    /// Short message rendered next to the span.
    pub message: String,
    /// Whether this is the primary span of the diagnostic.
    pub primary: bool,
}

/// A fully-built diagnostic. Prefer constructing through `DiagnosticBuilder`.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// Severity.
    pub level: Level,
    /// Stable compiler-error code, e.g. `E0001`. Optional.
    pub code: Option<&'static str>,
    /// Main message shown in the header.
    pub message: String,
    /// Labelled spans pointing into source.
    pub labels: Vec<Label>,
    /// Notes shown after the labels.
    pub notes: Vec<String>,
    /// Help / suggestion lines.
    pub help: Vec<String>,
}
