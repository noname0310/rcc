//! `check-error-codes`: verify every `E\d{4}` / `W\d{4}` code used in source
//! has a matching docs entry in `docs/error-codes.md`, and vice-versa.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Run the check, returning `Ok(())` when everything is consistent.
pub fn run(root: &Path) -> Result<()> {
    let docs_path = root.join("docs/error-codes.md");
    let docs_codes = collect_doc_codes(&docs_path)
        .with_context(|| format!("reading {}", docs_path.display()))?;

    let codes_rs = root.join("crates/rcc_errors/src/codes.rs");
    let registry_codes = collect_registry_codes(&codes_rs)
        .with_context(|| format!("reading {}", codes_rs.display()))?;

    let source_codes = collect_source_codes(root)?;

    let mut ok = true;

    // Every code in the registry must appear in docs.
    for code in &registry_codes {
        if !docs_codes.contains(code) {
            eprintln!("error: {code} is in codes.rs but missing from docs/error-codes.md");
            ok = false;
        }
    }

    // Every code in docs must appear in the registry.
    for code in &docs_codes {
        if !registry_codes.contains(code) {
            eprintln!("error: {code} is in docs/error-codes.md but missing from codes.rs");
            ok = false;
        }
    }

    // Every diagnostic code used in workspace source must be registered.
    for code in &source_codes {
        if !registry_codes.contains(code) {
            eprintln!("error: {code} is used in source but not in the registry (codes.rs)");
            ok = false;
        }
    }

    if ok {
        let n = registry_codes.len();
        println!("check-error-codes: {n} codes, all consistent.");
        Ok(())
    } else {
        bail!("error-code consistency check failed");
    }
}

/// Collect `## EXXXX` / `## WXXXX` headings from the docs file.
fn collect_doc_codes(path: &Path) -> Result<BTreeSet<String>> {
    let content = std::fs::read_to_string(path)?;
    let mut codes = BTreeSet::new();
    for line in content.lines() {
        // Match lines like "## E0001 — unexpected character".
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            if let Some(code) = rest.split(|c: char| !c.is_ascii_alphanumeric()).next() {
                if is_diagnostic_code(code) {
                    codes.insert(code.to_string());
                }
            }
        }
    }
    Ok(codes)
}

/// Collect codes declared in `codes.rs` via `pub const EXXXX: &str = "EXXXX";`.
fn collect_registry_codes(path: &Path) -> Result<BTreeSet<String>> {
    let content = std::fs::read_to_string(path)?;
    let mut codes = BTreeSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub const ") {
            if let Some(name) = rest.split(':').next() {
                let name = name.trim();
                if is_diagnostic_code(name) {
                    codes.insert(name.to_string());
                }
            }
        }
    }
    Ok(codes)
}

/// Scan `.rs` files under `crates/` (excluding codes.rs itself) for
/// string literals that look like error codes.
fn collect_source_codes(root: &Path) -> Result<BTreeSet<String>> {
    let mut codes = BTreeSet::new();
    let crates_dir = root.join("crates");
    if !crates_dir.is_dir() {
        return Ok(codes);
    }
    walk_rs_files(&crates_dir, &mut |path| {
        // Skip the registry itself.
        if path.ends_with("rcc_errors/src/codes.rs") {
            return Ok(());
        }
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        extract_error_codes_from_text(&content, &mut codes);
        Ok(())
    })?;
    Ok(codes)
}

fn walk_rs_files(dir: &Path, cb: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, cb)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            cb(&path)?;
        }
    }
    Ok(())
}

/// Find substrings matching `"E\d{4}"` or `"W\d{4}"` (quoted diagnostic codes in source).
fn extract_error_codes_from_text(text: &str, out: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 6 < bytes.len() {
        if bytes[i] == b'"' && matches!(bytes[i + 1], b'E' | b'W') && bytes[i + 6] == b'"' {
            let candidate = &text[i + 1..i + 6];
            if is_diagnostic_code(candidate) {
                out.insert(candidate.to_string());
            }
        }
        i += 1;
    }
}

fn is_diagnostic_code(s: &str) -> bool {
    s.len() == 5
        && matches!(s.as_bytes().first(), Some(b'E' | b'W'))
        && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Programmatic helper: returns the list of registered codes from disk.
/// Used by the integration test.
pub fn registered_codes(root: &Path) -> Result<Vec<String>> {
    let codes_rs = root.join("crates/rcc_errors/src/codes.rs");
    let set = collect_registry_codes(&codes_rs)?;
    Ok(set.into_iter().collect())
}

/// Programmatic helper: returns the list of documented codes from disk.
pub fn documented_codes(root: &Path) -> Result<Vec<String>> {
    let docs_path = root.join("docs/error-codes.md");
    let set = collect_doc_codes(&docs_path)?;
    Ok(set.into_iter().collect())
}

/// Full list of validation helpers used by the workspace test.
pub fn find_dirs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must have a parent directory")
        .to_path_buf()
}
