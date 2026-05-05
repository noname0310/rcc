//! `#include` header resolution (C99 §6.10.2).
//!
//! The search algorithm is intentionally simple and deterministic so the
//! driver surfaces predictable error messages:
//!
//! | Form            | Directories searched, in order                      |
//! |-----------------|-----------------------------------------------------|
//! | `#include "h"`  | the current translation unit's directory, `-I`, then rcc's builtin include root |
//! | `#include <h>`  | `-I`, then rcc's builtin include root              |
//!
//! The first existing file on that path wins; no file-system readdir is
//! performed. A header whose string resolves to an absolute path bypasses
//! the search entirely. Higher-level concerns — include-guard caching
//! (task 04-04) and `#pragma once` (task 04-05) — are layered on top of
//! this resolver and are deliberately out of scope here.

use std::path::{Path, PathBuf};

use rcc_errors::{codes::E0021, Diagnostic, Label, Level};
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{BytePos, FileId, Span};

use crate::guard::detect_guard;
use crate::macros::{MacroDef, MacroKind};
use crate::Preprocessor;

const MAX_INCLUDE_DEPTH: usize = 64;

/// Resolve `name` against C99 §6.10.2 search rules.
///
/// `name` is the header filename with the surrounding `"..."` / `<...>`
/// delimiters already stripped. `system = true` selects the `<...>` rule
/// (skip the current-file directory). `current_dir` is the directory
/// containing the source file that issued the `#include`; it is ignored
/// for the `<...>` form.
///
/// Returns the canonical on-disk path of the first matching file, or
/// `None` if no search entry resolves to an existing regular file.
///
/// Absolute paths are accepted verbatim (checked for existence), which
/// matches the behaviour of `gcc` and `clang` even though C99 does not
/// mandate it. Non-existent absolute paths still return `None`.
pub fn resolve_header(
    name: &str,
    system: bool,
    current_dir: &Path,
    include_paths: &[PathBuf],
) -> Option<PathBuf> {
    resolve_header_with(name, system, current_dir, include_paths, Path::is_file)
}

fn resolve_header_with<F>(
    name: &str,
    system: bool,
    current_dir: &Path,
    include_paths: &[PathBuf],
    mut exists: F,
) -> Option<PathBuf>
where
    F: FnMut(&Path) -> bool,
{
    let as_path = Path::new(name);
    if as_path.is_absolute() {
        return if exists(as_path) { Some(as_path.to_path_buf()) } else { None };
    }

    if !system {
        let candidate = current_dir.join(as_path);
        if exists(&candidate) {
            return Some(candidate);
        }
    }

    for dir in include_paths {
        let candidate = dir.join(as_path);
        if exists(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Compiler-provided C header root.
///
/// This is deliberately part of the preprocessor, not the parser smoke tests:
/// once ordinary `#include` dispatch is active, standard headers must resolve
/// through the same path production users will exercise.
pub fn builtin_include_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("lib")
        .join("rcc")
        .join("include")
}

/// Complete include search path for system-header probes.
///
/// The compiler resource directory comes first so freestanding shims such as
/// `stddef.h` and `stdarg.h` resolve before host system headers. User-provided
/// `-I` entries still handle project headers and non-resource headers after the
/// compiler-owned surface.
pub fn include_search_paths(user_paths: &[PathBuf]) -> Vec<PathBuf> {
    let builtin = builtin_include_dir();
    let mut paths = Vec::with_capacity(user_paths.len() + usize::from(builtin.is_dir()));
    if builtin.is_dir() {
        paths.push(builtin);
    }
    paths.extend(user_paths.iter().cloned());
    paths
}

/// Return `true` if `tokens` (a header's full pp-token stream, as
/// produced by `rcc_lexer::tokenize`) contains a `#pragma once`
/// directive anywhere in the file.
///
/// Unlike the `#ifndef` header-guard shape, `#pragma once` carries no
/// "covers the whole file" requirement: a single occurrence at any
/// position marks the file as include-once. The match criterion is a
/// `#` token that starts a logical line, followed immediately by the
/// identifiers `pragma` then `once`; trailing tokens on the same line
/// are permitted and ignored (matching clang/gcc behaviour).
pub fn detect_pragma_once(tokens: &[PpToken], src: &str) -> bool {
    for window in tokens.windows(3) {
        let (hash, pragma, once) = (&window[0], &window[1], &window[2]);
        if !(matches!(hash.kind, PpTokenKind::Punct(Punct::Hash)) && hash.at_line_start) {
            continue;
        }
        if pragma.kind != PpTokenKind::Ident || once.kind != PpTokenKind::Ident {
            continue;
        }
        if slice_text(pragma, src) == "pragma" && slice_text(once, src) == "once" {
            return true;
        }
    }
    false
}

fn slice_text<'a>(tok: &PpToken, src: &'a str) -> &'a str {
    &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]
}

/// Strip the surrounding `<...>` or `"..."` delimiters from a raw
/// `Directive::Include::header` value. Returns the inner filename; if no
/// recognisable delimiter pair is present, returns the input unchanged.
///
/// The preprocessor's directive parser stores the header as the raw
/// source substring (`<stdio.h>` or `"util.h"`); [`resolve_header`] wants
/// just `stdio.h` / `util.h`.
pub fn strip_header_delimiters(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'<' && last == b'>') || (first == b'"' && last == b'"') {
            return &raw[1..raw.len() - 1];
        }
    }
    raw
}

impl Preprocessor<'_> {
    /// Resolve, load, and recursively preprocess an `#include` directive.
    ///
    /// `header` is the raw header text from `Directive::Include::header`
    /// (delimiters included); `is_system` selects `<...>` vs `"..."`;
    /// `directive_span` labels the failing `#include` line on an E0021
    /// diagnostic; `current_file` is the file id the directive was
    /// issued from (used to derive the current directory for the
    /// `"..."` search rule).
    ///
    /// Returns the expanded token stream contributed by the included
    /// file. When resolution fails an E0021 diagnostic is emitted and
    /// the token stream is empty; this matches the standard's "one
    /// diagnostic, translation unit is ill-formed" model without
    /// abandoning the parent file.
    ///
    /// Recursion currently calls back into [`Preprocessor::run`], which
    /// is still a pass-through tokeniser in the overall M5 schedule
    /// (tasks 04-06..04-14 populate the directive loop itself).
    /// Include-guard caching (04-04) and `#pragma once` caching
    /// (04-05) both short-circuit repeated inclusions here.
    pub fn process_include(
        &mut self,
        header: &str,
        is_system: bool,
        directive_span: Span,
        current_file: FileId,
    ) -> Vec<PpToken> {
        let name = strip_header_delimiters(header);

        let current_dir: PathBuf = {
            let sm = self.session.source_map.read().unwrap();
            sm.file(current_file)
                .name
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        };
        let include_paths = include_search_paths(&self.session.opts.include_paths);

        let Some(resolved) =
            resolve_header_with(name, is_system, &current_dir, &include_paths, |path| {
                path.is_file() || self.session.has_virtual_file(path)
            })
        else {
            self.session.handler.emit(&cannot_find_header(directive_span, name, is_system));
            return Vec::new();
        };
        self.session.record_source_dependency(resolved.clone(), is_system);

        // Dedupe on the resolved path: if this header was already
        // loaded during a prior `#include`, reuse its existing
        // `FileId` so the include-guard cache (keyed by `FileId`) is
        // stable across repeat inclusions. `SourceMap::load_file`
        // always appends a fresh entry, so this check is what makes
        // the guard optimisation reachable at all.
        let existing = self
            .session
            .source_map
            .read()
            .unwrap()
            .files()
            .find(|f| f.name == resolved)
            .map(|f| f.id);
        let new_file = match existing {
            Some(id) => id,
            None => match self.session.load_source_file(&resolved) {
                Ok(id) => id,
                Err(err) => {
                    self.session.handler.emit(&cannot_load_header(directive_span, &resolved, &err));
                    return Vec::new();
                }
            },
        };

        // `#pragma once` fast path: a previous inclusion marked this
        // file as include-once, so any further inclusion is an
        // unconditional no-op. Checked ahead of the `#ifndef` guard
        // fast path because the pragma is cheaper — no macro-table
        // lookup — and semantically non-overridable (there is no
        // `#undef` for it).
        if self.pragma_once.contains_key(&new_file) {
            return Vec::new();
        }

        // Include-guard fast path: if a previous inclusion of the
        // same file detected a `#ifndef X / #define X / ... / #endif`
        // guard *and* `X` is still defined, the body would expand to
        // nothing. Elide the work entirely — O(1) skip.
        if let Some(&guard_sym) = self.include_guards.get(&new_file) {
            if self.macros.is_defined(guard_sym) {
                return Vec::new();
            }
        }

        if self.include_depth >= MAX_INCLUDE_DEPTH {
            self.session.handler.emit(&include_depth_exceeded(
                directive_span,
                &resolved,
                MAX_INCLUDE_DEPTH,
            ));
            return Vec::new();
        }

        self.active_includes.insert(new_file, ());
        self.include_depth += 1;
        let tokens = self.run(new_file);
        self.include_depth -= 1;
        self.active_includes.remove(&new_file);

        // First-inclusion fingerprinting: scan for both the
        // `#pragma once` marker and the canonical `#ifndef` guard
        // shape. The two are deliberately independent — headers in
        // the wild commonly use `#pragma once` *and* a traditional
        // `#ifndef` guard for portability — and either, alone, is
        // sufficient to short-circuit the next inclusion. The source
        // buffer is cloned once and reused by both detectors.
        //
        // Detection runs against a *raw* re-tokenisation of the file,
        // not against `tokens`: since task 04-08 macro-expands and
        // consumes directive lines inside `run`, the returned stream
        // no longer contains `#pragma` / `#ifndef` punctuator shapes.
        // The detectors inspect directive shape, so they must see the
        // pre-expansion token stream.
        let need_pragma_scan = !self.pragma_once.contains_key(&new_file);
        let need_guard_scan = !self.include_guards.contains_key(&new_file);
        if need_pragma_scan || need_guard_scan {
            let src = self.session.source_map.read().unwrap().file(new_file).src.clone();
            let raw: Vec<PpToken> = rcc_lexer::tokenize(new_file, &src).collect();
            if need_pragma_scan && detect_pragma_once(&raw, &src) {
                self.pragma_once.insert(new_file, ());
            }
            if need_guard_scan {
                if let Some(guard) = detect_guard(&raw, &src, &mut self.session.interner) {
                    self.include_guards.insert(new_file, guard);
                    // Stub-define the guard symbol in the macro table so
                    // the next inclusion hits the skip branch above.
                    // Task 04-06 will replace the stub with a real
                    // `#define NAME` expansion of the guard directive.
                    self.macros.define(MacroDef {
                        name: guard,
                        kind: MacroKind::ObjectLike,
                        body: Vec::new(),
                        def_span: Span::new(new_file, BytePos(0), BytePos(0)),
                        is_predefined: false,
                    });
                }
            }
        }

        tokens
    }
}

fn cannot_find_header(span: Span, name: &str, is_system: bool) -> Diagnostic {
    let form = if is_system { "<...>" } else { "\"...\"" };
    Diagnostic {
        level: Level::Error,
        code: Some(E0021),
        message: format!("cannot find header `{name}`"),
        labels: vec![Label {
            span,
            message: format!("{form} header not found in any search path"),
            primary: true,
        }],
        notes: vec!["C99 §6.10.2: `\"...\"` searches the current file's directory first, \
             then the command-line include paths; `<...>` searches only the \
             include paths"
            .into()],
        help: vec!["add the containing directory to the include path (e.g. `-I`)".into()],
    }
}

fn cannot_load_header(span: Span, path: &Path, err: &std::io::Error) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0021),
        message: format!("failed to read header `{}`", path.display()),
        labels: vec![Label {
            span,
            message: format!("I/O error while loading include target: {err}"),
            primary: true,
        }],
        notes: vec![],
        help: vec![],
    }
}

fn include_depth_exceeded(span: Span, path: &Path, limit: usize) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0021),
        message: format!("include nesting exceeds {limit} while loading `{}`", path.display()),
        labels: vec![Label {
            span,
            message: "this include would exceed the maximum include nesting depth".into(),
            primary: true,
        }],
        notes: vec![
            "finite conditional self-inclusion is allowed, but unbounded include cycles must be \
             stopped before they overflow the compiler stack"
                .into(),
        ],
        help: vec!["break the include cycle or add a guard around the recursive include".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    use rcc_errors::{CaptureEmitter, Handler};
    use rcc_session::{Options, Session};
    use rcc_span::BytePos;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).expect("write test header");
        path
    }

    // ── resolve_header ──────────────────────────────────────────────

    #[test]
    fn local_form_finds_header_next_to_current_file() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "util.h", "int util;\n");
        let hit = resolve_header("util.h", false, tmp.path(), &[]);
        assert_eq!(hit.as_deref(), Some(tmp.path().join("util.h").as_path()));
    }

    #[test]
    fn local_form_falls_back_to_include_paths() {
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(inc_dir.path(), "lib.h", "int lib;\n");
        let hit = resolve_header("lib.h", false, src_dir.path(), &[inc_dir.path().to_path_buf()]);
        assert_eq!(hit.as_deref(), Some(inc_dir.path().join("lib.h").as_path()));
    }

    #[test]
    fn local_form_prefers_current_dir_over_include_paths() {
        // Two files named `shared.h` — in the current directory and on an
        // include path. §6.10.2 requires the current-dir copy to win for
        // the `"..."` form.
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        let local = write_file(src_dir.path(), "shared.h", "int local;\n");
        write_file(inc_dir.path(), "shared.h", "int remote;\n");
        let hit =
            resolve_header("shared.h", false, src_dir.path(), &[inc_dir.path().to_path_buf()]);
        assert_eq!(hit.as_deref(), Some(local.as_path()));
    }

    #[test]
    fn system_form_ignores_current_dir() {
        // `<...>` must skip the current-dir lookup even if a matching
        // file sits right there.
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(src_dir.path(), "sys.h", "int wrong;\n");
        let expected = write_file(inc_dir.path(), "sys.h", "int right;\n");
        let hit = resolve_header("sys.h", true, src_dir.path(), &[inc_dir.path().to_path_buf()]);
        assert_eq!(hit.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn system_form_without_matching_path_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert!(resolve_header("stddef.h", true, tmp.path(), &[]).is_none());
    }

    #[test]
    fn include_paths_searched_in_order() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        // Both paths contain `pick.h`; the first one listed must win.
        let expected = write_file(first.path(), "pick.h", "int first;\n");
        write_file(second.path(), "pick.h", "int second;\n");
        let cwd = TempDir::new().unwrap();
        let hit = resolve_header(
            "pick.h",
            true,
            cwd.path(),
            &[first.path().to_path_buf(), second.path().to_path_buf()],
        );
        assert_eq!(hit.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn absolute_path_bypasses_search() {
        let tmp = TempDir::new().unwrap();
        let abs = write_file(tmp.path(), "abs.h", "int abs;\n");
        let abs_str = abs.to_string_lossy().into_owned();
        // Unrelated directories as the "current dir" / include paths
        // prove they are never consulted for absolute names.
        let other = TempDir::new().unwrap();
        let hit = resolve_header(&abs_str, false, other.path(), &[]);
        assert_eq!(hit.as_deref(), Some(abs.as_path()));
    }

    #[test]
    fn missing_file_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert!(resolve_header("nope.h", false, tmp.path(), &[]).is_none());
        assert!(resolve_header("nope.h", true, tmp.path(), &[tmp.path().to_path_buf()]).is_none());
    }

    // ── strip_header_delimiters ─────────────────────────────────────

    #[test]
    fn strip_removes_angle_brackets() {
        assert_eq!(strip_header_delimiters("<stdio.h>"), "stdio.h");
    }

    #[test]
    fn strip_removes_double_quotes() {
        assert_eq!(strip_header_delimiters("\"util.h\""), "util.h");
    }

    #[test]
    fn strip_leaves_bare_name_alone() {
        assert_eq!(strip_header_delimiters("bare.h"), "bare.h");
        assert_eq!(strip_header_delimiters(""), "");
        assert_eq!(strip_header_delimiters("<unclosed"), "<unclosed");
    }

    // ── process_include end-to-end ──────────────────────────────────

    /// Build a session with a capture emitter, seed it with `main.c` in
    /// `dir`, return `(session, main_file_id, capture)`. The `main.c`
    /// itself is empty — tests only care about include resolution from
    /// its location.
    fn seed_session(dir: &Path, include_paths: Vec<PathBuf>) -> (Session, FileId, CaptureEmitter) {
        let main_path = write_file(dir, "main.c", "");
        let cap = CaptureEmitter::new();
        let opts = Options { include_paths, ..Options::default() };
        let sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
        let main_id = sess.source_map.write().unwrap().load_file(&main_path).expect("load main.c");
        (sess, main_id, cap)
    }

    fn dummy_span(file: FileId) -> Span {
        Span::new(file, BytePos(0), BytePos(0))
    }

    fn joined_token_text(sess: &Session, tokens: &[PpToken]) -> String {
        tokens
            .iter()
            .map(|tok| {
                let sm = sess.source_map.read().unwrap();
                let file = sm.file(tok.span.file);
                file.src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn acceptance_local_header_resolves_and_includes() {
        // main.c in tmp/ includes "util.h" sitting next to it. Must
        // resolve and its token stream must surface through process_include.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "util.h", "int util_marker;\n");
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("\"util.h\"", false, span, main_id);

        // `util.h` tokenises to at least an `int` keyword → not empty.
        assert!(!tokens.is_empty(), "included tokens must be forwarded to caller");
        assert!(cap.diagnostics().is_empty(), "successful include must emit no diagnostics");
    }

    #[test]
    fn acceptance_missing_system_header_emits_e0021() {
        // A non-existent `<...>` header with no matching include path must fail with
        // E0021, and the label must point at the `#include` span.
        let tmp = TempDir::new().unwrap();
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = Span::new(main_id, BytePos(0), BytePos(31)); // `#include <rcc-missing-test.h>`
        let tokens = Preprocessor::new(&mut sess).process_include(
            "<rcc-missing-test.h>",
            true,
            span,
            main_id,
        );

        assert!(tokens.is_empty(), "failed include must contribute no tokens");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one diagnostic, got {diags:?}");
        let d = &diags[0];
        assert_eq!(d.code, Some(E0021), "E0021 required for missing header");
        assert!(
            d.message.contains("rcc-missing-test.h"),
            "diagnostic message should name the missing header, got {:?}",
            d.message
        );
        let primary = d.labels.iter().find(|l| l.primary).expect("E0021 has a primary label");
        assert_eq!(primary.span, span, "primary label must point at the #include line");
    }

    #[test]
    fn process_include_uses_cli_include_paths_for_system_form() {
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(inc_dir.path(), "ext.h", "int ext_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session(src_dir.path(), vec![inc_dir.path().to_path_buf()]);
        let span = dummy_span(main_id);
        let tokens = Preprocessor::new(&mut sess).process_include("<ext.h>", true, span, main_id);

        assert!(!tokens.is_empty());
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn process_include_finds_compiler_builtin_system_headers() {
        let src_dir = TempDir::new().unwrap();
        let (mut sess, main_id, cap) = seed_session(src_dir.path(), Vec::new());
        let span = dummy_span(main_id);
        {
            let mut pp = Preprocessor::new(&mut sess);
            for header in ["stddef.h", "stdio.h", "math.h", "ctype.h"] {
                let tokens = pp.process_include(&format!("<{header}>"), true, span, main_id);
                assert!(!tokens.is_empty(), "builtin {header} must contribute tokens");
            }
        }

        assert!(cap.diagnostics().is_empty(), "builtin system include must not diagnose");
        let deps = sess.source_dependencies();
        for header in ["stddef.h", "stdio.h", "math.h", "ctype.h"] {
            assert!(
                deps.iter().any(|dep| dep.path.ends_with(Path::new(header)) && dep.system),
                "{header} must be recorded as a system dependency: {deps:?}"
            );
        }
    }

    #[test]
    fn compiler_builtin_headers_precede_cli_include_paths_for_system_form() {
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(inc_dir.path(), "stddef.h", "int user_stddef_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session(src_dir.path(), vec![inc_dir.path().to_path_buf()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<stddef.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("size_t"), "builtin stddef.h must define size_t: {text}");
        assert!(
            !text.contains("user_stddef_marker"),
            "compiler resource header must win over same-named -I system header: {text}"
        );
        assert!(cap.diagnostics().is_empty(), "builtin system include must not diagnose");
    }

    #[test]
    fn process_include_resolves_session_virtual_files() {
        let (mut sess, _cap) = Session::for_test();
        let root = PathBuf::from("__rcc_vfs__");
        let main_path = root.join("main.c");
        let header_path = root.join("virtual.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"virtual.h\"\n"));
        sess.add_virtual_file(header_path.clone(), Arc::from("int from_virtual;\n"));
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);
        let text = out
            .iter()
            .map(|tok| {
                let sm = sess.source_map.read().unwrap();
                let file = sm.file(tok.span.file);
                file.src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string()
            })
            .collect::<String>();

        assert_eq!(text, "intfrom_virtual;");
        assert!(sess.source_dependencies().iter().any(|dep| dep.path == header_path));
    }

    #[test]
    fn recursive_virtual_include_is_diagnosed_without_stack_overflow() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_vfs__");
        let main_path = root.join("main.c");
        let header_path = root.join("loop.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"loop.h\"\n"));
        sess.add_virtual_file(header_path.clone(), Arc::from("#include \"loop.h\"\nint after;\n"));
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);

        assert!(!out.is_empty(), "tokens after the recursive include should still recover");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected one include-depth diagnostic: {diags:?}");
        assert_eq!(diags[0].code, Some(E0021));
        assert!(diags[0].message.contains("include nesting exceeds"));
    }

    #[test]
    fn process_include_registers_new_file_in_source_map() {
        // After a successful include, the loaded header must show up
        // in the SourceMap so its spans are renderable. Since task
        // 04-12 `run()` also seeds the map with synthetic files for
        // the C99 §6.10.8 predefined macros (`__STDC__` et al.), we
        // assert on the presence of the *included* path by name
        // rather than on the raw file count.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "util.h", "int x;\n");
        let (mut sess, main_id, _cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        Preprocessor::new(&mut sess).process_include("\"util.h\"", false, span, main_id);
        let expected = tmp.path().join("util.h");
        let registered = sess.source_map.read().unwrap().files().any(|f| f.name == expected);
        assert!(registered, "included file {expected:?} must be registered in the source map");
    }

    // ── 04-04 include-guard optimisation ────────────────────────────

    #[test]
    fn guarded_header_is_skipped_on_repeat_include() {
        // Canonical guard shape: first inclusion yields tokens and
        // populates `include_guards`; a second inclusion of the same
        // file must return an empty token stream without re-lexing.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "ok.h", "#ifndef OK_H\n#define OK_H\nint ok;\n#endif\n");
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let first = pp.process_include("\"ok.h\"", false, span, main_id);
        assert!(!first.is_empty(), "first inclusion must deliver the header tokens");
        assert_eq!(pp.include_guards.len(), 1, "guard must be cached after first inclusion");

        let second = pp.process_include("\"ok.h\"", false, span, main_id);
        assert!(second.is_empty(), "guarded re-include must contribute no tokens");
        assert!(cap.diagnostics().is_empty(), "skipping must not produce diagnostics");
    }

    // ── 04-05 `#pragma once` ────────────────────────────────────────

    #[test]
    fn pragma_once_header_skipped_on_repeat_include() {
        // Acceptance: a header marked `#pragma once`, included twice,
        // contributes tokens the first time and zero tokens the second
        // time. Verified via token count so the assertion mirrors the
        // `--emit=pp` byte-count check named in the task spec.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "once.h", "#pragma once\nint once_marker;\n");
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let first = pp.process_include("\"once.h\"", false, span, main_id);
        assert!(!first.is_empty(), "first inclusion must deliver the header tokens");
        assert_eq!(pp.pragma_once.len(), 1, "`#pragma once` must be cached on first inclusion");

        let second = pp.process_include("\"once.h\"", false, span, main_id);
        assert!(
            second.is_empty(),
            "repeat include of a `#pragma once` header must contribute no tokens"
        );
        assert!(cap.diagnostics().is_empty(), "skipping must not produce diagnostics");
    }

    #[test]
    fn pragma_once_skipped_across_two_caller_files() {
        // Fixture per task spec: header is included from two different
        // caller files; the second inclusion is elided regardless of
        // which file issued it.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "once.h", "#pragma once\nint once_marker;\n");
        let sibling_path = write_file(tmp.path(), "sibling.c", "");
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let sibling_id =
            sess.source_map.write().unwrap().load_file(&sibling_path).expect("load sibling.c");
        let mut pp = Preprocessor::new(&mut sess);

        let first = pp.process_include("\"once.h\"", false, dummy_span(main_id), main_id);
        assert!(!first.is_empty(), "first caller sees the header tokens");

        let second = pp.process_include("\"once.h\"", false, dummy_span(sibling_id), sibling_id);
        assert!(
            second.is_empty(),
            "a different caller including the same `#pragma once` header must skip"
        );
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn pragma_once_coexists_with_ifndef_guard() {
        // Acceptance: `#pragma once` and an explicit `#ifndef` guard
        // in the same file must not conflict. The file has both; the
        // second inclusion is still a no-op. Our `#pragma once` sits
        // before the `#ifndef`, which disqualifies the canonical
        // guard shape — exercising the independence of the two
        // caches.
        let tmp = TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "combo.h",
            "#pragma once\n#ifndef COMBO_H\n#define COMBO_H\nint combo;\n#endif\n",
        );
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let first = pp.process_include("\"combo.h\"", false, span, main_id);
        assert!(!first.is_empty());
        assert_eq!(pp.pragma_once.len(), 1, "`#pragma once` must be cached");

        let second = pp.process_include("\"combo.h\"", false, span, main_id);
        assert!(
            second.is_empty(),
            "combined `#pragma once` + `#ifndef` header must still skip on re-include"
        );
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn pragma_once_and_guard_caches_both_populated_when_both_shapes_match() {
        // When the `#pragma once` sits *inside* the `#ifndef` guard,
        // both detectors fire. The pragma cache must win the fast
        // path because it short-circuits unconditionally.
        let tmp = TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "both.h",
            "#ifndef BOTH_H\n#define BOTH_H\n#pragma once\nint both;\n#endif\n",
        );
        let (mut sess, main_id, _cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let _ = pp.process_include("\"both.h\"", false, span, main_id);
        assert_eq!(pp.pragma_once.len(), 1, "pragma-once cache populated");
        assert_eq!(pp.include_guards.len(), 1, "include-guard cache populated");
    }

    #[test]
    fn header_without_pragma_once_is_not_cached() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "plain.h", "int plain_marker;\n");
        let (mut sess, main_id, _cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let _ = pp.process_include("\"plain.h\"", false, span, main_id);
        assert!(pp.pragma_once.is_empty(), "only files with `#pragma once` should be cached");
    }

    // ── detect_pragma_once (unit) ───────────────────────────────────

    #[test]
    fn detect_pragma_once_matches_canonical_form() {
        let src = "#pragma once\nint x;\n";
        let toks: Vec<PpToken> = rcc_lexer::tokenize(FileId(0), src).collect();
        assert!(detect_pragma_once(&toks, src));
    }

    #[test]
    fn detect_pragma_once_anywhere_in_file() {
        // Mid-file occurrence still qualifies — unlike `#ifndef`
        // guards, `#pragma once` has no positional requirement.
        let src = "int x;\n#pragma once\nint y;\n";
        let toks: Vec<PpToken> = rcc_lexer::tokenize(FileId(0), src).collect();
        assert!(detect_pragma_once(&toks, src));
    }

    #[test]
    fn detect_pragma_once_rejects_other_pragmas() {
        let src = "#pragma pack(1)\nint x;\n";
        let toks: Vec<PpToken> = rcc_lexer::tokenize(FileId(0), src).collect();
        assert!(!detect_pragma_once(&toks, src));
    }

    #[test]
    fn detect_pragma_once_requires_hash_at_line_start() {
        // `x # pragma once` puts `#` mid-line — not a directive.
        let src = "x # pragma once\n";
        let toks: Vec<PpToken> = rcc_lexer::tokenize(FileId(0), src).collect();
        assert!(!detect_pragma_once(&toks, src));
    }

    #[test]
    fn detect_pragma_once_empty_file_is_false() {
        assert!(!detect_pragma_once(&[], ""));
    }

    #[test]
    fn unguarded_header_is_processed_fully_each_time() {
        // `bad.h` has a stray declaration before `#ifndef` — the
        // guard pattern does NOT match, so the file must be processed
        // in full on every inclusion.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "bad.h", "int stray;\n#ifndef BAD_H\n#define BAD_H\n#endif\n");
        let (mut sess, main_id, _cap) = seed_session(tmp.path(), Vec::new());
        let span = dummy_span(main_id);
        let mut pp = Preprocessor::new(&mut sess);

        let first = pp.process_include("\"bad.h\"", false, span, main_id);
        assert!(pp.include_guards.is_empty(), "non-guard shape must not be cached");
        assert!(!first.is_empty());

        let second = pp.process_include("\"bad.h\"", false, span, main_id);
        assert_eq!(
            first.len(),
            second.len(),
            "unguarded header must produce identical token counts on each inclusion"
        );
    }
}
