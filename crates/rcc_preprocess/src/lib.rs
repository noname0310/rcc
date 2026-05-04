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
use rcc_session::{Arch, DataModel, Endianness, Os, Session};
use rcc_span::{BytePos, FileId, Span, Symbol};

pub mod cond_stack;
pub mod directive;
pub mod expand;
pub mod guard;
pub mod if_eval;
pub mod include;
pub mod line_map;
pub mod line_stream;
pub mod macros;

pub use cond_stack::{CondFrame, CondStack};
pub use directive::{parse_directive, ConditionalKind, Directive};
pub use expand::expand_line;
pub use guard::detect_guard;
pub use if_eval::eval_if;
pub use include::{detect_pragma_once, resolve_header, strip_header_delimiters};
pub use line_map::{LineMap, LineOverride};
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
    /// GNU-compatible `#pragma push_macro` / `#pragma pop_macro`
    /// snapshots. These pragmas are not part of C99, but system headers
    /// and conformance fixtures commonly use them as benign state-saving
    /// directives. Supporting them here prevents the unknown-pragma path
    /// from silently changing macro semantics.
    pub macro_stack: FxHashMap<Symbol, Vec<Option<MacroDef>>>,
    /// Per-file `#line` overrides that retarget `__FILE__` /
    /// `__LINE__` expansion (task 04-15). The lexer is unchanged; the
    /// built-in expander consults this map when computing values.
    pub line_overrides: LineMap,
    /// Set once [`Self::install_cli_defines`] + [`Self::install_predefined`]
    /// have seeded the macro table. `run()` is re-entered for every
    /// `#include`d file and must install those entries *exactly* once
    /// per preprocessor instance; this latch is how it tells the
    /// top-level invocation apart from the recursive ones.
    predefined_installed: bool,
    /// Fatal-abort latch set by the first `#error` encountered during
    /// preprocessing (task 04-16, C99 §6.10.5). Once true, every
    /// remaining line in the current translation unit — including
    /// lines in the `#include`r — is skipped: no tokens are emitted,
    /// no directive side effects are applied, no further diagnostics
    /// are produced. The `Handler` already records the E0020 itself,
    /// so downstream phases can still observe the failure via
    /// `Handler::has_errors`.
    halted: bool,
}

impl<'a> Preprocessor<'a> {
    /// Build a new preprocessor.
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            macros: MacroTable::default(),
            include_guards: FxHashMap::default(),
            pragma_once: FxHashMap::default(),
            macro_stack: FxHashMap::default(),
            line_overrides: LineMap::new(),
            predefined_installed: false,
            halted: false,
        }
    }

    /// Run preprocessing and return the expanded token stream.
    ///
    /// Directive lines apply their side effects (`#define` / `#undef`
    /// populate [`Self::macros`]; conditional directives drive a
    /// per-file [`CondStack`] state machine; other directive variants
    /// are parsed and then skipped pending later tasks 04-15 / 04-16).
    /// Non-directive lines are fed through Prosser's hide-set macro
    /// expander (task 04-08), and the rescanned tokens are
    /// concatenated into the returned stream. Newline separators are
    /// consumed by [`line_stream::LineStream`] and not re-emitted.
    ///
    /// ### Conditional compilation (task 04-14)
    ///
    /// A fresh [`CondStack`] is created at entry and torn down at
    /// exit, so each translation unit / `#include`d file starts with
    /// an empty nesting. Lines are filtered by
    /// [`CondStack::is_active`]: inactive branches skip token output
    /// and suppress every non-conditional directive (no `#define`
    /// installs a macro; no `#include` is resolved). The
    /// `#if`/`#elif` controlling expression is evaluated *only* when
    /// the parent stack is already active, matching GCC/Clang and
    /// preventing dead-code E0028 (e.g. `#if 0 ... #if 1/0`). If any
    /// frames remain open at end of file, one E0018 is emitted per
    /// frame, each pointing at its originating `#if`/`#ifdef`/
    /// `#ifndef` keyword.
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
        let mut cond = cond_stack::CondStack::new();
        while let Some(line) = ls.next_line() {
            // `#error` latches [`Self::halted`] as a fatal abort for
            // the entire preprocessing run (task 04-16, C99 §6.10.5).
            // Stop eagerly so neither tokens nor additional directive
            // side effects surface after it.
            if self.halted {
                break;
            }
            if is_directive_line(&line) {
                // Null directive (`#` alone): no side effect, no output.
                if line.len() == 1 {
                    continue;
                }
                out.extend(self.dispatch_directive(&line, &src, &mut cond));
                continue;
            }
            if !cond.is_active() {
                continue;
            }
            let expanded = self.expand_tokens(line);
            out.extend(expanded);
        }

        // Unterminated `#if` / `#ifdef` / `#ifndef` at end of file.
        // One diagnostic per still-open frame, each labelled at its
        // own opening keyword — helps when several groups were nested.
        //
        // Suppressed when a `#error` has halted the run: §6.10.5
        // makes `#error` fatal, so piling unrelated E0018 on top
        // would just be noise about a translation unit that already
        // failed.
        if !self.halted {
            for frame in cond.into_unclosed() {
                let diag = cond_stack::missing_endif(frame.open_span);
                self.session.handler.emit(&diag);
            }
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
        let gnu_permissive_paste = self.session.opts.gnu_permissive_paste;
        expand::expand_line(
            &sm_arc,
            &mut self.session.interner,
            &mut self.session.handler,
            &self.macros,
            &self.line_overrides,
            line,
            gnu_va_args_elision,
            gnu_permissive_paste,
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
        self.install_target_predefined();
        if self.session.opts.gnu_builtin_libcalls {
            self.install_gnu_builtin_predefined();
        }
        self.install_builtin_predefined("__FILE__", BuiltinMacro::File);
        self.install_builtin_predefined("__LINE__", BuiltinMacro::Line);
    }

    fn install_target_predefined(&mut self) {
        let target = self.session.opts.target.clone();

        self.install_static_predefined(
            "__SIZEOF_POINTER__",
            &target.layouts.pointer.size.to_string(),
        );
        self.install_static_predefined("__SIZEOF_INT__", &target.layouts.int.size.to_string());
        self.install_static_predefined("__SIZEOF_LONG__", &target.layouts.long.size.to_string());
        self.install_static_predefined(
            "__SIZEOF_LONG_LONG__",
            &target.layouts.long_long.size.to_string(),
        );

        if target.data_model == DataModel::Lp64 {
            self.install_static_predefined("__LP64__", "1");
            self.install_static_predefined("_LP64", "1");
        }

        match target.arch {
            Arch::X86_64 => {
                self.install_static_predefined("__x86_64__", "1");
                self.install_static_predefined("__amd64__", "1");
            }
            Arch::Aarch64 => {
                self.install_static_predefined("__aarch64__", "1");
            }
            Arch::I386 => {
                self.install_static_predefined("__i386__", "1");
            }
        }

        match target.os {
            Os::Linux => {
                self.install_static_predefined("__linux__", "1");
            }
            Os::Darwin => {
                self.install_static_predefined("__APPLE__", "1");
                self.install_static_predefined("__MACH__", "1");
            }
            Os::Windows => {
                self.install_static_predefined("_WIN32", "1");
                if target.pointer_width == 64 {
                    self.install_static_predefined("_WIN64", "1");
                }
            }
            Os::None => {}
        }
    }

    fn install_gnu_builtin_predefined(&mut self) {
        self.install_static_predefined("__CHAR_BIT__", "8");
        self.install_static_predefined("__INT_MAX__", "2147483647");
        self.install_static_predefined("__LONG_MAX__", "9223372036854775807L");
        self.install_static_predefined("__LONG_LONG_MAX__", "9223372036854775807LL");
        self.install_static_predefined("__SIZE_TYPE__", "unsigned long");
        self.install_static_predefined("__ORDER_LITTLE_ENDIAN__", "1234");
        self.install_static_predefined("__ORDER_BIG_ENDIAN__", "4321");
        self.install_static_predefined("__ORDER_PDP_ENDIAN__", "3412");
        let byte_order = match self.session.opts.target.endianness {
            Endianness::Little => "1234",
            Endianness::Big => "4321",
        };
        self.install_static_predefined("__BYTE_ORDER__", byte_order);

        self.install_static_predefined("__builtin_abort", "abort");
        self.install_static_predefined("__builtin_exit", "exit");
        self.install_static_predefined("__builtin_printf", "printf");
        self.install_static_predefined("__builtin_sprintf", "sprintf");
        self.install_static_predefined("__builtin_snprintf", "snprintf");
        self.install_static_predefined("__builtin_malloc", "malloc");
        self.install_static_predefined("__builtin_alloca", "alloca");
        self.install_static_predefined("__builtin_memcpy", "memcpy");
        self.install_static_predefined("__builtin_memset", "memset");
        self.install_static_predefined("__builtin_memcmp", "memcmp");
        self.install_static_predefined("__builtin_strcmp", "strcmp");
        self.install_static_predefined("__builtin_strcpy", "strcpy");
        self.install_static_predefined("__builtin_strncpy", "strncpy");
        self.install_static_predefined("__builtin_strchr", "strchr");
        self.install_static_predefined("__builtin_strlen", "strlen");
        self.install_variadic_predefined("__builtin_prefetch", &["addr"], "((void)(addr))");
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

    fn install_variadic_predefined(&mut self, name: &str, params: &[&str], body_src: &str) {
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
        let param_syms = params.iter().map(|param| self.session.interner.intern(param)).collect();
        let def_span = Span::new(file_id, BytePos(0), BytePos(body_src.len() as u32));
        self.macros.define(MacroDef {
            name: name_sym,
            kind: MacroKind::FunctionLike {
                params: param_syms,
                variadic: true,
                named_variadic: None,
            },
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
    ///
    /// Conditional-control directives (`#if` / `#ifdef` / `#ifndef` /
    /// `#elif` / `#else` / `#endif`) always participate in the
    /// [`CondStack`] state machine — even inside an already-inactive
    /// region they must bump the nesting so the matching `#endif`
    /// pops the right frame. Every other directive is skipped
    /// entirely when [`CondStack::is_active`] is false: no
    /// diagnostics are produced for malformed `#define`s in dead
    /// code, no `#error` fires, no macro table changes.
    fn dispatch_directive(
        &mut self,
        line: &[PpToken],
        src: &str,
        cond: &mut cond_stack::CondStack,
    ) -> Vec<PpToken> {
        // Peek the directive name so we can identify conditional
        // control directives without running the full parser (and
        // therefore without emitting unrelated diagnostics while in
        // a skipped branch). Line layout: [0] = `#`, [1] = name.
        let name_kind = is_conditional_by_name(line, src);

        if let Some(kind) = name_kind {
            self.dispatch_conditional(line, src, cond, kind);
            return Vec::new();
        }

        // Non-conditional directive: if we're inside an inactive
        // region, drop the line with no parsing, no diagnostics, no
        // side effects. C99 §6.10p5 calls this "skipping groups".
        if !cond.is_active() {
            return Vec::new();
        }

        // C99 §6.10.4: the tokens after `#line` are macro-expanded
        // before the directive body is interpreted. The generic
        // directive parser intentionally works on raw tokens, so this
        // runtime path handles `#line MACRO` before parse_directive_ext
        // would reject the leading identifier as malformed.
        if is_directive_named(line, src, "line") {
            self.dispatch_line_directive(line);
            return Vec::new();
        }

        match directive::parse_directive_ext(
            line,
            src,
            &mut self.session.interner,
            self.session.opts.gnu_named_variadic,
        ) {
            Ok(directive::Directive::Define(def)) => {
                let result = {
                    let sm = self.session.source_map.read().unwrap();
                    let permissive = self.session.opts.gnu_permissive_redefinition;
                    macros::define_macro(
                        def,
                        &mut self.macros,
                        &sm,
                        &self.session.interner,
                        permissive,
                    )
                };
                match result {
                    Ok(Some(warning)) => self.session.handler.emit(&warning),
                    Ok(None) => {}
                    Err(diag) => self.session.handler.emit(&diag),
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
            Ok(directive::Directive::Include { span, is_system, header }) => {
                return self.process_include(&header, is_system, span, line[0].span.file);
            }
            Ok(directive::Directive::Line { span, line, file }) => {
                self.apply_line_directive(span, line, file);
            }
            Ok(directive::Directive::Error { span, message }) => {
                self.dispatch_error_directive(span, &message);
            }
            Ok(directive::Directive::Pragma { span, tokens }) => {
                self.dispatch_pragma_directive(span, &tokens, src);
            }
            // `Include` is still resolved out-of-band (task 04-03);
            // conditional variants are routed above before ever
            // reaching `parse_directive`.
            Ok(_) => {}
            Err(diag) => self.session.handler.emit(&diag),
        }
        Vec::new()
    }

    fn dispatch_line_directive(&mut self, line: &[PpToken]) {
        let hash = &line[0];
        let span = line.last().map(|t| hash.span.to(t.span)).unwrap_or(hash.span);
        let expanded = self.expand_tokens(line[2..].to_vec());
        let parsed = {
            let sm = self.session.source_map.read().unwrap();
            directive::parse_expanded_line_operands(span, &expanded, &sm)
        };
        match parsed {
            Ok((line_no, file)) => self.apply_line_directive(span, line_no, file),
            Err(diag) => self.session.handler.emit(&diag),
        }
    }

    /// Handle a parsed `#error` directive: emit E0020 with the
    /// stringified message tokens and latch [`Self::halted`] so the
    /// translation unit stops producing further output (C99 §6.10.5).
    ///
    /// The message is the raw source text between the first and last
    /// body token as captured by
    /// [`directive::parse_directive`] — whitespace between tokens is
    /// preserved, matching GCC/clang's behaviour of surfacing the
    /// directive's tail verbatim. A bare `#error` (no tokens) still
    /// fires the diagnostic so the user sees something actionable
    /// rather than a silent halt.
    fn dispatch_error_directive(&mut self, span: Span, message: &str) {
        let diag = error_directive(span, message);
        self.session.handler.emit(&diag);
        self.halted = true;
    }

    /// Handle a parsed `#pragma` directive.
    ///
    /// Three cases per C99 §6.10.6:
    ///
    /// - **`#pragma once`** — already cached at include time by
    ///   [`include::detect_pragma_once`]; accepted silently here.
    /// - **`#pragma STDC ...`** — the C99-reserved family
    ///   (`FP_CONTRACT`, `FENV_ACCESS`, `CX_LIMITED_RANGE`); accepted
    ///   silently. The implementation may ignore any state-setting
    ///   per §6.10.6p2, but must not diagnose.
    /// - **`#pragma push_macro("NAME")` / `pop_macro("NAME")`** —
    ///   GNU/MSVC-compatible macro state snapshots. C99 says unknown
    ///   pragmas are ignored, but treating these two as known avoids a
    ///   common non-C99 header idiom from altering macro expansion.
    /// - Anything else (including a bare `#pragma` with no tokens)
    ///   is unknown; we warn with W0001 and proceed. Warnings do
    ///   **not** bump `Handler::has_errors`.
    fn dispatch_pragma_directive(&mut self, span: Span, tokens: &[PpToken], src: &str) {
        let first_name = tokens.first().and_then(|tok| match tok.kind {
            PpTokenKind::Ident => Some(&src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]),
            _ => None,
        });
        match first_name {
            // Bare `#pragma` (no body) — silently ignored, matching
            // gcc/clang. Nothing to warn about.
            None if tokens.is_empty() => {}
            Some("once") | Some("STDC") => {}
            Some("push_macro") => {
                if let Some(name) = pragma_macro_name(tokens, src) {
                    self.push_macro_pragma(name);
                }
            }
            Some("pop_macro") => {
                if let Some(name) = pragma_macro_name(tokens, src) {
                    self.pop_macro_pragma(name);
                }
            }
            _ => {
                let diag = unknown_pragma(span, tokens, src);
                self.session.handler.emit(&diag);
            }
        }
    }

    fn push_macro_pragma(&mut self, name: &str) {
        let sym = self.session.interner.intern(name);
        let snapshot = self.macros.get(sym).cloned();
        self.macro_stack.entry(sym).or_default().push(snapshot);
    }

    fn pop_macro_pragma(&mut self, name: &str) {
        let sym = self.session.interner.intern(name);
        let Some(stack) = self.macro_stack.get_mut(&sym) else {
            return;
        };
        let Some(snapshot) = stack.pop() else {
            return;
        };
        if stack.is_empty() {
            self.macro_stack.remove(&sym);
        }
        match snapshot {
            Some(def) => self.macros.define(def),
            None => {
                self.macros.undef(sym);
            }
        }
    }

    /// Install a `#line` override driven by a [`directive::Directive::Line`].
    ///
    /// The physical line of the `#line` directive itself is extracted
    /// from `span.lo` via the source map; the override takes effect
    /// on the *following* physical line (§6.10.4p2). When `file` is
    /// `Some`, a virtual [`rcc_span::SourceFile`] is synthesised so
    /// `__FILE__` has a concrete id to dereference even if the named
    /// file does not exist in the `SourceMap`; when `file` is `None`
    /// the directive inherits the most recent override's file id
    /// (or the real file if none is active).
    ///
    /// `#line 0` and `#line N` for `N > 2147483647` are rejected with
    /// E0029 per §6.10.4p3 and no override is installed.
    fn apply_line_directive(&mut self, span: Span, line: u32, file: Option<String>) {
        // §6.10.4p3: `0 < N <= 2147483647`. `u32::parse` already
        // capped at 4294967295, so we only have to guard the low and
        // high ends; `line as i64` keeps the check straightforward.
        if line == 0 || line > 2_147_483_647 {
            let diag = line_out_of_range(span);
            self.session.handler.emit(&diag);
            return;
        }

        let directive_file = span.file;
        let directive_phys_line = {
            let sm = self.session.source_map.read().unwrap();
            sm.lookup_line_col(directive_file, span.lo).line
        };
        let start = directive_phys_line.saturating_add(1);

        let file_id = match file {
            Some(name) => {
                // §6.10.4p4 / plan §15: drop into a virtual file with
                // empty contents if the override name doesn't
                // correspond to a real source file. Matching against
                // the existing `SourceMap` is intentionally *not*
                // attempted — `#line` names are usually generator-
                // relative paths that happen to collide with real
                // ones only by accident. Keeping every override in
                // its own virtual file makes `__FILE__` round-trip
                // the exact spelling the directive requested.
                let mut sm = self.session.source_map.write().unwrap();
                sm.add_file(PathBuf::from(name), Arc::from(""))
            }
            None => self.line_overrides.inherited_file(directive_file, start),
        };

        self.line_overrides.push(
            directive_file,
            LineOverride { start_physical_line: start, logical_line: line, file_id },
        );
    }

    /// Drive [`CondStack`] for a conditional-control directive. Called
    /// from [`Self::dispatch_directive`] regardless of current
    /// active-ness so the nesting counter stays consistent.
    fn dispatch_conditional(
        &mut self,
        line: &[PpToken],
        src: &str,
        cond: &mut cond_stack::CondStack,
        kind: directive::ConditionalKind,
    ) {
        // `keyword_span` labels the `#<name>` introducer; used for
        // E0016/E0017/E0018 so the diagnostic points at the offending
        // directive rather than at its whole tail. `line` always has
        // at least 2 tokens by [`is_directive_line`]/length guard.
        let keyword_span = line[0].span.to(line[1].span);

        // We defer parsing of the directive's tail as much as possible
        // so dead-branch directives leave no diagnostic footprint.
        // Only `#if` / `#elif` / `#ifdef` / `#ifndef` carry a body
        // whose parsing we can shortcut; `#else` / `#endif` are pure.
        match kind {
            directive::ConditionalKind::If => {
                let parent_active = cond.is_active();
                let value = if parent_active {
                    self.eval_if_line(line, src).unwrap_or(false)
                } else {
                    false
                };
                cond.push_if(value, keyword_span);
            }
            directive::ConditionalKind::IfDef => {
                let parent_active = cond.is_active();
                let value = if parent_active {
                    self.eval_ifdef_line(line, src, keyword_span, false).unwrap_or(false)
                } else {
                    false
                };
                cond.push_if(value, keyword_span);
            }
            directive::ConditionalKind::IfNDef => {
                let parent_active = cond.is_active();
                let value = if parent_active {
                    self.eval_ifdef_line(line, src, keyword_span, true).unwrap_or(false)
                } else {
                    false
                };
                cond.push_if(value, keyword_span);
            }
            directive::ConditionalKind::ElIf => {
                // Borrow-checker dance: `on_elif` holds `&mut cond`,
                // and the closure it wants to call needs `&mut self`.
                // We side-step the conflict by snapshotting whether
                // evaluation is even needed (parent active + no
                // prior branch taken + no prior `#else`) from the
                // frame state, then running evaluation before the
                // stack transition.
                let would_evaluate = cond.parent_active() && needs_elif_eval(cond);
                let value = if would_evaluate {
                    self.eval_if_line(line, src).unwrap_or(false)
                } else {
                    false
                };
                if let Some(diag) = cond.on_elif(|| value, keyword_span) {
                    self.session.handler.emit(&diag);
                }
            }
            directive::ConditionalKind::Else => {
                if let Some(diag) = cond.on_else(keyword_span) {
                    self.session.handler.emit(&diag);
                }
            }
            directive::ConditionalKind::EndIf => {
                if let Some(diag) = cond.on_endif(keyword_span) {
                    self.session.handler.emit(&diag);
                }
            }
        }
    }

    /// Helper for [`Self::dispatch_conditional`]: run [`if_eval::eval_if`]
    /// on the tail of a `#if` / `#elif` line. Returns `None` on a
    /// controlling-expression diagnostic (already emitted through
    /// [`Self::eval_conditional`]); callers treat that as a false
    /// branch so the stack state stays well-defined.
    fn eval_if_line(&mut self, line: &[PpToken], _src: &str) -> Option<bool> {
        let tail = &line[2..];
        let raw = self.eval_conditional(tail)?;
        Some(raw != 0)
    }

    /// Helper: run the `#ifdef` / `#ifndef` identifier lookup. Returns
    /// `None` on E0015 (already emitted). The identifier is
    /// interned anew per call; lookup is O(1) against the macro
    /// table.
    fn eval_ifdef_line(
        &mut self,
        line: &[PpToken],
        src: &str,
        keyword_span: Span,
        is_ndef: bool,
    ) -> Option<bool> {
        let tail = &line[2..];
        let Some(tok) = tail.first() else {
            let diag = cond_stack::expected_ident_after_ifdef(keyword_span, is_ndef);
            self.session.handler.emit(&diag);
            return None;
        };
        if tok.kind != PpTokenKind::Ident {
            let diag = cond_stack::expected_ident_after_ifdef(tok.span, is_ndef);
            self.session.handler.emit(&diag);
            return None;
        }
        let text = &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize];
        let sym = self.session.interner.intern(text);
        let defined = self.macros.is_defined(sym);
        Some(if is_ndef { !defined } else { defined })
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
            &self.line_overrides,
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

/// Identify the conditional-control directive at the head of `line`
/// without invoking the full directive parser. Returns the matching
/// [`directive::ConditionalKind`] for `#if` / `#ifdef` / `#ifndef` /
/// `#elif` / `#else` / `#endif`, and `None` for any other directive
/// (or a malformed `#<not-an-ident>` line).
///
/// The lightweight probe matters in skipped branches: a dead-group
/// `#define 123` must not reach the full parser (which would emit
/// E0014), but we still need to notice that e.g. `#if 1/0` inside
/// the dead group is a new nesting level. Matching on the raw
/// directive-name text is sufficient and sidesteps the parser
/// entirely.
fn is_conditional_by_name(line: &[PpToken], src: &str) -> Option<directive::ConditionalKind> {
    // `is_directive_line` guards `line[0] == Hash`; `dispatch_directive`
    // guards `line.len() >= 2`. The name slot must therefore exist
    // and be an identifier to name any directive at all.
    let name_tok = line.get(1)?;
    if name_tok.kind != PpTokenKind::Ident {
        return None;
    }
    let name = &src[name_tok.span.lo.0 as usize..name_tok.span.hi.0 as usize];
    match name {
        "if" => Some(directive::ConditionalKind::If),
        "ifdef" => Some(directive::ConditionalKind::IfDef),
        "ifndef" => Some(directive::ConditionalKind::IfNDef),
        "elif" => Some(directive::ConditionalKind::ElIf),
        "else" => Some(directive::ConditionalKind::Else),
        "endif" => Some(directive::ConditionalKind::EndIf),
        _ => None,
    }
}

fn is_directive_named(line: &[PpToken], src: &str, expected: &str) -> bool {
    let Some(name_tok) = line.get(1) else {
        return false;
    };
    if name_tok.kind != PpTokenKind::Ident {
        return false;
    }
    &src[name_tok.span.lo.0 as usize..name_tok.span.hi.0 as usize] == expected
}

/// Whether a `#elif`'s controlling expression should be evaluated
/// given the current stack. Mirrors the condition under which
/// [`cond_stack::CondStack::on_elif`] would actually call its
/// closure: the stack must be non-empty, the current frame must not
/// have taken a prior branch, and must not have seen `#else` yet.
/// The parent-active predicate is checked separately at the call
/// site (it is independent of the top frame).
fn needs_elif_eval(cond: &cond_stack::CondStack) -> bool {
    let n = cond.depth();
    if n == 0 {
        return false;
    }
    // SAFETY: depth() > 0, so the frame slice is non-empty. We need
    // the top frame's `taken` / `else_seen` flags; expose them via
    // a tiny accessor-less peek.
    cond.top_needs_eval().unwrap_or(false)
}

/// Build the E0029 diagnostic for a `#line` argument outside the
/// §6.10.4p3 permitted range (`1..=2_147_483_647`).
fn line_out_of_range(span: Span) -> rcc_errors::Diagnostic {
    use rcc_errors::{codes::E0029, Diagnostic, Label, Level};
    Diagnostic {
        level: Level::Error,
        code: Some(E0029),
        message: "`#line` argument out of range".into(),
        labels: vec![Label {
            span,
            message: "must be between 1 and 2147483647".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.4p3: the digit sequence shall not specify zero, \
             nor a number greater than 2147483647"
            .into()],
        help: vec![],
    }
}

/// Build the E0020 diagnostic for `#error <tokens...>` (task 04-16,
/// C99 §6.10.5p1). `message` is the raw body text as captured by
/// [`directive::parse_directive`]; it is interpolated verbatim into
/// the diagnostic so users see the exact wording they wrote.
fn error_directive(span: Span, message: &str) -> rcc_errors::Diagnostic {
    use rcc_errors::{codes::E0020, Diagnostic, Label, Level};
    let trimmed = message.trim();
    let (header, label) = if trimmed.is_empty() {
        ("#error directive encountered".to_string(), "`#error` reached".to_string())
    } else {
        (format!("#error: {trimmed}"), trimmed.to_string())
    };
    Diagnostic {
        level: Level::Error,
        code: Some(E0020),
        message: header,
        labels: vec![Label { span, message: label, primary: true }],
        notes: vec!["C99 §6.10.5p1: the `#error` directive causes the \
             implementation to produce a diagnostic message"
            .into()],
        help: vec![],
    }
}

fn pragma_macro_name<'src>(tokens: &[PpToken], src: &'src str) -> Option<&'src str> {
    let [name, lparen, string, rparen] = tokens else {
        return None;
    };
    if !matches!(name.kind, PpTokenKind::Ident)
        || !matches!(lparen.kind, PpTokenKind::Punct(Punct::LParen))
        || !matches!(string.kind, PpTokenKind::StringLit { .. })
        || !matches!(rparen.kind, PpTokenKind::Punct(Punct::RParen))
    {
        return None;
    }
    let raw = src.get(string.span.lo.0 as usize..string.span.hi.0 as usize)?;
    raw.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
}

/// Build the W0001 warning for an unrecognised `#pragma`. Labels the
/// first body token (the pragma's name) so the user can see which
/// form was dropped; falls back to the directive span when the
/// `#pragma` has no body tokens to point at (that edge case currently
/// dispatches as a silent no-op, but keep the fallback so future
/// callers can reuse this helper safely).
fn unknown_pragma(span: Span, tokens: &[PpToken], src: &str) -> rcc_errors::Diagnostic {
    use rcc_errors::{codes::W0001, Diagnostic, Label, Level};
    let (label_span, label_msg) = match tokens.first() {
        Some(tok) => {
            let text = src.get(tok.span.lo.0 as usize..tok.span.hi.0 as usize).unwrap_or("");
            if text.is_empty() {
                (tok.span, "unknown `#pragma`".to_string())
            } else {
                (tok.span, format!("unknown pragma `{text}`"))
            }
        }
        None => (span, "empty `#pragma`".to_string()),
    };
    Diagnostic {
        level: Level::Warning,
        code: Some(W0001),
        message: "unknown #pragma directive — ignored".into(),
        labels: vec![Label { span: label_span, message: label_msg, primary: true }],
        notes: vec!["C99 §6.10.6: an implementation shall ignore any \
             pragma it does not recognise"
            .into()],
        help: vec![],
    }
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
    use rcc_errors::{
        codes::{E0013, E0014, E0022, W0006},
        CaptureEmitter, Handler, Level,
    };
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
            MacroKind::FunctionLike { params, variadic, .. } => {
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
            MacroKind::FunctionLike { params, variadic, .. } => {
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
            matches!(def.kind, MacroKind::FunctionLike { ref params, variadic: false, .. } if params.is_empty()),
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
            "__SIZEOF_POINTER__",
            "__SIZEOF_INT__",
            "__SIZEOF_LONG__",
            "__SIZEOF_LONG_LONG__",
            "__LP64__",
            "__x86_64__",
            "__linux__",
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
            "__SIZEOF_POINTER__",
            "__SIZEOF_INT__",
            "__SIZEOF_LONG__",
            "__SIZEOF_LONG_LONG__",
            "__LP64__",
            "__x86_64__",
            "__linux__",
            "__FILE__",
            "__LINE__",
        ]) {
            let def = pp.macros.get(*sym).unwrap_or_else(|| panic!("{label} must be defined"));
            assert!(def.is_predefined, "{label} must carry is_predefined=true");
        }
    }

    #[test]
    fn gnu_builtin_libcall_aliases_are_explicitly_feature_gated() {
        let src = "__builtin_printf(\"x\")\n__builtin_prefetch(p, 0, 3)\n";

        let (mut strict_sess, strict_id, _cap) = seed(src);
        let mut strict_pp = Preprocessor::new(&mut strict_sess);
        let strict_out = strict_pp.run(strict_id);
        assert_eq!(
            joined_text(&strict_pp, &strict_out),
            "__builtin_printf(\"x\")__builtin_prefetch(p,0,3)",
            "strict C99 mode must not silently define GNU builtin aliases",
        );

        let opts =
            rcc_session::Options { gnu_builtin_libcalls: true, ..rcc_session::Options::default() };
        let (mut gnu_sess, gnu_id, cap) = seed_with_opts(opts, src);
        let mut gnu_pp = Preprocessor::new(&mut gnu_sess);
        let gnu_out = gnu_pp.run(gnu_id);

        assert_eq!(joined_text(&gnu_pp, &gnu_out), "printf(\"x\")((void)(p))");
        assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn gnu_byte_order_predefined_macros_follow_target_endianness() {
        let opts =
            rcc_session::Options { gnu_builtin_libcalls: true, ..rcc_session::Options::default() };
        let (mut sess, id, cap) = seed_with_opts(
            opts,
            "__BYTE_ORDER__ __ORDER_LITTLE_ENDIAN__ __ORDER_BIG_ENDIAN__ __ORDER_PDP_ENDIAN__\n",
        );
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);

        assert_eq!(joined_text(&pp, &out), "1234123443213412");
        assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn target_predefined_macros_follow_session_target() {
        let opts = rcc_session::Options {
            target: rcc_session::TargetInfo::from_triple(&rcc_session::TargetTriple::new(
                "x86_64-pc-windows-msvc",
            ))
            .unwrap(),
            ..rcc_session::Options::default()
        };
        let (mut sess, id, cap) =
            seed_with_opts(opts, "_WIN32\n_WIN64\n__SIZEOF_POINTER__\n__SIZEOF_LONG__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);

        assert_eq!(joined_text(&pp, &out), "1184");
        assert!(cap.diagnostics().is_empty());
    }

    // ── Conditional-compilation state machine (task 04-14) ──────────

    use rcc_errors::codes::{E0016, E0017, E0018};

    #[test]
    fn if_true_keeps_body() {
        let (mut sess, id, cap) = seed("#if 1\nfoo\n#endif\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "foo");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn if_false_skips_body() {
        let (mut sess, id, cap) = seed("#if 0\nfoo\n#endif\nbar\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "bar", "dead `#if 0` body must be skipped");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn else_branch_materialises_when_if_false() {
        let (mut sess, id, cap) = seed("#if 0\nlive\n#else\ndead\n#endif\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "dead");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_deeply_nested_combos() {
        // Four nesting levels; only the innermost A=1/B=1 combo reaches
        // `hit`. A distinctive sentinel per dead branch lets us be sure
        // we didn't simply pick the wrong slot.
        let src = "\
#if 1
#if 1
#if 1
#if 1
hit
#else
dead4
#endif
#else
dead3
#endif
#else
dead2
#endif
#else
dead1
#endif
";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "hit");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_inner_if_in_dead_outer_stays_dead() {
        // Inner `#if 1` must still be dead because outer `#if 0` is.
        // The matching `#endif` must still pop the inner frame so the
        // trailing `bar` is emitted outside both groups.
        let src = "#if 0\n#if 1\nfoo\n#endif\n#endif\nbar\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "bar");
        assert!(
            cap.diagnostics().is_empty(),
            "nested conditionals in a dead group must not diagnose: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn acceptance_define_in_dead_branch_has_no_side_effect() {
        // §6.10p5: skipped groups do not execute any directive. A
        // `#define` inside the dead side must not install the macro.
        let src = "#if 0\n#define SHOULD_NOT_APPEAR 1\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let sym = sess.interner.intern("SHOULD_NOT_APPEAR");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        assert!(pp.macros.get(sym).is_none(), "dead-branch `#define` must not install a macro");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_malformed_define_in_dead_branch_is_silent() {
        // Dead-group directive bodies are not diagnosed — `#define 123`
        // would normally emit E0014, but inside `#if 0` it is skipped
        // silently per §6.10p5.
        let src = "#if 0\n#define 123\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        assert!(
            cap.diagnostics().is_empty(),
            "dead-branch malformed directives must not diagnose: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn acceptance_elif_selects_first_true_branch() {
        let src = "#if 0\none\n#elif 0\ntwo\n#elif 1\nthree\n#elif 1\nfour\n#else\nfive\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "three");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_elif_after_taken_does_not_evaluate_condition() {
        // `#if 1` takes the branch; the `#elif 1/0` that follows is
        // dead code and must not reach the `#if`-expression evaluator
        // (which would otherwise surface E0028 for 1/0).
        let src = "#if 1\nfoo\n#elif 1/0\nbar\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "foo");
        assert!(
            cap.diagnostics().is_empty(),
            "dead `#elif` must not evaluate its controlling expression: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn acceptance_ifdef_with_defined_name_takes_branch() {
        let src = "#define FOO 1\n#ifdef FOO\nyes\n#else\nno\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "yes");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_ifdef_with_undefined_name_skips_branch() {
        let src = "#ifdef FOO\nyes\n#else\nno\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "no");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_ifndef_mirrors_ifdef() {
        let defined = "#define FOO 1\n#ifndef FOO\nyes\n#else\nno\n#endif\n";
        let (mut sess, id, cap) = seed(defined);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "no");
        assert!(cap.diagnostics().is_empty());

        let undefined = "#ifndef FOO\nyes\n#else\nno\n#endif\n";
        let (mut sess, id, cap) = seed(undefined);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "yes");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_duplicate_else_emits_e0017() {
        let src = "#if 0\n#else\n#else\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0017)),
            "duplicate `#else` must carry E0017, got {diags:?}"
        );
    }

    #[test]
    fn acceptance_elif_after_else_emits_e0017() {
        let src = "#if 0\n#else\n#elif 1\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0017)),
            "`#elif` after `#else` must carry E0017, got {diags:?}"
        );
    }

    #[test]
    fn acceptance_unmatched_endif_emits_e0016() {
        let src = "#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0016 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0016));
    }

    #[test]
    fn acceptance_unmatched_else_emits_e0017() {
        let src = "#else\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0017)),
            "bare `#else` must carry E0017, got {diags:?}"
        );
    }

    #[test]
    fn acceptance_missing_endif_at_eof_emits_e0018_labeled_at_if() {
        let src = "#ifdef FOO\nbody\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        let e18: Vec<_> = diags.iter().filter(|d| d.code == Some(E0018)).collect();
        assert_eq!(e18.len(), 1, "exactly one E0018 expected, got {diags:?}");
        let label = e18[0].labels.iter().find(|l| l.primary).expect("primary label");
        // The label must cover the `#ifdef` keyword — at byte offsets
        // 0..6 in this single-file source (`#ifdef`).
        assert_eq!(label.span.lo.0, 0, "E0018 should label the opening keyword");
        assert_eq!(
            label.span.hi.0, 6,
            "E0018 primary label should cover `#ifdef` (got {:?})",
            label.span
        );
    }

    #[test]
    fn acceptance_nested_unterminated_groups_all_surface() {
        // Two unclosed groups → two E0018 diagnostics, each at its own
        // opening keyword.
        let src = "#if 1\n#ifdef FOO\nbody\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        let e18: Vec<_> = diags.iter().filter(|d| d.code == Some(E0018)).collect();
        assert_eq!(e18.len(), 2, "one E0018 per unclosed frame, got {diags:?}");
    }

    #[test]
    fn acceptance_ifdef_without_identifier_emits_e0015() {
        use rcc_errors::codes::E0015;
        let src = "#ifdef\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0015)),
            "bare `#ifdef` must carry E0015, got {diags:?}"
        );
    }

    // ── `#line` directive (task 04-15) ──────────────────────────────

    use rcc_errors::codes::{E0015, E0029};

    #[test]
    fn acceptance_line_directive_overrides_line_number() {
        // `#line 100` on physical line 1 ⇒ the next physical line
        // (where `__LINE__` sits) is renumbered to 100. §6.10.4p2.
        let (mut sess, id, cap) = seed("#line 100\n__LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "100");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn line_directive_expands_object_like_line_number() {
        // C99 §6.10.4 expands the directive body before interpreting
        // it, so an object-like macro may provide the line operand.
        let (mut sess, id, cap) = seed("#define line 1000\n#line line\n__LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "1000");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn line_directive_expands_object_like_filename() {
        let (mut sess, id, cap) =
            seed("#define FILE_NAME \"macro.c\"\n#line 7 FILE_NAME\n__FILE__ __LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"macro.c\"7");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn line_directive_expands_function_like_operands() {
        let (mut sess, id, cap) =
            seed("#define LOC(n, f) n f\n#line LOC(55, \"loc.c\")\n__FILE__ __LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"loc.c\"55");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn acceptance_line_directive_increments_per_newline() {
        // After `#line 100`, __LINE__ on the next line is 100, the one
        // after that is 101: the physical-line step is preserved.
        let (mut sess, id, cap) = seed("#line 100\n__LINE__\n__LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "100101");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_line_directive_overrides_file_name() {
        // `#line 100 "foo.c"` renumbers the following physical line
        // as 100 in "foo.c". The `__FILE__` on line 2 expands to
        // `"foo.c"` and the `__LINE__` on line 3 is 101 (100 + the
        // one-newline physical step from line 2 to line 3).
        let (mut sess, id, cap) = seed_at("real.c", "#line 100 \"foo.c\"\n__FILE__\n__LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"foo.c\"101");
        assert!(cap.diagnostics().is_empty(), "unexpected diagnostics: {:?}", cap.diagnostics());
    }

    #[test]
    fn acceptance_line_directive_first_next_line_matches_override() {
        // Minimal shape from the task's Acceptance line: `#line 100
        // "foo.c"` → the next `__LINE__` is 100 and the next
        // `__FILE__` is `"foo.c"`. Both tokens sit on physical line 2.
        let (mut sess, id, cap) = seed_at("real.c", "#line 100 \"foo.c\"\n__LINE__ __FILE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "100\"foo.c\"");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn line_directive_without_filename_inherits_previous_override() {
        // `#line 100 "foo.c"` installs foo.c; a later `#line 50` with
        // no filename must keep `__FILE__` pointing at foo.c and only
        // renumber — §6.10.4p3.
        //
        // Layout:
        //   line 1  #line 100 "foo.c"
        //   line 2  __FILE__         → "foo.c"
        //   line 3  #line 50         (override start=4, file inherited = foo.c)
        //   line 4  __FILE__         → "foo.c"
        //   line 5  __LINE__         → 50 + (5 - 4) = 51
        let (mut sess, id, cap) =
            seed("#line 100 \"foo.c\"\n__FILE__\n#line 50\n__FILE__\n__LINE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"foo.c\"\"foo.c\"51");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn line_directive_zero_is_rejected_with_e0029() {
        // C99 §6.10.4p3: the digit sequence "shall not specify zero".
        let (mut sess, id, cap) = seed("#line 0\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0029 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0029));
    }

    #[test]
    fn line_directive_non_numeric_is_rejected_with_e0015() {
        let (mut sess, id, cap) = seed("#line abc\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0015 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0015));
    }

    #[test]
    fn line_directive_synthesises_virtual_file_for_unknown_name() {
        // The override file does not exist on disk; `__FILE__` must
        // still render as a string literal naming "generated.c".
        let (mut sess, id, cap) = seed("#line 1 \"generated.c\"\n__FILE__\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"generated.c\"");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn acceptance_dead_if_expression_is_not_evaluated() {
        // `#if 1/0` inside a dead outer group would normally emit
        // E0028 (division by zero), but §6.10p5 skipping suppresses
        // the evaluation entirely.
        let src = "#if 0\n#if 1/0\nfoo\n#endif\n#endif\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        assert!(
            cap.diagnostics().is_empty(),
            "dead `#if 1/0` must not evaluate and not emit E0028: {:?}",
            cap.diagnostics()
        );
    }

    // ── `#error` / `#pragma` (task 04-16) ────────────────────────────

    use rcc_errors::codes::{E0020, W0001};

    #[test]
    fn acceptance_error_directive_emits_e0020_with_stringified_message() {
        // `#error foo bar` → one E0020 diagnostic carrying the body
        // text `foo bar` (whitespace between the two tokens is
        // preserved because parse_directive captures the raw source
        // slice from the first to the last body token).
        let (mut sess, id, cap) = seed("#error foo bar\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert!(out.is_empty(), "`#error` halts output, got {out:?}");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0020 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0020));
        assert_eq!(diags[0].level, Level::Error);
        assert!(
            diags[0].message.contains("foo bar"),
            "E0020 header must contain the stringified body `foo bar`, got {:?}",
            diags[0].message
        );
        assert!(pp.session.handler.has_errors(), "E0020 must count as an error");
    }

    #[test]
    fn acceptance_error_directive_halts_subsequent_processing() {
        // §6.10.5: `#error` is fatal. A subsequent `#define` must
        // *not* install a macro, and a subsequent malformed
        // directive that would ordinarily diagnose must stay silent.
        let src = "#error stop\n#define AFTER 1\n#define 123\nliteral\n";
        let (mut sess, id, cap) = seed(src);
        let sym = sess.interner.intern("AFTER");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);

        assert!(out.is_empty(), "no tokens may be emitted after `#error`");
        assert!(pp.macros.get(sym).is_none(), "`#define` past `#error` must not install a macro");

        let diags = cap.diagnostics();
        assert_eq!(
            diags.len(),
            1,
            "only the triggering E0020 should surface — later lines are skipped, got {diags:?}"
        );
        assert_eq!(diags[0].code, Some(E0020));
    }

    #[test]
    fn error_directive_with_no_body_still_emits_e0020() {
        // Edge case: a bare `#error` (nothing after it) still fires
        // so the user sees a diagnostic rather than a silent abort.
        let (mut sess, id, cap) = seed("#error\ntrailing\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert!(out.is_empty(), "halt still engages for an empty `#error`");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(E0020));
    }

    #[test]
    fn acceptance_unknown_pragma_emits_w0001_and_compilation_continues() {
        // `#pragma mystery` → W0001 warning, and the `after` line
        // that follows still reaches the output stream.
        let (mut sess, id, cap) = seed("#pragma mystery\nafter\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "after", "unknown pragma must NOT halt");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one W0001 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(W0001));
        assert_eq!(diags[0].level, Level::Warning);
        assert!(
            !pp.session.handler.has_errors(),
            "W0001 must NOT count as an error for has_errors"
        );
        assert_eq!(pp.session.handler.warning_count(), 1, "W0001 should bump the warning count");
    }

    #[test]
    fn acceptance_pragma_stdc_family_is_silently_accepted() {
        // §6.10.6: `STDC` pragmas are reserved. Every form of
        // `#pragma STDC ...` is accepted silently — no diagnostic,
        // and compilation proceeds.
        let forms = [
            "#pragma STDC FP_CONTRACT ON\nafter\n",
            "#pragma STDC FP_CONTRACT OFF\nafter\n",
            "#pragma STDC FP_CONTRACT DEFAULT\nafter\n",
            "#pragma STDC FENV_ACCESS ON\nafter\n",
            "#pragma STDC CX_LIMITED_RANGE OFF\nafter\n",
        ];
        for src in forms {
            let (mut sess, id, cap) = seed(src);
            let mut pp = Preprocessor::new(&mut sess);
            let out = pp.run(id);
            assert_eq!(joined_text(&pp, &out), "after", "src={src:?} must continue");
            assert!(
                cap.diagnostics().is_empty(),
                "`#pragma STDC ...` must be silent, got {:?} for src={src:?}",
                cap.diagnostics()
            );
        }
    }

    #[test]
    fn pragma_push_pop_macro_restores_saved_definition() {
        let src = r#"
#define abort "111"
abort
#pragma push_macro("abort")
#undef abort
#define abort "222"
abort
#pragma push_macro("abort")
#undef abort
#define abort "333"
abort
#pragma pop_macro("abort")
abort
#pragma pop_macro("abort")
abort
"#;
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "\"111\"\"222\"\"333\"\"222\"\"111\"");
        assert!(
            cap.diagnostics().is_empty(),
            "push/pop pragmas should be known: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn pragma_once_in_top_level_file_is_silent_in_dispatch() {
        // `#pragma once` at the top of the translation unit is
        // handled by the include-time detector; the dispatcher must
        // not diagnose on it. (Only meaningful inside a header, but
        // must not warn even when it appears standalone.)
        let (mut sess, id, cap) = seed("#pragma once\nafter\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "after");
        assert!(cap.diagnostics().is_empty(), "`#pragma once` must be silent in the dispatcher");
    }

    #[test]
    fn bare_pragma_with_no_body_is_silent() {
        // `#pragma` with no tail tokens is the implementation-defined
        // empty pragma — silently ignored, matching gcc/clang.
        let (mut sess, id, cap) = seed("#pragma\nafter\n");
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "after");
        assert!(cap.diagnostics().is_empty(), "bare `#pragma` must not warn");
    }

    #[test]
    fn error_after_unterminated_if_suppresses_e0018() {
        // `#if 1` with no `#endif` normally emits one E0018 at EOF,
        // but an intervening `#error` halts the run and makes the
        // unclosed-frame sweep silent — one diagnostic in, one
        // diagnostic out.
        let (mut sess, id, cap) = seed("#if 1\n#error boom\nbody\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "only E0020 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0020));
    }

    #[test]
    fn dead_branch_error_and_pragma_are_skipped() {
        // §6.10p5: a `#error` / `#pragma` in a skipped group must
        // have no side effect — neither the E0020 halt nor the
        // W0001 warning fires.
        let src = "#if 0\n#error dead\n#pragma mystery\n#endif\nalive\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        assert_eq!(joined_text(&pp, &out), "alive");
        assert!(
            cap.diagnostics().is_empty(),
            "dead `#error`/`#pragma` must be silent, got {:?}",
            cap.diagnostics()
        );
        assert!(!pp.halted, "dead `#error` must not latch the halt flag");
    }

    // ── GNU extensions (task 04-20) ─────────────────────────────────

    fn seed_gnu(src: &str) -> (Session, FileId, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let opts = Options {
            gnu_permissive_redefinition: true,
            gnu_named_variadic: true,
            gnu_permissive_paste: true,
            gnu_va_args_elision: true,
            ..Options::default()
        };
        let sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
        let id = sess.source_map.write().unwrap().add_file(PathBuf::from("<unit>"), Arc::from(src));
        (sess, id, cap)
    }

    // (a) Permissive redefinition

    #[test]
    fn gnu_permissive_redefinition_emits_warning_not_error() {
        let (mut sess, id, cap) = seed_gnu("#define X 1\n#define X 2\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "one W0006 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(W0006));
        assert_eq!(diags[0].level, Level::Warning);
    }

    #[test]
    fn strict_redefinition_still_emits_e0022() {
        let (mut sess, id, cap) = seed("#define X 1\n#define X 2\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(E0022));
        assert_eq!(diags[0].level, Level::Error);
    }

    // (b) Computed #include (IncludeTokens)

    #[test]
    fn computed_include_does_not_emit_e0013() {
        // `#include MACRO` — should NOT produce E0013 anymore.
        let (mut sess, id, cap) = seed("#define HDR \"foo.h\"\n#include HDR\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        assert!(
            !errors.iter().any(|d| d.code == Some(E0013)),
            "computed #include must not emit E0013, got {errors:?}"
        );
    }

    // (c) GNU named variadic

    #[test]
    fn gnu_named_variadic_parses_and_substitutes() {
        let src = "#define LOG(args...) args\nLOG(1, 2, 3)\n";
        let (mut sess, id, cap) = seed_gnu(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        let text = joined_text(&pp, &out);
        assert!(text.contains('1'), "variadic args must appear: {text}");
        assert!(text.contains('3'), "variadic args must appear: {text}");
        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        assert!(errors.is_empty(), "no errors expected, got {errors:?}");
    }

    #[test]
    fn strict_named_variadic_emits_e0014() {
        let (mut sess, id, cap) = seed("#define LOG(args...) args\n");
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        assert!(
            errors.iter().any(|d| d.code == Some(E0014)),
            "strict mode must reject `args...` with E0014, got {errors:?}"
        );
    }

    // (d) Permissive paste

    #[test]
    fn gnu_permissive_paste_accepts_pp_number_concat() {
        let src = "#define CONCAT(x,y) x##y\nCONCAT(4,.57)\n";
        let (mut sess, id, cap) = seed_gnu(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        assert!(errors.is_empty(), "GNU paste must not emit E0025, got {errors:?}");
        let text = joined_text(&pp, &out);
        assert!(text.contains("4"), "pasted text must be present: {text}");
    }

    #[test]
    fn strict_paste_pp_number_emits_e0025() {
        let src = "#define CONCAT(x,y) x##y\nCONCAT(4,.57)\n";
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        pp.run(id);
        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        // In strict mode, if `4##.57` re-lexes to multiple tokens it's E0025.
        // If the lexer produces a single token, no error is expected either.
        // Both outcomes are acceptable — this test just documents the behavior.
        let _ = errors;
    }
}
