//! `#include` header resolution (C99 §6.10.2).
//!
//! The search algorithm is intentionally simple and deterministic so the
//! driver surfaces predictable error messages:
//!
//! | Form            | Directories searched, in order                      |
//! |-----------------|-----------------------------------------------------|
//! | `#include "h"`  | current file directory, `-I`, rcc's resource include root, then system paths |
//! | `#include <h>`  | `-I`, rcc's resource include root, then system paths |
//! | hosted Linux    | same order; `lib/rcc/include` is restricted to compiler-owned headers, not libc/POSIX shims |
//!
//! The first existing file on that path wins; no file-system readdir is
//! performed. A header whose string resolves to an absolute path bypasses
//! the search entirely. Higher-level concerns — include-guard caching
//! (task 04-04) and `#pragma once` (task 04-05) — are layered on top of
//! this resolver and are deliberately out of scope here.

use std::path::{Path, PathBuf};
use std::process::Command;

use rcc_errors::{codes::E0021, Diagnostic, Label, Level};
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_session::{Arch, Environment, Options, Os, TargetInfo};
use rcc_span::{BytePos, FileId, Span};

use crate::guard::detect_guard;
use crate::macros::{MacroDef, MacroKind};
use crate::Preprocessor;

const MAX_INCLUDE_DEPTH: usize = 64;
// Permit bounded active self-reentry so macro-controlled self-inclusion still
// works. LibTomMath's `tommath_class.h` needs four active copies of the same
// file, and TinyCC's `95_bitfields.c` builds a deeper macro-controlled include
// tree before reaching its terminal `TEST == N` branches. Keep this budget well
// below the global include-depth limit so fuzz-discovered cycles still stop
// before they can grow the compiler stack substantially.
const MAX_SELF_INCLUDE_ACTIVE: usize = 16;

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

fn resolve_header_next_with<F>(
    name: &str,
    current_file: &Path,
    current_dir: &Path,
    include_paths: &[PathBuf],
    mut exists: F,
) -> (Option<PathBuf>, PathBuf)
where
    F: FnMut(&Path) -> bool,
{
    let as_path = Path::new(name);
    if as_path.is_absolute() {
        return (exists(as_path).then(|| as_path.to_path_buf()), current_dir.to_path_buf());
    }

    let (start, skipped) = include_next_start(name, current_file, current_dir, include_paths);

    for dir in include_paths.iter().skip(start) {
        let candidate = dir.join(as_path);
        if exists(&candidate) {
            return (Some(candidate), skipped);
        }
    }

    (None, skipped)
}

fn include_next_start(
    name: &str,
    current_file: &Path,
    current_dir: &Path,
    include_paths: &[PathBuf],
) -> (usize, PathBuf) {
    let as_path = Path::new(name);
    let matched_root = include_paths
        .iter()
        .position(|dir| same_path(&dir.join(as_path), current_file))
        .or_else(|| include_paths.iter().position(|dir| same_directory(dir, current_dir)));

    match matched_root {
        Some(idx) => (idx + 1, include_paths[idx].clone()),
        None => (0, current_dir.to_path_buf()),
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn same_directory(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
/// User `-I` entries come first so project-owned headers are never stolen by
/// rcc's resource directory. The compiler resource directory comes next because
/// headers such as `stddef.h`, `stdarg.h`, and `stdatomic.h` describe frontend
/// builtins and target lowering decisions. ABI-visible libc/POSIX/Linux headers
/// are intentionally not provided here; they must come from the host sysroot.
pub fn include_search_paths(user_paths: &[PathBuf], system_paths: &[PathBuf]) -> Vec<PathBuf> {
    let builtin = builtin_include_dir();
    let mut paths =
        Vec::with_capacity(user_paths.len() + system_paths.len() + usize::from(builtin.is_dir()));
    paths.extend(user_paths.iter().cloned());
    if builtin.is_dir() {
        paths.push(builtin);
    }
    paths.extend(system_paths.iter().cloned());
    paths
}

/// Complete include search path for a compilation session.
///
/// The policy is intentionally the same for hosted and non-hosted sessions:
/// project `-I`, then compiler-owned resource headers, then explicit/default
/// system paths. Hosted Linux relies on the host sysroot for libc/POSIX/Linux
/// headers; rcc's resource directory is not a libc overlay.
pub fn include_search_paths_for_options(opts: &Options) -> Vec<PathBuf> {
    include_search_paths(&opts.include_paths, &opts.system_include_paths)
}

/// Discover target-default system include directories.
///
/// The returned list contains only directories that exist on this host, so
/// production preprocessing does not waste probes on impossible paths. Tests use
/// the private candidate builder below to validate platform-specific path shapes
/// without depending on the executing machine.
pub fn discover_system_include_paths(target: &TargetInfo, sysroot: Option<&Path>) -> Vec<PathBuf> {
    system_include_candidates(target, sysroot).into_iter().filter(|path| path.is_dir()).fold(
        Vec::new(),
        |mut acc, path| {
            if !acc.contains(&path) {
                acc.push(path);
            }
            acc
        },
    )
}

fn system_include_candidates(target: &TargetInfo, sysroot: Option<&Path>) -> Vec<PathBuf> {
    match target.os {
        Os::Linux => linux_system_include_candidates(target, sysroot),
        Os::Darwin => darwin_system_include_candidates(sysroot),
        Os::Windows => windows_system_include_candidates(sysroot),
        Os::None => Vec::new(),
    }
}

fn linux_system_include_candidates(target: &TargetInfo, sysroot: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if sysroot.is_none() {
        if let Some(path) = host_compiler_include_dir() {
            paths.push(path);
        }
    }
    paths.push(rooted_unix_path(sysroot, "/usr/local/include"));
    paths.extend(
        linux_multiarch_include_names(target)
            .into_iter()
            .map(|name| rooted_unix_path(sysroot, &format!("/usr/include/{name}"))),
    );
    paths.push(rooted_unix_path(sysroot, "/usr/include"));
    paths
}

fn linux_multiarch_include_names(target: &TargetInfo) -> Vec<String> {
    let mut names = Vec::new();
    if matches!(target.env, Environment::Gnu) {
        let debian_name = match target.arch {
            Arch::X86_64 => Some("x86_64-linux-gnu"),
            Arch::Aarch64 => Some("aarch64-linux-gnu"),
            Arch::I386 => Some("i386-linux-gnu"),
        };
        if let Some(name) = debian_name {
            names.push(name.to_owned());
        }
    }
    names.push(target.triple.to_string());
    names
}

fn darwin_system_include_candidates(sysroot: Option<&Path>) -> Vec<PathBuf> {
    if let Some(root) = sysroot {
        return vec![root.join("usr").join("include")];
    }
    let mut paths = Vec::new();
    if let Some(sdk) = xcrun_sdk_path() {
        paths.push(sdk.join("usr").join("include"));
    }
    paths.push(PathBuf::from("/usr/local/include"));
    paths.push(PathBuf::from("/usr/include"));
    paths
}

fn windows_system_include_candidates(sysroot: Option<&Path>) -> Vec<PathBuf> {
    if let Some(root) = sysroot {
        return vec![root.join("include"), root.join("usr").join("include")];
    }
    env_include_paths()
        .into_iter()
        .chain([
            PathBuf::from(r"C:\msys64\ucrt64\include"),
            PathBuf::from(r"C:\msys64\mingw64\include"),
            PathBuf::from(r"C:\msys64\clang64\include"),
        ])
        .collect()
}

fn rooted_unix_path(sysroot: Option<&Path>, absolute: &str) -> PathBuf {
    let path = Path::new(absolute);
    match sysroot {
        Some(root) => root.join(path.strip_prefix("/").unwrap_or(path)),
        None => path.to_path_buf(),
    }
}

fn xcrun_sdk_path() -> Option<PathBuf> {
    let output = Command::new("xcrun").arg("--show-sdk-path").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn host_compiler_include_dir() -> Option<PathBuf> {
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(cc).arg("-print-file-name=include").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(path.trim());
    path.is_dir().then_some(path)
}

fn env_include_paths() -> Vec<PathBuf> {
    std::env::var_os("INCLUDE")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
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
        let include_paths = include_search_paths_for_options(&self.session.opts);

        let Some(resolved) =
            resolve_header_with(name, is_system, &current_dir, &include_paths, |path| {
                path.is_file() || self.session.has_virtual_file(path)
            })
        else {
            self.session.handler.emit(&cannot_find_header(directive_span, name, is_system));
            return Vec::new();
        };

        self.process_resolved_include(resolved, is_system, directive_span, current_file)
    }

    /// Resolve, load, and recursively preprocess a GNU `#include_next`.
    pub fn process_include_next(
        &mut self,
        header: &str,
        is_system: bool,
        directive_span: Span,
        current_file: FileId,
    ) -> Vec<PpToken> {
        let name = strip_header_delimiters(header);

        let (current_path, current_dir): (PathBuf, PathBuf) = {
            let sm = self.session.source_map.read().unwrap();
            let file = sm.file(current_file);
            (
                file.name.clone(),
                file.name.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
            )
        };
        let include_paths = include_search_paths_for_options(&self.session.opts);

        let (resolved, skipped_dir) =
            resolve_header_next_with(name, &current_path, &current_dir, &include_paths, |path| {
                path.is_file() || self.session.has_virtual_file(path)
            });
        let Some(resolved) = resolved else {
            self.session.handler.emit(&cannot_find_include_next(
                directive_span,
                name,
                &skipped_dir,
            ));
            return Vec::new();
        };

        self.process_resolved_include(resolved, is_system, directive_span, current_file)
    }

    fn process_resolved_include(
        &mut self,
        resolved: PathBuf,
        is_system: bool,
        directive_span: Span,
        current_file: FileId,
    ) -> Vec<PpToken> {
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

        if let Some(&active_count) = self.active_includes.get(&new_file) {
            let finite_self_include = current_file == new_file
                && active_count < MAX_SELF_INCLUDE_ACTIVE
                && self.mark_self_include_reentry(current_file);
            if !finite_self_include {
                let edge = (current_file, new_file);
                if !self.diagnosed_include_cycles.contains_key(&edge) {
                    self.session.handler.emit(&recursive_include_cycle(directive_span, &resolved));
                    self.diagnosed_include_cycles.insert(edge, ());
                }
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

        *self.active_includes.entry(new_file).or_insert(0) += 1;
        self.include_depth += 1;
        let tokens = self.run(new_file);
        self.include_depth -= 1;
        if let Some(count) = self.active_includes.get_mut(&new_file) {
            *count -= 1;
            if *count == 0 {
                self.active_includes.remove(&new_file);
            }
        }

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
             then command-line, compiler-resource, and system include paths; `<...>` skips the \
             current file directory"
            .into()],
        help: vec![
            "add the containing directory to the include path (e.g. `-I` or `-isystem`)".into()
        ],
    }
}

fn cannot_find_include_next(span: Span, name: &str, skipped_dir: &Path) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0021),
        message: format!("cannot find next header `{name}`"),
        labels: vec![Label {
            span,
            message: "GNU `#include_next` did not find a later matching header".into(),
            primary: true,
        }],
        notes: vec![format!(
            "`#include_next` starts after the include search directory `{}`",
            skipped_dir.display()
        )],
        help: vec![
            "add a later include directory containing the requested header or use ordinary \
             `#include` if the current directory should be searched"
                .into(),
        ],
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

fn recursive_include_cycle(span: Span, path: &Path) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0021),
        message: format!("recursive include cycle while loading `{}`", path.display()),
        labels: vec![Label {
            span,
            message: "this include target is already active in the include stack".into(),
            primary: true,
        }],
        notes: vec![
            "the preprocessor skipped this recursive edge so malformed include graphs do not \
             grow exponentially"
                .into(),
        ],
        help: vec!["add an include guard or `#pragma once` to the recursive header".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    use rcc_errors::{CaptureEmitter, Handler};
    use rcc_session::{Options, Session, TargetInfo, TargetTriple};
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
    fn include_search_paths_place_user_before_builtin_before_system_paths() {
        let user = PathBuf::from("__rcc_user_include__");
        let system = PathBuf::from("__rcc_system_include__");
        let paths =
            include_search_paths(std::slice::from_ref(&user), std::slice::from_ref(&system));

        let user_pos = paths.iter().position(|path| path == &user).expect("user include path");
        let builtin_pos = paths
            .iter()
            .position(|path| path == &builtin_include_dir())
            .expect("builtin include path");
        let system_pos =
            paths.iter().position(|path| path == &system).expect("system include path");
        assert!(user_pos < builtin_pos, "-I paths must precede compiler resource headers");
        assert!(
            builtin_pos < system_pos,
            "compiler resource headers must precede -isystem/default system paths"
        );
    }

    #[test]
    fn hosted_linux_include_search_keeps_compiler_resource_before_system_paths() {
        let user = PathBuf::from("__rcc_user_include__");
        let system = PathBuf::from("__rcc_system_include__");
        let opts = Options {
            include_paths: vec![user.clone()],
            system_include_paths: vec![system.clone()],
            linux_gnu_hosted: true,
            ..Options::default()
        };
        let paths = include_search_paths_for_options(&opts);

        let user_pos = paths.iter().position(|path| path == &user).expect("user include path");
        let system_pos =
            paths.iter().position(|path| path == &system).expect("system include path");
        let builtin_pos = paths
            .iter()
            .position(|path| path == &builtin_include_dir())
            .expect("builtin include path");
        assert!(user_pos < builtin_pos, "-I paths must still precede compiler headers");
        assert!(
            builtin_pos < system_pos,
            "compiler-owned headers must precede host system paths even in hosted mode"
        );
    }

    #[test]
    fn project_i_header_precedes_compiler_resource_for_system_form() {
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(inc_dir.path(), "stddef.h", "int user_stddef_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session(src_dir.path(), vec![inc_dir.path().to_path_buf()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<stddef.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("user_stddef_marker"), "project -I header must win: {text}");
        assert!(!text.contains("size_t"), "resource stddef.h must not steal project -I: {text}");
        assert!(cap.diagnostics().is_empty(), "project system include must not diagnose");
        let deps = sess.source_dependencies();
        assert!(
            deps.iter().any(|dep| dep.path == inc_dir.path().join("stddef.h")),
            "dependency trace must record the exact selected -I header: {deps:?}"
        );
    }

    #[test]
    fn quoted_current_dir_precedes_project_i_and_resource_headers() {
        let src_dir = TempDir::new().unwrap();
        let inc_dir = TempDir::new().unwrap();
        write_file(src_dir.path(), "stddef.h", "int local_stddef_marker;\n");
        write_file(inc_dir.path(), "stddef.h", "int user_stddef_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session(src_dir.path(), vec![inc_dir.path().to_path_buf()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("\"stddef.h\"", false, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("local_stddef_marker"), "quoted local header must win: {text}");
        assert!(!text.contains("user_stddef_marker"), "{text}");
        assert!(!text.contains("size_t"), "{text}");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn compiler_resource_header_precedes_host_system_header() {
        let src_dir = TempDir::new().unwrap();
        let sys_dir = TempDir::new().unwrap();
        write_file(sys_dir.path(), "stddef.h", "int host_stddef_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session_with_system_paths(src_dir.path(), Vec::new(), vec![sys_dir.path().into()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<stddef.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("size_t"), "resource stddef.h must define size_t: {text}");
        assert!(
            !text.contains("host_stddef_marker"),
            "compiler-owned stddef.h must shadow host system header: {text}"
        );
        assert!(cap.diagnostics().is_empty(), "resource system include must not diagnose");
        let deps = sess.source_dependencies();
        assert!(
            deps.iter().any(|dep| dep.path == builtin_include_dir().join("stddef.h")),
            "dependency trace must record exact resource header: {deps:?}"
        );
    }

    #[test]
    fn hosted_linux_finds_host_system_header_when_resource_root_does_not_own_it() {
        let src_dir = TempDir::new().unwrap();
        let sys_dir = TempDir::new().unwrap();
        let header = write_file(sys_dir.path(), "stdio.h", "int host_stdio_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session_with_system_paths(src_dir.path(), Vec::new(), vec![sys_dir.path().into()]);
        sess.opts.linux_gnu_hosted = true;
        let span = dummy_span(main_id);

        let tokens = Preprocessor::new(&mut sess).process_include("<stdio.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("host_stdio_marker"), "host system header must be found: {text}");
        assert!(cap.diagnostics().is_empty(), "hosted system include must not diagnose");
        assert!(
            sess.source_dependencies().iter().any(|dep| dep.path == header && dep.system),
            "dependency trace must record exact host system header"
        );
    }

    #[test]
    fn host_system_header_remains_discoverable_after_resource_layer() {
        let src_dir = TempDir::new().unwrap();
        let sys_dir = TempDir::new().unwrap();
        let header_path = write_file(sys_dir.path(), "host-only.h", "int host_only_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session_with_system_paths(src_dir.path(), Vec::new(), vec![sys_dir.path().into()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<host-only.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("host_only_marker"), "host-only system header must be found: {text}");
        assert!(cap.diagnostics().is_empty());
        assert!(
            sess.source_dependencies().iter().any(|dep| dep.path == header_path && dep.system),
            "dependency trace must record exact host system header"
        );
    }

    #[test]
    fn include_next_skips_current_include_dir() {
        let src_dir = TempDir::new().unwrap();
        let first_dir = TempDir::new().unwrap();
        let second_dir = TempDir::new().unwrap();
        write_file(first_dir.path(), "string.h", "#include_next <string.h>\nint from_first_dir;\n");
        write_file(second_dir.path(), "string.h", "int from_second_dir;\n");
        let (mut sess, main_id, cap) = seed_session(
            src_dir.path(),
            vec![first_dir.path().to_path_buf(), second_dir.path().to_path_buf()],
        );
        let span = dummy_span(main_id);

        let tokens =
            Preprocessor::new(&mut sess).process_include("<string.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("from_first_dir"), "wrapper header body should remain: {text}");
        assert!(
            text.contains("from_second_dir"),
            "include_next should resolve the later matching header: {text}"
        );
        assert_eq!(
            text.matches("from_first_dir").count(),
            1,
            "include_next must not recurse into the wrapper header: {text}"
        );
        assert!(cap.diagnostics().is_empty(), "include_next fixture should not diagnose");
    }

    #[test]
    fn include_next_skips_include_root_for_subdirectory_header() {
        let src_dir = TempDir::new().unwrap();
        let first_dir = TempDir::new().unwrap();
        let second_dir = TempDir::new().unwrap();
        fs::create_dir_all(first_dir.path().join("sys")).unwrap();
        fs::create_dir_all(second_dir.path().join("sys")).unwrap();
        write_file(
            &first_dir.path().join("sys"),
            "stat.h",
            "#include_next <sys/stat.h>\nint from_first_sys_stat;\n",
        );
        write_file(&second_dir.path().join("sys"), "stat.h", "int from_second_sys_stat;\n");
        let (mut sess, main_id, cap) = seed_session(
            src_dir.path(),
            vec![first_dir.path().to_path_buf(), second_dir.path().to_path_buf()],
        );
        let span = dummy_span(main_id);

        let tokens =
            Preprocessor::new(&mut sess).process_include("<sys/stat.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("from_first_sys_stat"), "{text}");
        assert!(
            text.contains("from_second_sys_stat"),
            "include_next should skip the include root, not the physical sys/ dir: {text}"
        );
        assert_eq!(
            text.matches("from_first_sys_stat").count(),
            1,
            "subdirectory include_next must not recurse into the wrapper header: {text}"
        );
        assert!(cap.diagnostics().is_empty(), "subdirectory include_next should not diagnose");
    }

    #[test]
    fn ordinary_include_keeps_first_include_dir() {
        let src_dir = TempDir::new().unwrap();
        let first_dir = TempDir::new().unwrap();
        let second_dir = TempDir::new().unwrap();
        write_file(first_dir.path(), "string.h", "int from_first_dir;\n");
        write_file(second_dir.path(), "string.h", "int from_second_dir;\n");
        let (mut sess, main_id, cap) = seed_session(
            src_dir.path(),
            vec![first_dir.path().to_path_buf(), second_dir.path().to_path_buf()],
        );
        let span = dummy_span(main_id);

        let tokens =
            Preprocessor::new(&mut sess).process_include("<string.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("from_first_dir"), "ordinary include should use first hit: {text}");
        assert!(
            !text.contains("from_second_dir"),
            "ordinary include must not skip ahead like include_next: {text}"
        );
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn missing_include_next_reports_skipped_directory() {
        let src_dir = TempDir::new().unwrap();
        let first_dir = TempDir::new().unwrap();
        write_file(first_dir.path(), "string.h", "#include_next <missing.h>\nint after;\n");
        let (mut sess, main_id, cap) =
            seed_session(src_dir.path(), vec![first_dir.path().to_path_buf()]);
        let span = dummy_span(main_id);

        let tokens =
            Preprocessor::new(&mut sess).process_include("<string.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("after"), "preprocessing should recover after missing include_next");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected one include_next diagnostic: {diags:?}");
        let diag = &diags[0];
        assert_eq!(diag.code, Some(E0021));
        assert!(diag.message.contains("missing.h"), "{diag:?}");
        assert!(
            diag.notes.iter().any(|note| note.contains("include_next")
                && note.contains(&first_dir.path().display().to_string())),
            "diagnostic should name the skipped directory: {diag:?}"
        );
    }

    #[test]
    fn linux_sysroot_candidates_are_prefixed_in_documented_order() {
        let target = TargetInfo::from_triple(&TargetTriple::new("x86_64-unknown-linux-gnu"))
            .expect("supported linux target");
        let root = PathBuf::from("/custom/root");
        let paths = system_include_candidates(&target, Some(&root));

        assert_eq!(
            paths,
            vec![
                root.join("usr").join("local").join("include"),
                root.join("usr").join("include").join("x86_64-linux-gnu"),
                root.join("usr").join("include").join("x86_64-unknown-linux-gnu"),
                root.join("usr").join("include"),
            ]
        );
    }

    #[test]
    fn linux_multiarch_candidates_include_debian_gnu_name_before_raw_triple() {
        let target = TargetInfo::from_triple(&TargetTriple::new("aarch64-unknown-linux-gnu"))
            .expect("supported linux target");
        let root = PathBuf::from("/custom/root");
        let paths = system_include_candidates(&target, Some(&root));

        let debian = root.join("usr").join("include").join("aarch64-linux-gnu");
        let raw = root.join("usr").join("include").join("aarch64-unknown-linux-gnu");
        let debian_pos = paths.iter().position(|path| path == &debian).expect("debian path");
        let raw_pos = paths.iter().position(|path| path == &raw).expect("raw triple path");
        assert!(debian_pos < raw_pos, "Debian multiarch path must precede raw LLVM triple path");
    }

    #[test]
    fn discover_system_include_paths_filters_missing_candidates() {
        let tmp = TempDir::new().unwrap();
        let usr_include = tmp.path().join("usr").join("include");
        fs::create_dir_all(&usr_include).unwrap();
        let target = TargetInfo::from_triple(&TargetTriple::new("x86_64-unknown-linux-gnu"))
            .expect("supported linux target");

        let paths = discover_system_include_paths(&target, Some(tmp.path()));

        assert_eq!(paths, vec![usr_include]);
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
        seed_session_with_system_paths(dir, include_paths, Vec::new())
    }

    fn seed_session_with_system_paths(
        dir: &Path,
        include_paths: Vec<PathBuf>,
        system_include_paths: Vec<PathBuf>,
    ) -> (Session, FileId, CaptureEmitter) {
        let main_path = write_file(dir, "main.c", "");
        let cap = CaptureEmitter::new();
        let opts = Options { include_paths, system_include_paths, ..Options::default() };
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
    fn process_include_uses_system_include_paths_for_system_form() {
        let src_dir = TempDir::new().unwrap();
        let sys_dir = TempDir::new().unwrap();
        write_file(sys_dir.path(), "sys-only.h", "int sys_marker;\n");
        let (mut sess, main_id, cap) =
            seed_session_with_system_paths(src_dir.path(), Vec::new(), vec![sys_dir.path().into()]);
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<sys-only.h>", true, span, main_id);

        assert!(!tokens.is_empty());
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn cli_include_paths_precede_system_include_paths_for_same_header() {
        let src_dir = TempDir::new().unwrap();
        let user_dir = TempDir::new().unwrap();
        let sys_dir = TempDir::new().unwrap();
        write_file(user_dir.path(), "shadow.h", "int user_shadow;\n");
        write_file(sys_dir.path(), "shadow.h", "int system_shadow;\n");
        let (mut sess, main_id, cap) = seed_session_with_system_paths(
            src_dir.path(),
            vec![user_dir.path().into()],
            vec![sys_dir.path().into()],
        );
        let span = dummy_span(main_id);
        let tokens =
            Preprocessor::new(&mut sess).process_include("<shadow.h>", true, span, main_id);
        let text = joined_token_text(&sess, &tokens);

        assert!(text.contains("user_shadow"), "{text}");
        assert!(!text.contains("system_shadow"), "{text}");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn process_include_finds_compiler_builtin_system_headers() {
        let src_dir = TempDir::new().unwrap();
        let (mut sess, main_id, cap) = seed_session(src_dir.path(), Vec::new());
        let span = dummy_span(main_id);
        let compiler_headers = ["stddef.h", "stdarg.h", "stdint.h", "stdbool.h", "iso646.h"];
        let token_headers = ["stddef.h", "stdarg.h", "stdint.h"];
        {
            let mut pp = Preprocessor::new(&mut sess);
            for header in compiler_headers {
                let tokens = pp.process_include(&format!("<{header}>"), true, span, main_id);
                if token_headers.contains(&header) {
                    assert!(!tokens.is_empty(), "compiler-owned {header} must contribute tokens");
                }
            }
        }

        assert!(cap.diagnostics().is_empty(), "compiler-owned include must not diagnose");
        let deps = sess.source_dependencies();
        for header in compiler_headers {
            assert!(
                deps.iter().any(|dep| dep.path.ends_with(Path::new(header)) && dep.system),
                "{header} must be recorded as a system dependency: {deps:?}"
            );
        }
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
        assert_eq!(diags.len(), 1, "expected one include-cycle diagnostic: {diags:?}");
        assert_eq!(diags[0].code, Some(E0021));
        assert!(diags[0].message.contains("recursive include cycle"));
    }

    #[test]
    fn immediate_self_include_without_macro_mutation_is_cut() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_vfs__");
        let main_path = root.join("main.c");
        let header_path = root.join("self.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"self.h\"\n"));
        sess.add_virtual_file(
            header_path.clone(),
            Arc::from("#include \"self.h\"\nint after_self_include;\n"),
        );
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);
        let text = joined_token_text(&sess, &out);

        assert_eq!(
            text.matches("after_self_include").count(),
            1,
            "plain self-inclusion must be cut before recursively duplicating the body: {text}"
        );
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected one include-cycle diagnostic: {diags:?}");
        assert!(diags[0].message.contains("recursive include cycle"));
    }

    #[test]
    fn branching_self_include_is_cut_before_exponential_growth() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_vfs__");
        let main_path = root.join("main.c");
        let header_path = root.join("test.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"test.h\"\n"));
        sess.add_virtual_file(
            header_path.clone(),
            Arc::from(
                "#include \"test.h\"\n#include \"test.h\"\nint after_branching_self_include;\n",
            ),
        );
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);
        let text = joined_token_text(&sess, &out);

        assert!(
            text.matches("after_branching_self_include").count() <= MAX_SELF_INCLUDE_ACTIVE,
            "branching self-include should be cut to a linear amount of output: {text}"
        );
        let diags = cap.diagnostics();
        assert!(
            !diags.is_empty()
                && diags.iter().all(|diag| diag.message.contains("recursive include cycle")),
            "expected only cycle diagnostics, got {diags:?}"
        );
    }

    #[test]
    fn finite_macro_controlled_self_include_is_allowed() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_self_include__");
        let main_path = root.join("main.c");
        let self_path = root.join("self.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"self.h\"\n"));
        sess.add_virtual_file(
            self_path.clone(),
            Arc::from(
                r#"
#if !defined(STEP)
#define STEP 0
#include "self.h"
#undef STEP
#define STEP 1
#include "self.h"
#elif STEP == 0
int first;
#elif STEP == 1
int second;
#endif
"#,
            ),
        );
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

        assert!(cap.diagnostics().is_empty(), "finite self-include must not diagnose");
        assert!(text.contains("intfirst;"), "first branch missing from {text}");
        assert!(text.contains("intsecond;"), "second branch missing from {text}");
    }

    #[test]
    fn libtommath_style_three_pass_self_include_is_allowed() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_libtommath_self_include__");
        let main_path = root.join("main.c");
        let class_path = root.join("tommath_class.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"tommath_class.h\"\n"));
        sess.add_virtual_file(
            class_path.clone(),
            Arc::from(
                r#"
#if !(defined(LTM1) && defined(LTM2) && defined(LTM3))
#define LTM_INSIDE
#if defined(LTM2)
#define LTM3
#endif
#if defined(LTM1)
#define LTM2
#endif
#define LTM1
int ltm_pass_marker;
#ifdef LTM_INSIDE
#undef LTM_INSIDE
#ifdef LTM3
#define LTM_LAST
#endif
#include "tommath_class.h"
#else
#define LTM_LAST
#endif
#else
int ltm_terminal_marker;
#endif
"#,
            ),
        );
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);
        let text = joined_token_text(&sess, &out);

        assert!(cap.diagnostics().is_empty(), "diagnostics: {:?}", cap.diagnostics());
        assert_eq!(
            text.matches("ltm_pass_marker").count(),
            3,
            "expected exactly three LibTomMath macro-state passes: {text}"
        );
        assert!(text.contains("ltm_terminal_marker"), "terminal branch missing from {text}");
    }

    #[test]
    fn recursive_virtual_include_graph_deduplicates_cycle_diagnostics() {
        let (mut sess, cap) = Session::for_test();
        let root = PathBuf::from("__rcc_fuzz_vfs__");
        let main_path = root.join("main.c");
        let test_path = root.join("test.h");
        let include1_path = root.join("include1.h");
        let include2_path = root.join("include2.h");
        sess.add_virtual_file(main_path.clone(), Arc::from("#include \"test.h\"\n"));
        sess.add_virtual_file(
            test_path.clone(),
            Arc::from("#include \"include1.h\"\nint test_header;\n#include \"include2.h\"\n"),
        );
        sess.add_virtual_file(
            include1_path.clone(),
            Arc::from("#include \"test.h\"\nint include1_header;\n#include \"include2.h\"\n"),
        );
        sess.add_virtual_file(
            include2_path.clone(),
            Arc::from("#include \"test.h\"\nint include2_header;\n"),
        );
        let main_id = sess.load_source_file(&main_path).expect("load virtual main");

        let out = Preprocessor::new(&mut sess).run(main_id);

        assert!(!out.is_empty(), "cycle recovery should keep non-include tokens");
        let diags = cap.diagnostics();
        assert!(
            (2..=3).contains(&diags.len()),
            "expected a small deduplicated diagnostic set, got {}: {diags:?}",
            diags.len()
        );
        assert!(
            diags.iter().all(|diag| diag.message.contains("recursive include cycle")),
            "all diagnostics should be cycle diagnostics: {diags:?}"
        );
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
