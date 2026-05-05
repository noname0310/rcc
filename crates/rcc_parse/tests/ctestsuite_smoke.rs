//! Smoke test: parse every file in `c-testsuite/tests/single-exec/`.
//!
//! For each `.c` file in the suite, we run `lex → preprocess → parse`
//! and assert that `rcc_parse::parse` returns `Some(TranslationUnit)`.
//! Files listed in `xfail.toml` are full-pipeline conformance xfails.
//! A conformance xfail may already parse successfully while still failing
//! HIR/typeck/CFG/codegen/runtime, so this smoke test treats parse-success
//! xfails as informative rather than failing the parser gate.
//!
//! The test is skipped (not failed) when the suite directory is absent.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcc_errors::{CaptureEmitter, Handler, Level};
use rcc_preprocess::preprocess;
use rcc_session::{Options, Session};
use serde::Deserialize;

// ── Paths ───────────────────────────────────────────────────────────

fn suite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("third_party")
        .join("testsuites")
        .join("c-testsuite")
}

fn single_exec_dir() -> PathBuf {
    suite_root().join("tests").join("single-exec")
}

// ── xfail ───────────────────────────────────────────────────────────

/// Schema matching `crates/rcc_conformance/src/xfail.rs`.
#[derive(Debug, Default, Deserialize)]
struct XFailFile {
    #[serde(default)]
    xfail: Vec<XFailEntry>,
}

#[derive(Debug, Deserialize)]
struct XFailEntry {
    id: String,
    #[allow(dead_code)]
    reason: String,
}

/// Load the xfail set from `<root>/xfail.toml`.
/// Returns file stems (e.g. "00055") extracted from ids like "c-testsuite::00055".
fn load_xfail(root: &Path) -> BTreeSet<String> {
    let path = root.join("xfail.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    let file: XFailFile = toml::from_str(&content).unwrap_or_else(|e| {
        panic!("failed to parse {}: {e}", path.display());
    });
    file.xfail
        .into_iter()
        .map(|entry| {
            // Strip suite prefix: "c-testsuite::00055" → "00055"
            entry.id.split("::").last().unwrap_or(&entry.id).to_string()
        })
        .collect()
}

// ── Pipeline ────────────────────────────────────────────────────────

/// Run lex→preprocess→parse on a single file.
/// Returns (parsed_ok, error_count). Catches panics so one file
/// doesn't abort the entire suite.
fn try_parse(path: &Path) -> (bool, usize) {
    let path = path.to_path_buf();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap.clone()));
        let mut sess = Session::with_handler(Options::default(), handler);
        let fid =
            sess.source_map.write().unwrap().add_file(path.to_path_buf(), Arc::from(src.as_str()));
        let pp_tokens = preprocess(&mut sess, fid);
        let ast = rcc_parse::parse(&mut sess, pp_tokens);

        let errors: Vec<_> =
            cap.diagnostics().into_iter().filter(|d| d.level == Level::Error).collect();
        (ast.is_some() && errors.is_empty(), errors.len())
    }));
    result.unwrap_or((false, 1))
}

// ── Test ────────────────────────────────────────────────────────────

#[test]
fn ctestsuite_parse_smoke() {
    let dir = single_exec_dir();
    if !dir.is_dir() {
        eprintln!("skipping: c-testsuite not vendored at {}", dir.display());
        return;
    }

    let xfail = load_xfail(&suite_root());

    // Discover .c files.
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    files.sort();

    assert!(!files.is_empty(), "no .c files found in {}", dir.display());

    let total = files.len();
    let mut pass = 0usize;
    let mut xfail_pass = 0usize;
    let mut unexpected_fail: Vec<String> = Vec::new();
    let mut xfail_parse_pass: Vec<String> = Vec::new();

    for file in &files {
        let stem = file.file_stem().unwrap().to_string_lossy().to_string();
        let (ok, error_count) = try_parse(file);

        if xfail.contains(&stem) {
            if ok {
                xfail_parse_pass.push(stem);
            } else {
                xfail_pass += 1;
            }
        } else if ok {
            pass += 1;
        } else {
            unexpected_fail.push(format!(
                "{} ({} error{})",
                stem,
                error_count,
                if error_count == 1 { "" } else { "s" }
            ));
        }
    }

    // Summary
    eprintln!();
    eprintln!("c-testsuite parse smoke: {pass}/{total} passed, {xfail_pass} xfail, {} unexpected failures",
        unexpected_fail.len());
    if !unexpected_fail.is_empty() {
        eprintln!();
        eprintln!("Unexpected failures:");
        for f in &unexpected_fail {
            eprintln!("  {f}");
        }
    }
    if !xfail_parse_pass.is_empty() {
        eprintln!();
        eprintln!("Conformance xfails that already parse successfully:");
        for f in &xfail_parse_pass {
            eprintln!("  {f}");
        }
    }

    // Assert: no unexpected parser failures. Full-pipeline xfails may be
    // parse-green while later compiler stages still need ownership tasks.
    assert!(
        unexpected_fail.is_empty(),
        "{} file(s) failed unexpectedly — add to xfail.toml or fix the parser:\n{}",
        unexpected_fail.len(),
        unexpected_fail.join("\n")
    );
}
