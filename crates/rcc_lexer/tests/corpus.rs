//! Corpus-wide sanity check against the c-testsuite vendored under
//! `third_party/testsuites/c-testsuite/tests/single-exec/` (task 03-lex/11).
//!
//! Runs the lexer over **every** `.c` source file in the suite and
//! enforces three invariants:
//!
//! 1. **No panics** — the tokenizer must terminate cleanly on every file.
//!    Per-file panics are caught (so one bad file does not obscure the
//!    rest) and reported together at the end.
//! 2. **Span partition** — with `preserve_whitespace(true)`, every byte
//!    of the input must be covered by exactly one emitted token's span.
//!    The only permitted gap is the *line-splice exception* (task
//!    03-lex/02): trailing `\\\n` / `\\\r\n` pairs that the line-splice
//!    cursor elides before the next tokenisation attempt are allowed
//!    between tokens or at end-of-file.
//! 3. **Unknown allow-list** — `PpTokenKind::Unknown` tokens may only
//!    cover the three bytes explicitly permitted by C99 §6.4.3 outside
//!    of identifiers: `$`, `@`, and `` ` ``.
//!
//! ## Locating the suite
//!
//! Resolution order:
//!
//! 1. `RCC_CTESTSUITE_SINGLE_EXEC_DIR` env var (used by CI / out-of-tree
//!    builds that keep the suite in a side location).
//! 2. In-tree vendored path
//!    `../../third_party/testsuites/c-testsuite/tests/single-exec`
//!    relative to this crate's `CARGO_MANIFEST_DIR`.
//!
//! If neither exists (i.e. `cargo xtask fetch-testsuites --only
//! c-testsuite` has not been run), the test emits a skip message on
//! stderr and passes — local development without internet stays green.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use rcc_lexer::{PpToken, PpTokenKind, Tokenizer};
use rcc_span::FileId;

/// Override hook for the suite's `single-exec` directory.
const ENV_OVERRIDE: &str = "RCC_CTESTSUITE_SINGLE_EXEC_DIR";

/// Minimum corpus size expected once the suite is vendored.
/// c-testsuite ships with ~220 `.c` files; any substantial shrinkage
/// probably means a fetch failure we want surfaced loudly.
const MIN_FILES: usize = 200;

/// Resolve the `single-exec` directory, preferring the env-var override
/// over the in-tree vendored path.
fn suite_dir() -> PathBuf {
    if let Ok(p) = std::env::var(ENV_OVERRIDE) {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/rcc_lexer parent (crates)")
        .parent()
        .expect("crates parent (workspace root)")
        .join("third_party")
        .join("testsuites")
        .join("c-testsuite")
        .join("tests")
        .join("single-exec")
}

/// Allow-list per task 03-lex/11: only `$`, `@`, `` ` `` may surface as
/// `Unknown` outside of an identifier-UCN context. All three are listed
/// as implementation-defined permitted source characters in C99 §6.4.3.
fn is_allowed_unknown_slice(slice: &str) -> bool {
    matches!(slice, "$" | "@" | "`")
}

/// Recognise a run of backslash-newline pairs (LF or CRLF form). The
/// line-splice cursor silently elides these during translation-phase-2
/// processing (C99 §5.1.1.2) without shifting the underlying physical
/// offset, so trailing / between-token splices are the one permitted
/// form of "uncovered" bytes in an otherwise partitioning stream.
fn is_splice_run(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            return false;
        }
        if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 2;
        } else if i + 2 < bytes.len() && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
            i += 3;
        } else {
            return false;
        }
    }
    !bytes.is_empty()
}

/// Render a short printable form of a byte gap for failure messages.
fn debug_gap(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => format!("{s:?}"),
        Err(_) => format!("{bytes:?}"),
    }
}

/// Try to extract a stringly panic payload for reporting.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Collect `.c` files from `dir` in a deterministic (sorted) order.
fn collect_c_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()))
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("c"))
        .collect();
    out.sort();
    out
}

/// Walk `tokens` and check the span-partition invariant against `src`.
/// Returns a list of per-file failure strings (empty on success).
fn check_partition(path: &Path, src: &str, tokens: &[PpToken]) -> Vec<String> {
    let mut failures = Vec::new();
    let src_bytes = src.as_bytes();
    let mut expected_lo: u32 = 0;

    for t in tokens {
        if t.span.lo.0 < expected_lo {
            failures.push(format!(
                "{}: token span {}..{} ({:?}) overlaps previous token ending at {}",
                path.display(),
                t.span.lo.0,
                t.span.hi.0,
                t.kind,
                expected_lo,
            ));
            return failures;
        }
        if t.span.lo.0 > expected_lo {
            let lo = expected_lo as usize;
            let hi = t.span.lo.0 as usize;
            let gap = &src_bytes[lo..hi];
            if !is_splice_run(gap) {
                failures.push(format!(
                    "{}: gap bytes [{lo}..{hi}] = {} before token {:?} at [{}..{}] are not a splice run; next token slice = {:?}",
                    path.display(),
                    debug_gap(gap),
                    t.kind,
                    t.span.lo.0,
                    t.span.hi.0,
                    &src[t.span.lo.0 as usize..t.span.hi.0 as usize],
                ));
                return failures;
            }
        }
        if (t.span.hi.0 as usize) > src.len() {
            failures.push(format!(
                "{}: token span {}..{} ({:?}) exceeds source length {}",
                path.display(),
                t.span.lo.0,
                t.span.hi.0,
                t.kind,
                src.len(),
            ));
            return failures;
        }
        expected_lo = t.span.hi.0;
    }

    if (expected_lo as usize) < src.len() {
        let tail = &src_bytes[expected_lo as usize..];
        if !is_splice_run(tail) {
            failures.push(format!(
                "{}: trailing bytes [{}..{}] = {} uncovered and not a splice run",
                path.display(),
                expected_lo,
                src.len(),
                debug_gap(tail),
            ));
        }
    }

    failures
}

/// Walk `tokens` and check the Unknown-allow-list invariant against `src`.
fn check_unknown_allowlist(path: &Path, src: &str, tokens: &[PpToken]) -> Vec<String> {
    let mut failures = Vec::new();
    for t in tokens {
        if !matches!(t.kind, PpTokenKind::Unknown) {
            continue;
        }
        let slice = &src[t.span.lo.0 as usize..t.span.hi.0 as usize];
        if !is_allowed_unknown_slice(slice) {
            failures.push(format!(
                "{}: unexpected Unknown token at [{}..{}] = {:?} (kind {:?})",
                path.display(),
                t.span.lo.0,
                t.span.hi.0,
                slice,
                t.kind,
            ));
        }
    }
    failures
}

#[test]
fn corpus_lex_single_exec_files() {
    let dir = suite_dir();
    if !dir.is_dir() {
        eprintln!(
            "skipping corpus_lex_single_exec_files: c-testsuite not vendored at {}\n\
             run `cargo xtask fetch-testsuites --only c-testsuite` or set {} to enable",
            dir.display(),
            ENV_OVERRIDE,
        );
        return;
    }

    let files = collect_c_files(&dir);
    assert!(
        files.len() >= MIN_FILES,
        "expected at least {} .c files under {}, found {} — is the suite fully fetched?",
        MIN_FILES,
        dir.display(),
        files.len(),
    );

    let mut all_failures: Vec<String> = Vec::new();
    let mut lexed = 0usize;
    let mut skipped_non_utf8 = 0usize;

    for path in &files {
        let raw = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                all_failures.push(format!("{}: read error: {e}", path.display()));
                continue;
            }
        };

        // The lexer operates on `&str`. c-testsuite is ASCII in practice;
        // a non-UTF-8 file is out of scope for this invariant check.
        let src = match std::str::from_utf8(&raw) {
            Ok(s) => s,
            Err(_) => {
                skipped_non_utf8 += 1;
                continue;
            }
        };

        // ── Check 1: No panics ────────────────────────────────────────
        let collect_result = panic::catch_unwind(AssertUnwindSafe(|| {
            Tokenizer::new(FileId(0), src).preserve_whitespace(true).collect::<Vec<PpToken>>()
        }));
        let tokens = match collect_result {
            Ok(v) => v,
            Err(payload) => {
                all_failures.push(format!(
                    "{}: panic during lex: {}",
                    path.display(),
                    panic_message(payload)
                ));
                continue;
            }
        };

        // ── Checks 2 and 3 ────────────────────────────────────────────
        all_failures.extend(check_partition(path, src, &tokens));
        all_failures.extend(check_unknown_allowlist(path, src, &tokens));

        lexed += 1;
    }

    if !all_failures.is_empty() {
        let shown: Vec<String> = all_failures.iter().take(20).cloned().collect();
        let more = all_failures.len().saturating_sub(shown.len());
        let mut msg = format!(
            "corpus lex failed: {} failure(s) across {} files (non-utf8 skipped: {})\n  - {}",
            all_failures.len(),
            lexed,
            skipped_non_utf8,
            shown.join("\n  - "),
        );
        if more > 0 {
            msg.push_str(&format!("\n  - ... and {more} more"));
        }
        panic!("{msg}");
    }

    // Acceptance: green on >= 200 files.
    assert!(
        lexed >= MIN_FILES,
        "only {} files were lexed successfully (min {}); non-utf8 skipped: {}",
        lexed,
        MIN_FILES,
        skipped_non_utf8,
    );
}
