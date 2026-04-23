//! `rcc_preprocess`: C preprocessor.
//!
//! Implements C99 translation phases 1–4: line splicing, pp-tokenisation
//! (delegated to `rcc_lexer`), macro expansion, and directive handling.
//! Output is a "clean" pp-token stream consumed by `rcc_parse`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_data_structures::FxHashMap;
use rcc_lexer::{PpToken, PpTokenKind, Punct};
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
pub use macros::{define_macro, define_object_like, HideSet, MacroDef, MacroKind, MacroTable};

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
    /// Current scope (task 04-06): directive-side-effect pass.
    /// `#define` / `#undef` update [`Self::macros`] in place; every
    /// other directive variant is parsed and then skipped (full
    /// dispatch arrives with tasks 04-08 / 04-13 / 04-14). Output
    /// tokens are still the raw `rcc_lexer` stream — macro expansion
    /// is task 04-08, and until then directive lines flow through to
    /// the caller verbatim so that ancillary scanners (include-guard
    /// detection, `#pragma once` detection, ...) see the original
    /// shape of the file.
    pub fn run(&mut self, root: FileId) -> Vec<PpToken> {
        let src = self.session.source_map.read().unwrap().file(root).src.clone();
        let tokens: Vec<PpToken> = rcc_lexer::tokenize(root, &src).collect();

        let mut ls = line_stream::LineStream::new(tokens.iter().cloned());
        while let Some(line) = ls.next_line() {
            if !is_directive_line(&line) {
                continue;
            }
            // Null directive (`#` alone): has no side effect; fall through.
            if line.len() == 1 {
                continue;
            }
            self.dispatch_directive(&line, &src);
        }

        tokens
    }

    /// Parse one logical `#`-line and apply its side effects.
    ///
    /// Caller must guarantee `line` starts with `#` at line-start and
    /// has at least two tokens (see [`is_directive_line`]).
    fn dispatch_directive(&mut self, line: &[PpToken], src: &str) {
        match directive::parse_directive(line, src, &mut self.session.interner) {
            Ok(directive::Directive::Define(def)) => {
                let result = {
                    let sm = self.session.source_map.read().unwrap();
                    macros::define_macro(def, &mut self.macros, &sm, &self.session.interner)
                };
                if let Err(diag) = result {
                    self.session.handler.emit(&diag);
                }
            }
            Ok(directive::Directive::Undef { name, .. }) => {
                // `#undef` of an undefined macro is a no-op per C99
                // §6.10.5p2; the `bool` return from `undef` is
                // intentionally ignored here.
                self.macros.undef(name);
            }
            // Other directives (Include / Conditional / Line / Error /
            // Pragma) are parsed-but-not-dispatched here; later tasks
            // (04-08 expansion, 04-13 #if eval, 04-14 conditional
            // stack, 04-15 #line, 04-16 #error/pragma) take them.
            Ok(_) => {}
            Err(diag) => self.session.handler.emit(&diag),
        }
    }
}

/// Whether `line` (as produced by [`line_stream::LineStream`]) is a
/// preprocessing directive — i.e. begins with a `#` punctuator whose
/// [`PpToken::at_line_start`] flag is set.
fn is_directive_line(line: &[PpToken]) -> bool {
    line.first()
        .map(|t| matches!(t.kind, PpTokenKind::Punct(Punct::Hash)) && t.at_line_start)
        .unwrap_or(false)
}

#[cfg(test)]
mod run_tests {
    //! End-to-end tests for [`Preprocessor::run`]'s directive-dispatch
    //! loop (task 04-06: object-like `#define` / `#undef`).

    use super::*;
    use rcc_errors::{codes::E0022, CaptureEmitter, Handler};
    use rcc_session::{Options, Session};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Load `src` into a fresh session (with a capturing emitter) and
    /// return `(Session, FileId, CaptureEmitter)`.
    fn seed(src: &str) -> (Session, FileId, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let sess =
            Session::with_handler(Options::default(), Handler::with_emitter(Box::new(cap.clone())));
        let id = sess.source_map.write().unwrap().add_file(PathBuf::from("<unit>"), Arc::from(src));
        (sess, id, cap)
    }

    #[test]
    fn acceptance_define_roundtrip_exposes_body_as_pp_number() {
        let (mut sess, id, cap) = seed("#define FOO 42\n");
        let foo = sess.interner.intern("FOO");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(foo).expect("FOO must be defined after run");
        assert!(matches!(def.kind, MacroKind::ObjectLike), "FOO is an object-like macro");
        assert_eq!(def.body.len(), 1, "replacement list is a single `42` pp-number");
        assert!(
            matches!(def.body[0].kind, PpTokenKind::PpNumber(_)),
            "body token must be a pp-number, got {:?}",
            def.body[0].kind
        );
        assert!(cap.diagnostics().is_empty(), "a fresh `#define` must not diagnose");
    }

    #[test]
    fn benign_redefinition_is_silently_accepted() {
        let (mut sess, id, cap) = seed("#define FOO 42\n#define FOO 42\n");
        let foo = sess.interner.intern("FOO");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        assert!(pp.macros.is_defined(foo));
        assert!(
            cap.diagnostics().is_empty(),
            "identical redefinition is C99 §6.10.3p1 benign: got {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn differing_redefinition_emits_e0022_with_two_labels() {
        let (mut sess, id, cap) = seed("#define FOO 42\n#define FOO 43\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0022 expected, got {diags:?}");
        let d = &diags[0];
        assert_eq!(d.code, Some(E0022));
        assert_eq!(d.labels.len(), 2, "both defs must be labelled");
        assert!(d.labels.iter().any(|l| l.primary), "primary label on the new def");
        assert!(d.labels.iter().any(|l| !l.primary), "secondary label on the previous def");
    }

    #[test]
    fn undef_removes_definition_and_allows_redefine() {
        let (mut sess, id, cap) = seed("#define FOO 42\n#undef FOO\n#define FOO 43\n");
        let foo = sess.interner.intern("FOO");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(foo).expect("FOO must be redefined");
        let sm = pp.session.source_map.read().unwrap();
        let src = &sm.file(def.body[0].span.file).src;
        let txt = &src[def.body[0].span.lo.0 as usize..def.body[0].span.hi.0 as usize];
        assert_eq!(txt, "43", "body must reflect the post-undef definition");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn undef_of_undefined_name_is_silent() {
        // C99 §6.10.5p2: `#undef` of a name that is not currently
        // defined is explicitly legal and must not produce a
        // diagnostic.
        let (mut sess, id, cap) = seed("#undef NEVER_DEFINED\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        assert!(cap.diagnostics().is_empty(), "undef of an unknown name must be silent");
    }

    // ── Function-like `#define` end-to-end (task 04-07) ─────────────

    #[test]
    fn acceptance_function_like_multi_param_is_registered() {
        // `#define MAX(a,b) ((a)>(b)?(a):(b))` — two params, not variadic.
        let (mut sess, id, cap) = seed("#define MAX(a,b) ((a)>(b)?(a):(b))\n");
        let max = sess.interner.intern("MAX");
        let a_sym = sess.interner.intern("a");
        let b_sym = sess.interner.intern("b");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(max).expect("MAX must be defined");
        match &def.kind {
            MacroKind::FunctionLike { params, variadic } => {
                assert_eq!(params, &vec![a_sym, b_sym]);
                assert!(!variadic);
            }
            other => panic!("expected FunctionLike, got {other:?}"),
        }
        assert!(!def.body.is_empty());
        assert!(cap.diagnostics().is_empty(), "well-formed fn-like define must not diagnose");
    }

    #[test]
    fn acceptance_variadic_macro_sets_variadic_flag() {
        let (mut sess, id, cap) = seed("#define V(...) __VA_ARGS__\n");
        let v = sess.interner.intern("V");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(v).expect("V must be defined");
        match &def.kind {
            MacroKind::FunctionLike { params, variadic } => {
                assert!(params.is_empty());
                assert!(*variadic, "`...`-only param list → variadic=true");
            }
            other => panic!("expected FunctionLike, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn function_like_zero_param_form_is_distinct_from_object_like() {
        // `#define F() 42` is function-like with zero params; it is
        // NOT the same macro as `#define F 42` (§6.10.3p1 requires
        // kind agreement).
        let (mut sess, id, cap) = seed("#define F() 42\n");
        let f = sess.interner.intern("F");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(f).expect("F must be defined");
        assert!(
            matches!(def.kind, MacroKind::FunctionLike { ref params, variadic: false } if params.is_empty()),
            "zero-param fn-like expected, got {:?}",
            def.kind
        );
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn whitespace_before_paren_keeps_define_object_like() {
        // §6.10.3p10: `#define F (x) x` is object-like whose body is
        // `(x) x`, NOT a function-like macro with parameter `x`.
        let (mut sess, id, cap) = seed("#define F (x) x\n");
        let f = sess.interner.intern("F");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let def = pp.macros.get(f).expect("F must be defined");
        assert!(matches!(def.kind, MacroKind::ObjectLike), "space before `(` → object-like");
        // Body is `(`, `x`, `)`, `x` — four pp-tokens.
        assert_eq!(def.body.len(), 4);
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn duplicate_param_emits_e0023() {
        use rcc_errors::codes::E0023;
        let (mut sess, id, cap) = seed("#define FOO(a, a) a\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0023 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0023));
    }

    #[test]
    fn function_like_benign_redefinition_is_silent() {
        let (mut sess, id, cap) =
            seed("#define MAX(a,b) ((a)>(b)?(a):(b))\n#define MAX(a,b) ((a)>(b)?(a):(b))\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        assert!(
            cap.diagnostics().is_empty(),
            "identical fn-like redefinition must be benign, got {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn function_like_param_rename_emits_e0022() {
        use rcc_errors::codes::E0022;
        let (mut sess, id, cap) =
            seed("#define MAX(a,b) ((a)>(b)?(a):(b))\n#define MAX(x,b) ((x)>(b)?(x):(b))\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0022 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0022));
    }
}
