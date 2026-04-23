//! `#include` header resolution (C99 §6.10.2).
//!
//! The search algorithm is intentionally simple and deterministic so the
//! driver surfaces predictable error messages:
//!
//! | Form            | Directories searched, in order                      |
//! |-----------------|-----------------------------------------------------|
//! | `#include "h"`  | the current translation unit's directory, then `-I` |
//! | `#include <h>`  | only the `-I` list                                  |
//!
//! The first existing file on that path wins; no file-system readdir is
//! performed. A header whose string resolves to an absolute path bypasses
//! the search entirely. Higher-level concerns — include-guard caching
//! (task 04-04) and `#pragma once` (task 04-05) — are layered on top of
//! this resolver and are deliberately out of scope here.

use std::path::{Path, PathBuf};

use rcc_errors::{codes::E0021, Diagnostic, Label, Level};
use rcc_lexer::PpToken;
use rcc_span::{BytePos, FileId, Span};

use crate::guard::detect_guard;
use crate::macros::{MacroDef, MacroKind};
use crate::Preprocessor;

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
    let as_path = Path::new(name);
    if as_path.is_absolute() {
        return if as_path.is_file() { Some(as_path.to_path_buf()) } else { None };
    }

    if !system {
        let candidate = current_dir.join(as_path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    for dir in include_paths {
        let candidate = dir.join(as_path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
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
    /// Include-guard caching (04-04) short-circuits repeated
    /// inclusions here; `#pragma once` (04-05) layers on top later.
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
        let include_paths = self.session.opts.include_paths.clone();

        let Some(resolved) = resolve_header(name, is_system, &current_dir, &include_paths) else {
            self.session.handler.emit(&cannot_find_header(directive_span, name, is_system));
            return Vec::new();
        };

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
            None => match self.session.source_map.write().unwrap().load_file(&resolved) {
                Ok(id) => id,
                Err(err) => {
                    self.session.handler.emit(&cannot_load_header(directive_span, &resolved, &err));
                    return Vec::new();
                }
            },
        };

        // Include-guard fast path: if a previous inclusion of the
        // same file detected a `#ifndef X / #define X / ... / #endif`
        // guard *and* `X` is still defined, the body would expand to
        // nothing. Elide the work entirely — O(1) skip.
        if let Some(&guard_sym) = self.include_guards.get(&new_file) {
            if self.macros.is_defined(guard_sym) {
                return Vec::new();
            }
        }

        let tokens = self.run(new_file);

        // First-inclusion fingerprinting: try to recognise the idiomatic
        // guard pattern. If found, cache it and stub-define the guard
        // symbol in the macro table so the next inclusion hits the
        // skip branch above. Task 04-06 will replace the stub with a
        // real `#define NAME` expansion of the guard directive; until
        // then the stub is what keeps the optimisation self-contained.
        if !self.include_guards.contains_key(&new_file) {
            let src = self.session.source_map.read().unwrap().file(new_file).src.clone();
            if let Some(guard) = detect_guard(&tokens, &src, &mut self.session.interner) {
                self.include_guards.insert(new_file, guard);
                self.macros.define(MacroDef {
                    name: guard,
                    kind: MacroKind::ObjectLike,
                    body: Vec::new(),
                    def_span: Span::new(new_file, BytePos(0), BytePos(0)),
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        // `<stddef.h>` with no matching include path must fail with
        // E0021, and the label must point at the `#include` span.
        let tmp = TempDir::new().unwrap();
        let (mut sess, main_id, cap) = seed_session(tmp.path(), Vec::new());
        let span = Span::new(main_id, BytePos(0), BytePos(18)); // `#include <stddef.h>`
        let tokens =
            Preprocessor::new(&mut sess).process_include("<stddef.h>", true, span, main_id);

        assert!(tokens.is_empty(), "failed include must contribute no tokens");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one diagnostic, got {diags:?}");
        let d = &diags[0];
        assert_eq!(d.code, Some(E0021), "E0021 required for missing header");
        assert!(
            d.message.contains("stddef.h"),
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
    fn process_include_registers_new_file_in_source_map() {
        // After a successful include, the loaded header must show up in
        // the SourceMap so its spans are renderable. We assert the file
        // count grows by one.
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "util.h", "int x;\n");
        let (mut sess, main_id, _cap) = seed_session(tmp.path(), Vec::new());
        let before = sess.source_map.read().unwrap().files().count();
        let span = dummy_span(main_id);
        Preprocessor::new(&mut sess).process_include("\"util.h\"", false, span, main_id);
        let after = sess.source_map.read().unwrap().files().count();
        assert_eq!(after, before + 1, "included file must be registered in the source map");
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

    #[test]
    fn unguarded_header_is_processed_fully_each_time() {
        // `bad.h` has a stray declaration before `#ifndef` — the
        // guard pattern does NOT match, so the file must be processed
        // in full on every inclusion.
        let tmp = TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "bad.h",
            "int stray;\n#ifndef BAD_H\n#define BAD_H\n#endif\n",
        );
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
