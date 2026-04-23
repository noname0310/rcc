//! Conditional-compilation state machine (C99 §6.10.1).
//!
//! The preprocessor tracks nested `#if` / `#ifdef` / `#ifndef` /
//! `#elif` / `#else` / `#endif` groups as a stack of [`CondFrame`]s.
//! Each frame records:
//!
//! - `taken`:    some branch of this group has already evaluated true.
//!   Once true, every later `#elif` in the same group becomes
//!   inactive, and an eventual `#else` is likewise inactive.
//! - `active`:   the *current* branch is the one being emitted. Only
//!   one slot in a `#if`/`#elif`/`#else` chain is ever active at a
//!   time; arithmetic on `active` happens at every conditional
//!   directive.
//! - `else_seen`: an `#else` has already been consumed for this
//!   group. Any further `#elif` is a constraint violation; a second
//!   `#else` is a duplicate.
//! - `open_span`: location of the opening `#if`/`#ifdef`/`#ifndef`
//!   keyword. Captured so the end-of-file check can point its
//!   E0018 "missing `#endif`" diagnostic at the offending directive.
//!
//! ### Active vs parent-active
//!
//! The whole stack is *active* iff every frame's `active` flag is
//! set — `is_active()`. A nested `#if` inside an inactive ancestor
//! must stay inactive regardless of its own controlling expression;
//! the controlling expression is not even evaluated in that case
//! (matching GCC/Clang behaviour and avoiding spurious E0028 for
//! dead-code division-by-zero). Callers consult
//! [`CondStack::parent_active`] before deciding whether to run
//! `eval_if` at all.
//!
//! ### Scope
//!
//! One `CondStack` per call to [`crate::Preprocessor::run`]. Nested
//! `#include`d files get their own fresh stack (a header's
//! conditionals cannot span across its closing brace), which matches
//! the C99 §6.10 translation-phase model where each source file is
//! preprocessed as an independent translation unit of directives.
//! End-of-file with unclosed frames emits one E0018 per remaining
//! frame.

use rcc_errors::{
    codes::{E0015, E0016, E0017, E0018},
    Diagnostic, Label, Level,
};
use rcc_span::Span;

/// One entry in the `#if` / `#endif` nesting stack.
#[derive(Debug, Clone)]
pub struct CondFrame {
    /// Whether any branch of this conditional group has already
    /// evaluated to a non-zero value (the initial `#if` / `#ifdef` /
    /// `#ifndef` or any subsequent `#elif`). Once set, no further
    /// branch in the same group can activate.
    pub taken: bool,
    /// Whether the currently-open branch should emit tokens and apply
    /// directive side effects. Every directive transition in this
    /// group recomputes `active` based on `taken`, the parent stack's
    /// activity, and `else_seen`.
    pub active: bool,
    /// Whether an `#else` has already been seen in this group. A
    /// second `#else` or any `#elif` after `#else` is a constraint
    /// violation per C99 §6.10.1p2.
    pub else_seen: bool,
    /// Span of the opening directive's `#if` / `#ifdef` / `#ifndef`
    /// keyword; used as the primary label on E0018 when the group
    /// is still open at end of file.
    pub open_span: Span,
}

/// The conditional-compilation stack.
#[derive(Debug, Default)]
pub struct CondStack {
    frames: Vec<CondFrame>,
}

impl CondStack {
    /// Fresh empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the stack is currently emitting tokens. Equivalent to
    /// "every frame is active"; an empty stack is trivially active
    /// (top-level code).
    pub fn is_active(&self) -> bool {
        self.frames.iter().all(|f| f.active)
    }

    /// Whether the *parent* stack — every frame except the top — is
    /// active. Used by `#elif`/`#else` when deciding whether their
    /// branch may activate: a dead outer region must keep its
    /// inner groups fully skipped regardless of their conditions.
    /// When the stack is empty, "parent" is top-level and therefore
    /// trivially active.
    pub fn parent_active(&self) -> bool {
        let n = self.frames.len();
        if n <= 1 {
            return true;
        }
        self.frames[..n - 1].iter().all(|f| f.active)
    }

    /// Whether an `#if` / `#elif`'s controlling expression should be
    /// evaluated at all. Returns `false` when the enclosing stack is
    /// already dead — evaluation of the dead branch is suppressed to
    /// match GCC/Clang and to avoid surfacing dead-code diagnostics
    /// (in particular E0028 for `1/0` in a skipped `#if`).
    pub fn should_evaluate(&self) -> bool {
        self.frames.iter().all(|f| f.active)
    }

    /// Push a new conditional group. `cond` is the truth value of
    /// the controlling expression (already evaluated if and only if
    /// [`Self::should_evaluate`] was true before the push); if the
    /// parent stack is inactive the new frame is forced inactive
    /// regardless of `cond`.
    pub fn push_if(&mut self, cond: bool, open_span: Span) {
        let parent = self.is_active();
        let take = parent && cond;
        self.frames.push(CondFrame { taken: take, active: take, else_seen: false, open_span });
    }

    /// Transition on `#elif`. Returns `Some(diag)` for a constraint
    /// violation (stack empty, or `#elif` after `#else`). Regardless
    /// of the return value the stack state is left consistent — a
    /// diagnosed `#elif` leaves the current frame inactive so the
    /// rest of the source parses deterministically.
    ///
    /// `cond` is consulted only when the group has not yet taken any
    /// branch and the parent is still active; in every other case
    /// the controlling expression is dead code and the closure is
    /// not called. Callers can therefore skip [`crate::eval_if`]
    /// entirely when the stack is already inactive, avoiding
    /// dead-code diagnostics.
    pub fn on_elif<F>(&mut self, cond: F, directive_span: Span) -> Option<Diagnostic>
    where
        F: FnOnce() -> bool,
    {
        if self.frames.is_empty() {
            return Some(unmatched_else_elif(directive_span, "#elif"));
        }
        let parent_active = self.parent_active();
        let top = self.frames.last_mut().expect("frames non-empty, pre-checked above");
        if top.else_seen {
            top.active = false;
            return Some(elif_after_else(directive_span));
        }
        if top.taken {
            top.active = false;
        } else if parent_active && cond() {
            top.taken = true;
            top.active = true;
        } else {
            top.active = false;
        }
        None
    }

    /// Transition on `#else`. Returns `Some(diag)` for stack-empty
    /// or duplicate-`#else`; the frame is still marked `else_seen`
    /// after a duplicate report so that a later `#endif` still pops
    /// the group cleanly.
    pub fn on_else(&mut self, directive_span: Span) -> Option<Diagnostic> {
        if self.frames.is_empty() {
            return Some(unmatched_else_elif(directive_span, "#else"));
        }
        let parent_active = self.parent_active();
        let top = self.frames.last_mut().expect("frames non-empty, pre-checked above");
        if top.else_seen {
            top.active = false;
            return Some(duplicate_else(directive_span));
        }
        top.else_seen = true;
        if top.taken {
            top.active = false;
        } else if parent_active {
            top.taken = true;
            top.active = true;
        } else {
            top.active = false;
        }
        None
    }

    /// Transition on `#endif`. Returns `Some(diag)` when the stack
    /// is empty (nothing to close); a successful pop returns `None`.
    pub fn on_endif(&mut self, directive_span: Span) -> Option<Diagnostic> {
        if self.frames.pop().is_none() {
            Some(unmatched_endif(directive_span))
        } else {
            None
        }
    }

    /// Consume the stack and return any remaining (unclosed) frames.
    /// Called from [`crate::Preprocessor::run`] at end-of-file; the
    /// caller emits one E0018 per returned frame.
    pub fn into_unclosed(self) -> Vec<CondFrame> {
        self.frames
    }

    /// Number of open frames — primarily exposed for tests and
    /// assertion-driven debugging.
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Whether the top frame is waiting for a branch to evaluate
    /// true, i.e. no `#else` has been seen and no prior `#if` /
    /// `#elif` branch has been taken. Used by the preprocessor's
    /// `#elif` dispatcher to decide whether the controlling
    /// expression is worth evaluating (evaluation is suppressed once
    /// a branch is taken so later `#elif`s don't spend time on dead
    /// code). Returns `None` for an empty stack.
    pub fn top_needs_eval(&self) -> Option<bool> {
        self.frames.last().map(|f| !f.taken && !f.else_seen)
    }
}

// ── Diagnostic constructors ──────────────────────────────────────────

/// E0017 variant: `#else` or `#elif` with no matching open `#if`.
fn unmatched_else_elif(span: Span, keyword: &str) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0017),
        message: format!("unmatched `{keyword}`"),
        labels: vec![Label {
            span,
            message: format!("no open `#if` / `#ifdef` / `#ifndef` to match `{keyword}`"),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1: each `#elif` / `#else` must appear \
             inside an open conditional group"
            .into()],
        help: vec![],
    }
}

/// E0017 variant: a `#elif` follows a `#else` in the same group.
fn elif_after_else(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0017),
        message: "`#elif` after `#else`".into(),
        labels: vec![Label {
            span,
            message: "no more branches are allowed once `#else` has been seen".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1p2: once an `#else` group has opened, \
             the only directive that may close its conditional group is `#endif`"
            .into()],
        help: vec!["remove the `#elif`, or move it ahead of the `#else`".into()],
    }
}

/// E0017 variant: two `#else` directives in the same group.
fn duplicate_else(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0017),
        message: "duplicate `#else`".into(),
        labels: vec![Label {
            span,
            message: "a second `#else` for the same conditional group".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1p2: a conditional group may contain at most one `#else`".into()],
        help: vec!["drop this `#else`, or close the previous group with `#endif` first".into()],
    }
}

/// E0016: `#endif` with no matching open `#if`.
fn unmatched_endif(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0016),
        message: "unmatched `#endif`".into(),
        labels: vec![Label {
            span,
            message: "no open `#if` / `#ifdef` / `#ifndef` to close".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1: every `#endif` must close a matching \
             `#if` / `#ifdef` / `#ifndef`"
            .into()],
        help: vec![],
    }
}

/// E0018: end of file with an open conditional group. Points at the
/// opening `#if` / `#ifdef` / `#ifndef` keyword.
pub fn missing_endif(open_span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0018),
        message: "missing `#endif` at end of file".into(),
        labels: vec![Label {
            span: open_span,
            message: "this conditional group was never closed".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1: every `#if` / `#ifdef` / `#ifndef` \
             must be matched by a `#endif` before end of file"
            .into()],
        help: vec!["add a `#endif` before the end of the file".into()],
    }
}

/// E0015: `#ifdef` / `#ifndef` without an identifier operand. Exposed
/// so [`crate::Preprocessor::run`] can reuse it when evaluating the
/// `ConditionalKind::IfDef` / `::IfNDef` branches of
/// [`crate::directive::Directive::Conditional`].
pub fn expected_ident_after_ifdef(span: Span, is_ndef: bool) -> Diagnostic {
    let keyword = if is_ndef { "#ifndef" } else { "#ifdef" };
    Diagnostic {
        level: Level::Error,
        code: Some(E0015),
        message: format!("expected identifier after `{keyword}`"),
        labels: vec![Label {
            span,
            message: format!("`{keyword}` requires a single macro name"),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1: `#ifdef` and `#ifndef` take exactly one identifier".into()],
        help: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_span::{BytePos, FileId};

    fn dummy_span() -> Span {
        Span::new(FileId(0), BytePos(0), BytePos(1))
    }

    #[test]
    fn empty_stack_is_active() {
        let s = CondStack::new();
        assert!(s.is_active());
        assert_eq!(s.depth(), 0);
    }

    #[test]
    fn if_true_then_endif_leaves_active() {
        let mut s = CondStack::new();
        s.push_if(true, dummy_span());
        assert!(s.is_active());
        assert_eq!(s.depth(), 1);
        assert!(s.on_endif(dummy_span()).is_none());
        assert!(s.is_active());
        assert_eq!(s.depth(), 0);
    }

    #[test]
    fn if_false_then_else_flips_to_active() {
        let mut s = CondStack::new();
        s.push_if(false, dummy_span());
        assert!(!s.is_active());
        assert!(s.on_else(dummy_span()).is_none());
        assert!(s.is_active());
        assert!(s.on_endif(dummy_span()).is_none());
    }

    #[test]
    fn if_true_then_else_flips_to_inactive() {
        let mut s = CondStack::new();
        s.push_if(true, dummy_span());
        assert!(s.is_active());
        assert!(s.on_else(dummy_span()).is_none());
        assert!(!s.is_active(), "else after a taken branch must skip");
        assert!(s.on_endif(dummy_span()).is_none());
    }

    #[test]
    fn nested_inactive_outer_suppresses_inner_if() {
        let mut s = CondStack::new();
        s.push_if(false, dummy_span()); // outer: inactive
        s.push_if(true, dummy_span()); // inner cond true but parent dead
        assert!(!s.is_active(), "inner `#if` inside an inactive region must stay dead");
        let _ = s.on_endif(dummy_span());
        assert!(!s.is_active());
        assert!(s.on_endif(dummy_span()).is_none());
        assert!(s.is_active());
    }

    #[test]
    fn elif_in_untaken_group_activates_when_true() {
        let mut s = CondStack::new();
        s.push_if(false, dummy_span());
        assert!(s.on_elif(|| true, dummy_span()).is_none());
        assert!(s.is_active());
        assert!(s.on_endif(dummy_span()).is_none());
    }

    #[test]
    fn elif_after_taken_stays_inactive_without_evaluating() {
        let mut s = CondStack::new();
        s.push_if(true, dummy_span());
        let mut called = false;
        assert!(s
            .on_elif(
                || {
                    called = true;
                    true
                },
                dummy_span(),
            )
            .is_none());
        assert!(!called, "elif condition must not run once a branch is taken");
        assert!(!s.is_active());
        assert!(s.on_endif(dummy_span()).is_none());
    }

    #[test]
    fn duplicate_else_emits_e0017() {
        let mut s = CondStack::new();
        s.push_if(false, dummy_span());
        assert!(s.on_else(dummy_span()).is_none());
        let diag = s.on_else(dummy_span()).expect("second #else must diagnose");
        assert_eq!(diag.code, Some(E0017));
    }

    #[test]
    fn elif_after_else_emits_e0017() {
        let mut s = CondStack::new();
        s.push_if(false, dummy_span());
        assert!(s.on_else(dummy_span()).is_none());
        let diag = s.on_elif(|| true, dummy_span()).expect("#elif after #else must diagnose");
        assert_eq!(diag.code, Some(E0017));
    }

    #[test]
    fn unmatched_endif_emits_e0016() {
        let mut s = CondStack::new();
        let diag = s.on_endif(dummy_span()).expect("bare #endif must diagnose");
        assert_eq!(diag.code, Some(E0016));
    }

    #[test]
    fn unmatched_else_emits_e0017() {
        let mut s = CondStack::new();
        let diag = s.on_else(dummy_span()).expect("bare #else must diagnose");
        assert_eq!(diag.code, Some(E0017));
    }

    #[test]
    fn eof_with_open_group_surfaces_unclosed_frames() {
        let mut s = CondStack::new();
        s.push_if(true, dummy_span());
        s.push_if(false, dummy_span());
        let left = s.into_unclosed();
        assert_eq!(left.len(), 2, "both frames must surface for E0018 emission");
    }
}
