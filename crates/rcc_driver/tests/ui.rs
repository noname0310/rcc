//! UI tests: golden `.stderr` fixtures for user-facing parse errors.
//!
//! Convention: each `.c` file under `tests/ui/parse/` has a sibling `.stderr`
//! file that records the exact diagnostic output. The runner compares
//! byte-for-byte; set `UPDATE_EXPECT=1` to regenerate expectations.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use rcc_errors::{CaptureEmitter, Handler, StderrEmitter};
use rcc_session::{Options, Session};

/// Strip ANSI escape sequences so snapshots are portable.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip `ESC [ ... final_byte` CSI sequences.
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch.is_ascii_alphabetic() || ch == 'm' {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Normalize Windows paths to forward slashes so fixtures work cross-platform.
fn normalize_paths(s: &str) -> String {
    s.replace('\\', "/")
}

/// Run the rcc pipeline (lex → preprocess → parse) on `src_path`, returning
/// the rendered diagnostic output with colours stripped.
fn compile_and_capture(src_path: &Path) -> String {
    let sm = Arc::new(RwLock::new(rcc_span::SourceMap::new()));
    let capture = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(capture.clone()));
    let opts = Options::default();
    let mut session = Session::with_handler(opts, handler);
    // Share the source map so the StderrEmitter can resolve spans.
    session.source_map = sm.clone();

    // 1. Load the file.
    let file = session
        .source_map
        .write()
        .unwrap()
        .load_file(src_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", src_path.display()));

    // 2. Preprocess.
    let pp_tokens = rcc_preprocess::preprocess(&mut session, file);

    // 3. Parse.
    let _ast = rcc_parse::parse(&mut session, pp_tokens);

    // 4. Render every captured diagnostic through StderrEmitter (no colour).
    let emitter = StderrEmitter::new(sm).with_color(false);
    let diags = capture.diagnostics();
    let mut rendered = String::new();
    for d in &diags {
        rendered.push_str(&emitter.render_to_string(d));
    }

    normalize_paths(&strip_ansi(&rendered))
}

/// Discover all `.c` fixture files under `dir`.
fn discover_fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|ext| ext == "c") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    files.sort();
    files
}

#[test]
fn ui_parse() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("ui").join("parse");

    let fixtures = discover_fixtures(&fixture_dir);
    assert!(!fixtures.is_empty(), "no .c fixtures found in {}", fixture_dir.display());

    let update = std::env::var("UPDATE_EXPECT").is_ok();
    let mut failures = Vec::new();

    for c_file in &fixtures {
        eprintln!("[ui] processing: {}", c_file.display());
        let stderr_file = c_file.with_extension("stderr");
        let actual = compile_and_capture(c_file);

        if update {
            std::fs::write(&stderr_file, &actual)
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", stderr_file.display()));
            continue;
        }

        let expected = match std::fs::read_to_string(&stderr_file) {
            Ok(s) => normalize_paths(&s),
            Err(e) => {
                failures.push(format!(
                    "{}: missing .stderr file (run with UPDATE_EXPECT=1 to create)\n  error: {e}",
                    c_file.display()
                ));
                continue;
            }
        };

        if actual != expected {
            failures.push(format!(
                "{}: stderr mismatch\n--- expected ({})\n+++ actual\n{}",
                c_file.display(),
                stderr_file.display(),
                diff_strings(&expected, &actual),
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{count} UI test(s) failed:\n\n{details}",
            count = failures.len(),
            details = failures.join("\n\n")
        );
    }
}

/// Simple line-by-line diff for readable failure output.
fn diff_strings(expected: &str, actual: &str) -> String {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    let max = exp_lines.len().max(act_lines.len());
    for i in 0..max {
        let e = exp_lines.get(i).copied().unwrap_or("<missing>");
        let a = act_lines.get(i).copied().unwrap_or("<missing>");
        if e != a {
            out.push_str(&format!("  line {}: -|{e}|\n", i + 1));
            out.push_str(&format!("  line {}: +|{a}|\n", i + 1));
        }
    }
    out
}
