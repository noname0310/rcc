//! chibicc preprocessor-suite integration (task 04-18, milestone M5).
//!
//! The chibicc upstream suite ships three preprocessor-heavy fixtures
//! under `third_party/testsuites/chibicc/test/`:
//!
//! | fixture      | what it stresses                                  |
//! |--------------|---------------------------------------------------|
//! | `typedef.c`  | pure C99 typedef forms, minimal preprocessor use  |
//! | `macro.c`    | the full chibicc macro corpus — includes GNU      |
//! |              | extensions (`args...` named variadics,            |
//! |              | `, ## __VA_ARGS__` comma elision, `__VA_OPT__`,   |
//! |              | `__COUNTER__`, `__BASE_FILE__`, `__TIMESTAMP__`,  |
//! |              | computed `#include`) alongside C99 features.      |
//! | `include.c`  | *not yet vendored* — reserved for when chibicc's  |
//! |              | separate include-only fixture lands upstream.     |
//!
//! This file runs each present fixture through [`Preprocessor::run`]
//! and records an error baseline against the fixture's current
//! behaviour. Assertions are split:
//!
//! * **`typedef.c`** — must preprocess without a single diagnostic.
//!   It's simple pure-C99 input and any new error here is a
//!   regression.
//! * **`macro.c`** — a C99 preprocessor cannot clear the whole file
//!   because chibicc relies on several GNU-only extensions that are
//!   intentionally deferred. The test instead checks:
//!     - the preprocessor finishes without panicking;
//!     - a non-empty pp-token stream is produced;
//!     - every emitted error falls into a pre-approved bucket
//!       (`E0013`, `E0014`, `E0022`, `E0025`);
//!     - the total error count does not exceed a baseline ceiling,
//!       so feature work that *fixes* one of these buckets surfaces
//!       as a visible reduction (and if we ever regress by adding a
//!       *new* error kind, the bucket check fails loudly).
//!
//! When C99 support grows to cover the gap (e.g. macro-expanded
//! `#include`), the corresponding bucket / ceiling in this file
//! should shrink accordingly.

use std::path::{Path, PathBuf};

use rcc_errors::{CaptureEmitter, Diagnostic, Handler, Level};
use rcc_preprocess::Preprocessor;
use rcc_session::{Options, Session};

/// Absolute path to the vendored chibicc `test/` directory.
fn chibicc_test_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("third_party")
        .join("testsuites")
        .join("chibicc")
        .join("test")
}

/// Load `path` into a fresh session (with a capture emitter + an `-I`
/// pointing at `path`'s directory so chibicc's `#include "test.h"`
/// resolves) and preprocess it end-to-end. Returns the number of
/// emitted pp-tokens plus the diagnostics the run produced.
fn preprocess_fixture(path: &Path) -> (usize, Vec<Diagnostic>) {
    preprocess_fixture_with_opts(path, Options::default())
}

fn preprocess_fixture_gnu(path: &Path) -> (usize, Vec<Diagnostic>) {
    let opts = Options {
        gnu_permissive_redefinition: true,
        gnu_named_variadic: true,
        gnu_permissive_paste: true,
        gnu_va_args_elision: true,
        ..Options::default()
    };
    preprocess_fixture_with_opts(path, opts)
}

fn preprocess_fixture_with_opts(path: &Path, mut opts: Options) -> (usize, Vec<Diagnostic>) {
    let cap = CaptureEmitter::new();
    if let Some(parent) = path.parent() {
        opts.include_paths.push(parent.to_path_buf());
    }
    let mut sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
    let id = sess.source_map.write().unwrap().load_file(path).expect("vendored fixture present");
    let mut pp = Preprocessor::new(&mut sess);
    let tokens = pp.run(id);
    (tokens.len(), cap.diagnostics())
}

#[test]
fn chibicc_typedef_c_preprocesses_cleanly() {
    let path = chibicc_test_dir().join("typedef.c");
    assert!(path.is_file(), "vendor fetch missing: {}", path.display());
    let (count, diags) = preprocess_fixture(&path);
    let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(count > 0, "typedef.c must produce a non-empty pp-token stream");
    assert!(
        errors.is_empty(),
        "typedef.c is pure C99 and must preprocess with zero errors, got {:?}",
        errors.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>(),
    );
}

/// Error codes the chibicc `macro.c` baseline is allowed to surface
/// in strict C99 mode (GNU extensions off).
///
/// These are the strictly-conforming C99 responses to GCC extensions
/// chibicc happens to use. With GNU extension flags enabled, the
/// `chibicc_macro_c_gnu_mode_zero_errors` test asserts 0 errors.
///
/// | code  | shape                                              |
/// |-------|----------------------------------------------------|
/// | E0022 | "redefined with a different body" — chibicc       |
/// |       | deliberately re-`#define`s across the file.        |
/// | E0014 | "invalid #define" — the GNU `args...`              |
/// |       | named-variadic form.                               |
/// | E0025 | "pasting forms an invalid token" — triggered by    |
/// |       | `CONCAT(4,.57)` and similar pp-number fragment     |
/// |       | pastes that our paste validator rejects.           |
const MACRO_C_ALLOWED_ERRORS: &[&str] = &["E0014", "E0022", "E0025"];

/// Upper bound on the total number of error diagnostics `macro.c` is
/// allowed to produce in strict C99 mode. E0013 was resolved by
/// the computed `#include` support (task 04-20b); current baseline
/// is 32 (22× E0022, 6× E0014, 2× E0025, 2× secondary E0022 from
/// named-variadic redefs).
const MACRO_C_ERROR_CEILING: usize = 32;

#[test]
fn chibicc_macro_c_runs_to_completion_with_bounded_gaps() {
    let path = chibicc_test_dir().join("macro.c");
    assert!(path.is_file(), "vendor fetch missing: {}", path.display());
    let (count, diags) = preprocess_fixture(&path);

    assert!(count > 0, "macro.c must produce a non-empty pp-token stream");

    let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.level == Level::Error).collect();

    // Every surfaced error must be in the pre-approved bucket list —
    // a fresh code here means we have either regressed on an input
    // that used to work or introduced a net-new failure mode that
    // deserves a dedicated task before this baseline is adjusted.
    for d in &errors {
        let code = d.code.unwrap_or("<no-code>");
        assert!(
            MACRO_C_ALLOWED_ERRORS.contains(&code),
            "unexpected error code `{code}` in macro.c baseline: {:?}; \
             update MACRO_C_ALLOWED_ERRORS if this is a new known gap, \
             otherwise this is a regression",
            d.message,
        );
    }

    assert!(
        errors.len() <= MACRO_C_ERROR_CEILING,
        "macro.c error count climbed from baseline {MACRO_C_ERROR_CEILING} to {}; \
         investigate the new diagnostics: {:?}",
        errors.len(),
        errors.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>(),
    );
}

#[test]
fn chibicc_include_fixtures_resolve_end_to_end() {
    // Sanity: include1.h chains to include2.h, both relative to the
    // fixture directory. A direct preprocessor run over include1.h
    // must not fail header resolution.
    let path = chibicc_test_dir().join("include1.h");
    assert!(path.is_file(), "vendor fetch missing: {}", path.display());
    let (count, diags) = preprocess_fixture(&path);
    let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(count > 0, "include1.h must produce output");
    assert!(
        errors.is_empty(),
        "include1.h / include2.h chain must resolve cleanly, got {:?}",
        errors.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>(),
    );
}

#[test]
fn chibicc_macro_c_gnu_mode_zero_errors() {
    let path = chibicc_test_dir().join("macro.c");
    assert!(path.is_file(), "vendor fetch missing: {}", path.display());
    let (count, diags) = preprocess_fixture_gnu(&path);

    assert!(count > 0, "macro.c must produce a non-empty pp-token stream");

    let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        errors.is_empty(),
        "macro.c with GNU extensions enabled must produce zero errors, got {} errors: {:?}",
        errors.len(),
        errors.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>(),
    );
}
