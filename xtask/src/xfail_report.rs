//! `xfail-report`: compare expected-failure entries across two git revisions.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// Run the command and print a human-readable delta report.
pub fn run(root: &Path, range: &str) -> Result<()> {
    let report = build_report(root, range)?;
    print!("{}", render_report(&report));
    Ok(())
}

/// Build an xfail delta report for `OLD..NEW`.
pub fn build_report(root: &Path, range: &str) -> Result<XFailDelta> {
    let (old_rev, new_rev) = parse_range(range)?;
    let old_entries = read_revision(root, old_rev)?;
    let new_entries = read_revision(root, new_rev)?;
    Ok(diff_entries(range, old_entries, new_entries))
}

/// Render a delta report suitable for commit-message footers.
pub fn render_report(report: &XFailDelta) -> String {
    let mut out = String::new();
    let delta = report.new_count as isize - report.old_count as isize;
    let trend = match delta.cmp(&0) {
        std::cmp::Ordering::Less => "shrink",
        std::cmp::Ordering::Equal => "unchanged",
        std::cmp::Ordering::Greater => "growth",
    };

    out.push_str(&format!("xfail-report: {}\n", report.range));
    out.push_str(&format!(
        "old: {} entries\nnew: {} entries\ndelta: {delta:+} ({trend}; removed {}, added {}, changed {})\n",
        report.old_count,
        report.new_count,
        report.removed.len(),
        report.added.len(),
        report.changed.len()
    ));

    if !report.removed.is_empty() {
        out.push_str("\nremoved:\n");
        for entry in &report.removed {
            out.push_str(&format!("  - {} [{}]\n", entry.id, entry.path));
            out.push_str(&format!("    reason: {}\n", entry.reason));
        }
    }

    if !report.added.is_empty() {
        out.push_str("\nadded:\n");
        for entry in &report.added {
            out.push_str(&format!("  + {} [{}]\n", entry.id, entry.path));
            out.push_str(&format!("    reason: {}\n", entry.reason));
        }
    }

    if !report.changed.is_empty() {
        out.push_str("\nchanged:\n");
        for change in &report.changed {
            out.push_str(&format!("  * {} [{}]\n", change.id, change.path));
            out.push_str(&format!("    old: {}\n", change.old_reason));
            out.push_str(&format!("    new: {}\n", change.new_reason));
        }
    }

    out
}

/// Full report data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XFailDelta {
    /// User-provided git range.
    pub range: String,
    /// Entry count at the old revision.
    pub old_count: usize,
    /// Entry count at the new revision.
    pub new_count: usize,
    /// Entries present in OLD but not NEW.
    pub removed: Vec<ReportEntry>,
    /// Entries present in NEW but not OLD.
    pub added: Vec<ReportEntry>,
    /// Entries whose reason changed without changing identity.
    pub changed: Vec<ReasonChange>,
}

/// One reportable xfail entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportEntry {
    /// `xfail.toml` repository path.
    pub path: String,
    /// Conformance test id.
    pub id: String,
    /// Human-readable xfail reason.
    pub reason: String,
}

/// A reason-only xfail change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasonChange {
    /// `xfail.toml` repository path.
    pub path: String,
    /// Conformance test id.
    pub id: String,
    /// Previous reason.
    pub old_reason: String,
    /// New reason.
    pub new_reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct EntryKey {
    path: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct XFailFile {
    #[serde(default)]
    xfail: Vec<XFailEntry>,
}

#[derive(Debug, Deserialize)]
struct XFailEntry {
    id: String,
    reason: String,
}

fn parse_range(range: &str) -> Result<(&str, &str)> {
    let Some((old_rev, new_rev)) = range.split_once("..") else {
        bail!("xfail-report expects a git range in OLD..NEW form");
    };
    if old_rev.is_empty() || new_rev.is_empty() || new_rev.contains("..") {
        bail!("xfail-report expects a git range in OLD..NEW form");
    }
    Ok((old_rev, new_rev))
}

fn read_revision(root: &Path, rev: &str) -> Result<BTreeMap<EntryKey, String>> {
    let paths =
        git_stdout(root, ["ls-tree", "-r", "--name-only", rev, "--", "third_party/testsuites"])
            .with_context(|| format!("listing xfail files at {rev}"))?;

    let mut entries = BTreeMap::new();
    for path in paths.lines().filter(|p| p.ends_with("/xfail.toml")) {
        let spec = format!("{rev}:{path}");
        let content = git_stdout(root, ["show", spec.as_str()])
            .with_context(|| format!("reading {path} at {rev}"))?;
        for entry in parse_entries(path, &content)? {
            let key = EntryKey { path: entry.path, id: entry.id };
            entries.insert(key, entry.reason);
        }
    }
    Ok(entries)
}

fn parse_entries(path: &str, content: &str) -> Result<Vec<ReportEntry>> {
    let parsed: XFailFile = toml::from_str(content).with_context(|| format!("parsing {path}"))?;
    Ok(parsed
        .xfail
        .into_iter()
        .map(|entry| ReportEntry { path: path.to_owned(), id: entry.id, reason: entry.reason })
        .collect())
}

fn diff_entries(
    range: &str,
    old_entries: BTreeMap<EntryKey, String>,
    new_entries: BTreeMap<EntryKey, String>,
) -> XFailDelta {
    let old_count = old_entries.len();
    let new_count = new_entries.len();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut changed = Vec::new();

    for (key, old_reason) in &old_entries {
        match new_entries.get(key) {
            None => removed.push(ReportEntry {
                path: key.path.clone(),
                id: key.id.clone(),
                reason: old_reason.clone(),
            }),
            Some(new_reason) if new_reason != old_reason => changed.push(ReasonChange {
                path: key.path.clone(),
                id: key.id.clone(),
                old_reason: old_reason.clone(),
                new_reason: new_reason.clone(),
            }),
            Some(_) => {}
        }
    }

    for (key, new_reason) in &new_entries {
        if !old_entries.contains_key(key) {
            added.push(ReportEntry {
                path: key.path.clone(),
                id: key.id.clone(),
                reason: new_reason.clone(),
            });
        }
    }

    XFailDelta { range: range.to_owned(), old_count, new_count, removed, added, changed }
}

fn git_stdout<const N: usize>(root: &Path, args: [&str; N]) -> Result<String> {
    let output =
        Command::new("git").args(args).current_dir(root).output().context("running git")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git failed: {stderr}");
    }
    String::from_utf8(output.stdout).context("git output was not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xfail_entries_with_paths() {
        let entries = parse_entries(
            "third_party/testsuites/demo/xfail.toml",
            r#"
[[xfail]]
id = "demo::case"
reason = "known gap"
"#,
        )
        .unwrap();

        assert_eq!(
            entries,
            vec![ReportEntry {
                path: "third_party/testsuites/demo/xfail.toml".into(),
                id: "demo::case".into(),
                reason: "known gap".into(),
            }]
        );
    }

    #[test]
    fn diffs_added_removed_and_changed_entries() {
        let old = BTreeMap::from([
            (
                EntryKey { path: "a/xfail.toml".into(), id: "suite::removed".into() },
                "old gap".into(),
            ),
            (
                EntryKey { path: "a/xfail.toml".into(), id: "suite::changed".into() },
                "old reason".into(),
            ),
        ]);
        let new = BTreeMap::from([
            (
                EntryKey { path: "a/xfail.toml".into(), id: "suite::changed".into() },
                "new reason".into(),
            ),
            (EntryKey { path: "b/xfail.toml".into(), id: "suite::added".into() }, "new gap".into()),
        ]);

        let delta = diff_entries("old..new", old, new);

        assert_eq!(delta.old_count, 2);
        assert_eq!(delta.new_count, 2);
        assert_eq!(delta.removed.len(), 1);
        assert_eq!(delta.added.len(), 1);
        assert_eq!(delta.changed.len(), 1);
        assert!(render_report(&delta).contains("changed 1"));
    }

    #[test]
    fn rejects_non_range_input() {
        let err = parse_range("HEAD").unwrap_err();
        assert!(err.to_string().contains("OLD..NEW"));
    }
}
