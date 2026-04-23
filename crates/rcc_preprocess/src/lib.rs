//! `rcc_preprocess`: C preprocessor.
//!
//! Implements C99 translation phases 1–4: line splicing, pp-tokenisation
//! (delegated to `rcc_lexer`), macro expansion, and directive handling.
//! Output is a "clean" pp-token stream consumed by `rcc_parse`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rcc_data_structures::FxHashMap;
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_session::Session;
use rcc_span::{BytePos, FileId, Span, Symbol};

pub mod directive;
pub mod expand;
pub mod guard;
pub mod if_eval;
pub mod include;
pub mod line_stream;
pub mod macros;

pub use directive::{parse_directive, ConditionalKind, Directive};
pub use expand::expand_line;
pub use guard::detect_guard;
pub use if_eval::eval_if;
pub use include::{detect_pragma_once, resolve_header, strip_header_delimiters};
pub use line_stream::LineStream;
pub use macros::{
    define_macro, define_object_like, undef_user, BuiltinMacro, HideSet, MacroDef, MacroKind,
    MacroTable, VA_ARGS_NAME,
};

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
    /// Set once [`Self::install_cli_defines`] + [`Self::install_predefined`]
    /// have seeded the macro table. `run()` is re-entered for every
    /// `#include`d file and must install those entries *exactly* once
    /// per preprocessor instance; this latch is how it tells the
    /// top-level invocation apart from the recursive ones.
    predefined_installed: bool,
}

impl<'a> Preprocessor<'a> {
    /// Build a new preprocessor.
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            macros: MacroTable::default(),
            include_guards: FxHashMap::default(),
            pragma_once: FxHashMap::default(),
            predefined_installed: false,
        }
    }

    /// Run preprocessing and return the expanded token stream.
    ///
    /// Directive lines apply their side effects (`#define` / `#undef`
    /// populate [`Self::macros`]; other directive variants are parsed
    /// and then skipped pending later tasks 04-13 / 04-14 / 04-15 /
    /// 04-16). Non-directive lines are fed through Prosser's
    /// hide-set macro expander (task 04-08), and the rescanned tokens
    /// are concatenated into the returned stream. Newline separators
    /// are consumed by [`line_stream::LineStream`] and not re-emitted.
    ///
    /// Before any source is seen, [`Self::install_cli_defines`] and
    /// [`Self::install_predefined`] seed the macro table in that
    /// order: CLI `-D` flags first so their entries are ordinary
    /// object-like macros, then the C99 §6.10.8 predefined set
    /// (which unconditionally overrides any colliding `-D`).
    pub fn run(&mut self, root: FileId) -> Vec<PpToken> {
        if !self.predefined_installed {
            self.predefined_installed = true;
            self.install_cli_defines();
            self.install_predefined();
        }

        let src = self.session.source_map.read().unwrap().file(root).src.clone();
        let tokens: Vec<PpToken> = rcc_lexer::tokenize(root, &src).collect();

        let mut out: Vec<PpToken> = Vec::new();
        let mut ls = line_stream::LineStream::new(tokens.into_iter());
        while let Some(line) = ls.next_line() {
            if is_directive_line(&line) {
                // Null directive (`#` alone): no side effect, no output.
                if line.len() == 1 {
                    continue;
                }
                self.dispatch_directive(&line, &src);
                continue;
            }
            // Non-directive: run Prosser expansion.
            let expanded = self.expand_tokens(line);
            out.extend(expanded);
        }

        out
    }

    /// Entry point for expanding a single pre-tokenised token into its
    /// post-macro pp-token sequence. Convenience wrapper around
    /// [`expand::expand_line`]; typically used by tests and by
    /// follow-up tasks (e.g. 04-13 `#if` expression evaluation) which
    /// expand individual identifiers before constant folding. For
    /// function-like invocations spanning multiple tokens, callers
    /// should use [`Self::expand_tokens`] with the full `name ( args
    /// )` slice instead.
    pub fn expand_one(&mut self, token: PpToken) -> Vec<PpToken> {
        self.expand_tokens(vec![token])
    }

    /// Expand a full logical line (or any sub-sequence) by running
    /// Prosser's algorithm over it. Returns the rescanned, fully
    /// expanded pp-token vector.
    pub fn expand_tokens(&mut self, line: Vec<PpToken>) -> Vec<PpToken> {
        // Clone the source-map Arc so `source_map` is borrowed
        // independently of `self.session`, leaving `self.session.interner`
        // and `self.session.handler` free to be borrowed mutably
        // alongside. The expander itself takes the lock (read for
        // text lookup, brief write for the stringize scratch file).
        let sm_arc = Arc::clone(&self.session.source_map);
        let gnu_va_args_elision = self.session.opts.gnu_va_args_elision;
        expand::expand_line(
            &sm_arc,
            &mut self.session.interner,
            &mut self.session.handler,
            &self.macros,
            line,
            gnu_va_args_elision,
        )
    }

    /// Install every CLI `-D NAME[=VALUE]` flag as an object-like
    /// macro. Invoked once at the top of [`Self::run`] before
    /// [`Self::install_predefined`] so that the predefined C99
    /// §6.10.8 set unconditionally wins on name collisions with the
    /// command line.
    ///
    /// Each `VALUE` is tokenised fresh — the synthesised source file
    /// lives in the session's [`rcc_span::SourceMap`] so diagnostics
    /// pointing at the replacement list have a place to land. A flag
    /// with no `=` (i.e. `-D NAME`) installs the empty-replacement
    /// spelling `NAME` as `1`, matching GCC / Clang convention.
    pub fn install_cli_defines(&mut self) {
        let defines = self.session.opts.cli_defines.clone();
        for (name, value) in defines {
            let body_src = value.unwrap_or_else(|| "1".to_string());
            let file_label = format!("<-D {name}>");
            let file_id = {
                let mut sm = self.session.source_map.write().unwrap();
                sm.add_file(PathBuf::from(file_label), Arc::from(body_src.as_str()))
            };
            let body: Vec<PpToken> = rcc_lexer::tokenize(file_id, &body_src)
                .filter(|t| {
                    !matches!(
                        t.kind,
                        PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof
                    )
                })
                .collect();
            let name_sym = self.session.interner.intern(&name);
            let def_span = Span::new(file_id, BytePos(0), BytePos(body_src.len() as u32));
            self.macros.define(MacroDef {
                name: name_sym,
                kind: MacroKind::ObjectLike,
                body,
                def_span,
                is_predefined: false,
            });
        }
    }

    /// Seed the macro table with the C99 §6.10.8p1 predefined macros.
    ///
    /// Static macros — `__STDC__`, `__STDC_VERSION__`,
    /// `__STDC_HOSTED__`, `__DATE__`, `__TIME__` — are materialised
    /// here as ordinary object-like definitions whose replacement
    /// lists are pre-tokenised synthetic source files. Their value is
    /// frozen at this call; `__DATE__` / `__TIME__` in particular
    /// capture the host's current UTC date and time, as
    /// `asctime`-style strings per §6.10.8.1.
    ///
    /// Dynamic macros — `__FILE__` and `__LINE__` — are installed
    /// with [`MacroKind::Builtin`] and an empty replacement list; the
    /// expander synthesises their value at every use site from the
    /// invocation token's own span.
    ///
    /// `__func__` is intentionally not installed: C99 §6.4.2.2 makes
    /// it a predeclared identifier, not a macro; the parser is
    /// responsible for materialising it inside each function
    /// definition.
    ///
    /// All entries carry [`MacroDef::is_predefined`] = `true`; the
    /// [`define_macro`] / [`undef_user`] helpers refuse to redefine
    /// or remove them (E0027, C99 §6.10.8p2).
    pub fn install_predefined(&mut self) {
        self.install_static_predefined("__STDC__", "1");
        self.install_static_predefined("__STDC_HOSTED__", "1");
        self.install_static_predefined("__STDC_VERSION__", "199901L");
        let (date, time) = current_date_time();
        self.install_static_predefined("__DATE__", &format!("\"{date}\""));
        self.install_static_predefined("__TIME__", &format!("\"{time}\""));
        self.install_builtin_predefined("__FILE__", BuiltinMacro::File);
        self.install_builtin_predefined("__LINE__", BuiltinMacro::Line);
    }

    fn install_static_predefined(&mut self, name: &str, body_src: &str) {
        let file_label = format!("<predefined:{name}>");
        let file_id = {
            let mut sm = self.session.source_map.write().unwrap();
            sm.add_file(PathBuf::from(file_label), Arc::from(body_src))
        };
        let body: Vec<PpToken> = rcc_lexer::tokenize(file_id, body_src)
            .filter(|t| {
                !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof)
            })
            .collect();
        let name_sym = self.session.interner.intern(name);
        let def_span = Span::new(file_id, BytePos(0), BytePos(body_src.len() as u32));
        self.macros.define(MacroDef {
            name: name_sym,
            kind: MacroKind::ObjectLike,
            body,
            def_span,
            is_predefined: true,
        });
    }

    fn install_builtin_predefined(&mut self, name: &str, builtin: BuiltinMacro) {
        let file_label = format!("<predefined:{name}>");
        let file_id = {
            let mut sm = self.session.source_map.write().unwrap();
            sm.add_file(PathBuf::from(file_label), Arc::from(name))
        };
        let name_sym = self.session.interner.intern(name);
        let def_span = Span::new(file_id, BytePos(0), BytePos(name.len() as u32));
        self.macros.define(MacroDef {
            name: name_sym,
            kind: MacroKind::Builtin(builtin),
            body: Vec::new(),
            def_span,
            is_predefined: true,
        });
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
            Ok(directive::Directive::Undef { name, span }) => {
                // `#undef` of an undefined macro is a no-op per C99
                // §6.10.5p2; the `bool` return from `undef_user` is
                // intentionally ignored here. Attempting to `#undef`
                // a predefined macro is a constraint violation per
                // §6.10.8p2 and yields E0027.
                if let Err(diag) =
                    macros::undef_user(name, span, &mut self.macros, &self.session.interner)
                {
                    self.session.handler.emit(&diag);
                }
            }
            Ok(directive::Directive::Conditional {
                kind: directive::ConditionalKind::If | directive::ConditionalKind::ElIf,
                condition,
                ..
            }) => {
                // Task 04-13: evaluate the controlling expression
                // purely for its side-effects (diagnostics). The
                // conditional-stack state machine that actually uses
                // the truth value to skip branches is task 04-14;
                // until it lands, evaluating here simply surfaces
                // E0028 for division-by-zero and similar.
                let _ = self.eval_conditional(&condition);
            }
            // Other directives (Include / Line / Error / Pragma, plus
            // the `#ifdef`/`#ifndef`/`#else`/`#endif` conditional
            // variants) are parsed-but-not-dispatched here; later
            // tasks (04-14 conditional stack, 04-15 #line, 04-16
            // #error/pragma) take them.
            Ok(_) => {}
            Err(diag) => self.session.handler.emit(&diag),
        }
    }

    /// Evaluate the controlling expression of a `#if` / `#elif` and
    /// return its value (or `None` if the expression was ill-formed
    /// and a diagnostic was emitted). Thin wrapper around
    /// [`if_eval::eval_if`] that hands over the session's interner,
    /// handler, and source map; extracted so task 04-14 can call it
    /// from the conditional-stack driver.
    pub fn eval_conditional(&mut self, condition: &[PpToken]) -> Option<i128> {
        let sm_arc = Arc::clone(&self.session.source_map);
        let gnu_va_args_elision = self.session.opts.gnu_va_args_elision;
        match if_eval::eval_if(
            condition,
            &sm_arc,
            &mut self.session.interner,
            &mut self.session.handler,
            &self.macros,
            gnu_va_args_elision,
        ) {
            Ok(v) => Some(v),
            Err(diag) => {
                self.session.handler.emit(&diag);
                None
            }
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

/// Render the host's current UTC wall-clock time as the two strings
/// C99 §6.10.8.1 requires for `__DATE__` and `__TIME__`: `"Mmm dd
/// yyyy"` (space-padded day) and `"HH:MM:SS"`. On a clock that
/// somehow predates the UNIX epoch the seconds count saturates at 0
/// (1970-01-01 00:00:00 UTC) rather than panic; the values are only
/// frozen once per preprocessor run, so even a pathological clock
/// yields internally-consistent output.
fn current_date_time() -> (String, String) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days_since_epoch = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let date = format!("{} {:>2} {:04}", MONTHS[(month - 1) as usize], day, year);
    let time = format!("{hour:02}:{minute:02}:{second:02}");
    (date, time)
}

/// Howard Hinnant's `civil_from_days` (public domain) — converts a
/// Unix day count (days since 1970-01-01) into the proleptic Gregorian
/// `(year, month, day)` triple, with `month` in `1..=12` and `day` in
/// `1..=31`. No leap-second handling is needed for this use.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod run_tests {
    //! End-to-end tests for [`Preprocessor::run`]'s directive-dispatch
    //! loop (task 04-06: object-like `#define` / `#undef`).

    use super::*;
    use rcc_errors::{codes::E0022, CaptureEmitter, Handler};
    use rcc_lexer::PpNumberKind;
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

    // ── End-to-end expansion (task 04-08) ───────────────────────────

    /// Collapse expanded pp-tokens to their concatenated source text
    /// (no inter-token separator). Useful for acceptance assertions.
    fn joined_text(pp: &Preprocessor<'_>, tokens: &[PpToken]) -> String {
        let sm = pp.session.source_map.read().unwrap();
        tokens
            .iter()
            .map(|t| {
                let src = &sm.file(t.span.file).src;
                src[t.span.lo.0 as usize..t.span.hi.0 as usize].to_string()
            })
            .collect()
    }

    #[test]
    fn run_expands_object_like_macro_in_body() {
        // `#define PI 314` followed by a use `PI` → output is `314`.
        let (mut sess, id, cap) = seed("#define PI 314\nPI\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "314");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn run_blocks_self_recursion_with_hide_set() {
        // Acceptance (§): `#define FOO FOO` / `FOO` → literal `FOO`.
        let (mut sess, id, cap) = seed("#define FOO FOO\nFOO\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "FOO");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn run_blocks_mutual_recursion_with_hide_set() {
        // Acceptance: `#define A B / #define B A / A` terminates with `A`.
        let (mut sess, id, cap) = seed("#define A B\n#define B A\nA\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "A");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn run_expands_function_like_max_invocation() {
        let (mut sess, id, cap) = seed("#define MAX(a,b) ((a)>(b)?(a):(b))\nMAX(1, 2)\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "((1)>(2)?(1):(2))");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn run_nested_paren_arg_collects_as_one() {
        // Acceptance: `F((a, b))` → one argument, `(a, b)` substituted for `x`.
        let (mut sess, id, cap) = seed("#define F(x) x\nF((a, b))\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "(a,b)");
        assert!(cap.diagnostics().is_empty());
    }

    // ── Predefined macros (task 04-12) ──────────────────────────────

    /// Build a session seeded at a given pathname (so `__FILE__`'s
    /// spelling is stable and predictable) and return its `FileId`.
    fn seed_at(path: &str, src: &str) -> (Session, FileId, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let sess =
            Session::with_handler(Options::default(), Handler::with_emitter(Box::new(cap.clone())));
        let id = sess.source_map.write().unwrap().add_file(PathBuf::from(path), Arc::from(src));
        (sess, id, cap)
    }

    /// Same as [`seed`] but with a user-supplied [`rcc_session::Options`]
    /// (used to wire up `-D` tests).
    fn seed_with_opts(opts: rcc_session::Options, src: &str) -> (Session, FileId, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
        let id = sess.source_map.write().unwrap().add_file(PathBuf::from("<unit>"), Arc::from(src));
        (sess, id, cap)
    }

    #[test]
    fn predefined_stdc_version_expands_to_199901l() {
        let (mut sess, id, cap) = seed("__STDC_VERSION__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "199901L");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn predefined_stdc_and_stdc_hosted_expand_to_1() {
        let (mut sess, id, cap) = seed("__STDC__\n__STDC_HOSTED__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        // Both lines flatten into the output stream; order is preserved.
        assert_eq!(joined_text(&pp, &out), "11");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn predefined_line_on_line_42_expands_to_42() {
        // 41 `\n` characters put the bare `__LINE__` token on line 42.
        let mut src = String::new();
        for _ in 0..41 {
            src.push('\n');
        }
        src.push_str("__LINE__\n");
        let (mut sess, id, cap) = seed(&src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(out.len(), 1, "one pp-number expected, got {out:?}");
        assert!(
            matches!(out[0].kind, PpTokenKind::PpNumber(PpNumberKind::Integer)),
            "expected PpNumber, got {:?}",
            out[0].kind
        );
        assert_eq!(joined_text(&pp, &out), "42");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn predefined_file_expands_to_string_literal_of_current_path() {
        let (mut sess, id, cap) = seed_at("src/main.c", "__FILE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(out.len(), 1, "one string literal expected");
        assert!(
            matches!(out[0].kind, PpTokenKind::StringLit { .. }),
            "expected StringLit, got {:?}",
            out[0].kind
        );
        // Rendered spelling includes the surrounding quotes and covers
        // the full path verbatim (modulo `\`/`"` escaping — neither
        // appears in this stable test path).
        assert_eq!(joined_text(&pp, &out), "\"src/main.c\"");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn predefined_date_and_time_are_nonempty_asctime_shapes() {
        let (mut sess, id, cap) = seed("__DATE__\n__TIME__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0].kind, PpTokenKind::StringLit { .. }));
        assert!(matches!(out[1].kind, PpTokenKind::StringLit { .. }));
        let text = joined_text(&pp, &out);
        // `"Mmm dd yyyy"` is 13 bytes (`"Apr 23 2026"` etc) and
        // `"HH:MM:SS"` is 10 bytes; concatenated without whitespace
        // the pair is always 23 bytes regardless of the host clock.
        assert_eq!(text.len(), 23, "got {text:?}");
        assert!(text.starts_with('"'));
        assert!(text.ends_with('"'));
        // Colon positions inside the time literal are fixed.
        assert_eq!(&text[16..17], ":");
        assert_eq!(&text[19..20], ":");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn cli_define_installs_object_like_macro() {
        let mut opts = rcc_session::Options::default();
        opts.cli_defines.push(("FOO".to_string(), Some("42".to_string())));
        let (mut sess, id, cap) = seed_with_opts(opts, "FOO\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "42");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn cli_define_without_value_defaults_to_1() {
        let mut opts = rcc_session::Options::default();
        opts.cli_defines.push(("DEBUG".to_string(), None));
        let (mut sess, id, cap) = seed_with_opts(opts, "DEBUG\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "1");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn predefined_wins_over_colliding_cli_define() {
        // `-D __STDC__=0` must not override the predefined `__STDC__=1`.
        let mut opts = rcc_session::Options::default();
        opts.cli_defines.push(("__STDC__".to_string(), Some("0".to_string())));
        let (mut sess, id, _cap) = seed_with_opts(opts, "__STDC__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "1", "predefined must win over `-D`");
    }

    #[test]
    fn user_define_of_line_emits_e0027() {
        use rcc_errors::codes::E0027;
        let (mut sess, id, cap) = seed("#define __LINE__ 42\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0027 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0027));
    }

    #[test]
    fn user_undef_of_stdc_emits_e0027() {
        use rcc_errors::codes::E0027;
        let (mut sess, id, cap) = seed("#undef __STDC__\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0027 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0027));
    }

    #[test]
    fn predefined_macros_are_visible_in_table() {
        let (mut sess, id, _cap) = seed("int x;\n");
        let names: Vec<Symbol> = [
            "__STDC__",
            "__STDC_HOSTED__",
            "__STDC_VERSION__",
            "__DATE__",
            "__TIME__",
            "__FILE__",
            "__LINE__",
        ]
        .iter()
        .map(|n| sess.interner.intern(n))
        .collect();
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        for (sym, label) in names.iter().zip([
            "__STDC__",
            "__STDC_HOSTED__",
            "__STDC_VERSION__",
            "__DATE__",
            "__TIME__",
            "__FILE__",
            "__LINE__",
        ]) {
            let def = pp.macros.get(*sym).unwrap_or_else(|| panic!("{label} must be defined"));
            assert!(def.is_predefined, "{label} must carry is_predefined=true");
        }
    }
}
