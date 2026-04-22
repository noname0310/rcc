//! Parsed preprocessing directive form. M5 materialises these; held here to
//! freeze the interface.

use rcc_lexer::PpToken;
use rcc_span::{Span, Symbol};

use crate::macros::MacroDef;

/// A single preprocessing directive after lexing `#...`.
#[derive(Clone, Debug)]
pub enum Directive {
    /// `#include "..."` / `#include <...>`
    Include {
        /// Full directive span.
        span: Span,
        /// Whether the form was `<...>` (system header).
        is_system: bool,
        /// Raw header name text (lexed as `HeaderName`).
        header: String,
    },
    /// `#define ...`
    Define(MacroDef),
    /// `#undef NAME`
    Undef {
        /// Full directive span.
        span: Span,
        /// Target macro name.
        name: Symbol,
    },
    /// `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else`, `#endif`
    Conditional {
        /// Full directive span.
        span: Span,
        /// Specific conditional kind.
        kind: ConditionalKind,
        /// Controlling expression tokens (for `#if`/`#elif`), raw.
        condition: Vec<PpToken>,
    },
    /// `#line N "file"?`
    Line {
        /// Full directive span.
        span: Span,
        /// New line number.
        line: u32,
        /// Optional new file name.
        file: Option<String>,
    },
    /// `#error "msg"`
    Error {
        /// Full directive span.
        span: Span,
        /// Message body.
        message: String,
    },
    /// `#pragma ...`
    Pragma {
        /// Full directive span.
        span: Span,
        /// Raw pragma tokens (implementation-defined).
        tokens: Vec<PpToken>,
    },
}

/// Specific `#if` / `#ifdef` / ... form.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConditionalKind {
    /// `#if`
    If,
    /// `#ifdef`
    IfDef,
    /// `#ifndef`
    IfNDef,
    /// `#elif`
    ElIf,
    /// `#else`
    Else,
    /// `#endif`
    EndIf,
}
