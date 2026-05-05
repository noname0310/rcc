//! Coverage gate wrapper for `cargo llvm-cov`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const WORKSPACE_LINE_THRESHOLD: f64 = 80.0;
const IGNORE_FILENAME_REGEX: &str =
    "(third_party|fuzz/corpus|fuzz/artifacts|target|crates/.*/tests/snapshots)";

const CRATE_THRESHOLDS: &[(&str, f64, &str)] = &[
    ("rcc_ast", 5.0, "visitor traversal is mostly exercised indirectly later"),
    ("rcc_cfg", 80.0, "core CFG builder/lower/verifier is release-critical"),
    ("rcc_cfg_transform", 0.0, "placeholder pass trait, no real transform code yet"),
    ("rcc_codegen_llvm", 80.0, "backend layout and IR tests cover the stable surface"),
    ("rcc_conformance", 45.0, "adapters include subprocess paths exercised by CI jobs"),
    ("rcc_data_structures", 70.0, "small helpers with macro-generated index types"),
    ("rcc_driver", 60.0, "integration-heavy driver paths are partly platform-gated"),
    ("rcc_errors", 85.0, "diagnostic builder/emitter policy is release-critical"),
    ("rcc_hir", 75.0, "layout service covered; pretty/debug helpers are lighter"),
    ("rcc_hir_lower", 80.0, "source lowering is release-critical"),
    ("rcc_lexer", 90.0, "lexer has dense table/corpus/fuzz coverage"),
    ("rcc_parse", 80.0, "grammar coverage is release-critical"),
    ("rcc_preprocess", 90.0, "preprocessor has dense unit and corpus coverage"),
    ("rcc_session", 90.0, "small option/session surface"),
    ("rcc_span", 85.0, "source map and symbol APIs are small but critical"),
    ("rcc_typeck", 80.0, "semantic checks are release-critical"),
    ("xtask", 60.0, "automation includes subprocess paths not unit-tested"),
];

/// Run coverage collection and enforce thresholds.
pub fn run(root: &Path, lcov: &Path, json: &Path, check_only: bool) -> Result<()> {
    let lcov = root.join(lcov);
    let json = root.join(json);
    let report = root.join("target/coverage/coverage-report.txt");

    ensure_parent(&lcov)?;
    ensure_parent(&json)?;
    ensure_parent(&report)?;

    if !check_only {
        run_cargo_llvm_cov_json(root, &json)?;
        run_cargo_llvm_cov_lcov(root, &lcov)?;
    }

    check_artifacts(&[&json, &lcov])?;
    let summary = CoverageSummary::from_json_file(&json, root)?;
    let report_text = summary.render_report();
    fs::write(&report, &report_text).with_context(|| format!("writing {}", report.display()))?;
    print!("{report_text}");

    summary.enforce_thresholds()
}

fn run_cargo_llvm_cov_json(root: &Path, json: &Path) -> Result<()> {
    run_command(
        root,
        Command::new("cargo")
            .arg("llvm-cov")
            .arg("--workspace")
            .arg("--json")
            .arg("--summary-only")
            .arg("--output-path")
            .arg(json)
            .arg("--ignore-filename-regex")
            .arg(IGNORE_FILENAME_REGEX),
        "cargo llvm-cov JSON summary",
    )
}

fn run_cargo_llvm_cov_lcov(root: &Path, lcov: &Path) -> Result<()> {
    run_command(
        root,
        Command::new("cargo")
            .arg("llvm-cov")
            .arg("report")
            .arg("--lcov")
            .arg("--output-path")
            .arg(lcov)
            .arg("--ignore-filename-regex")
            .arg(IGNORE_FILENAME_REGEX),
        "cargo llvm-cov LCOV report",
    )
}

fn run_command(root: &Path, command: &mut Command, label: &str) -> Result<()> {
    let status = command.current_dir(root).status().with_context(|| format!("running {label}"))?;
    if !status.success() {
        bail!("{label} failed with status {status}");
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("creating coverage artifact directory {}", parent.display())
        })?;
    }
    Ok(())
}

fn check_artifacts(paths: &[&Path]) -> Result<()> {
    for path in paths {
        let meta = fs::metadata(path)
            .with_context(|| format!("coverage artifact missing: {}", path.display()))?;
        if meta.len() == 0 {
            bail!("coverage artifact is empty: {}", path.display());
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CoverageSummary {
    workspace_lines: Metric,
    crates: BTreeMap<String, Metric>,
}

impl CoverageSummary {
    fn from_json_file(path: &Path, root: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let export: LlvmCovExport =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let data =
            export.data.first().ok_or_else(|| anyhow!("coverage export has no data entries"))?;
        let workspace_lines = Metric::from_json(&data.totals.lines);
        let mut crates: BTreeMap<String, Metric> = BTreeMap::new();
        for file in &data.files {
            let Some(crate_name) = crate_name_for_file(root, &file.filename) else {
                continue;
            };
            crates.entry(crate_name).or_default().add(&Metric::from_json(&file.summary.lines));
        }
        Ok(Self { workspace_lines, crates })
    }

    fn render_report(&self) -> String {
        let mut out = String::new();
        out.push_str("Coverage gate report\n");
        out.push_str("====================\n\n");
        out.push_str(&format!(
            "workspace lines: {:.2}% ({}/{}) threshold {:.2}%\n\n",
            self.workspace_lines.percent(),
            self.workspace_lines.covered,
            self.workspace_lines.count,
            WORKSPACE_LINE_THRESHOLD
        ));
        out.push_str("| crate | line coverage | threshold | status | note |\n");
        out.push_str("| ----- | ------------- | --------- | ------ | ---- |\n");
        for (name, threshold, note) in CRATE_THRESHOLDS {
            let metric = self.crates.get(*name).copied().unwrap_or_default();
            let status = if metric.percent() + f64::EPSILON >= *threshold { "ok" } else { "below" };
            out.push_str(&format!(
                "| `{name}` | {:.2}% ({}/{}) | {:.2}% | {status} | {note} |\n",
                metric.percent(),
                metric.covered,
                metric.count,
                threshold
            ));
        }
        out
    }

    fn enforce_thresholds(&self) -> Result<()> {
        let mut failures = Vec::new();
        if self.workspace_lines.percent() + f64::EPSILON < WORKSPACE_LINE_THRESHOLD {
            failures.push(format!(
                "workspace line coverage {:.2}% < {:.2}%",
                self.workspace_lines.percent(),
                WORKSPACE_LINE_THRESHOLD
            ));
        }
        for (name, threshold, _) in CRATE_THRESHOLDS {
            let metric = self.crates.get(*name).copied().unwrap_or_default();
            if metric.percent() + f64::EPSILON < *threshold {
                failures.push(format!(
                    "{name} line coverage {:.2}% < {:.2}%",
                    metric.percent(),
                    threshold
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            bail!("coverage threshold failure:\n{}", failures.join("\n"));
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Metric {
    count: u64,
    covered: u64,
}

impl Metric {
    fn from_json(json: &JsonMetric) -> Self {
        Self { count: json.count, covered: json.covered }
    }

    fn add(&mut self, other: &Metric) {
        self.count += other.count;
        self.covered += other.covered;
    }

    fn percent(self) -> f64 {
        if self.count == 0 {
            100.0
        } else {
            self.covered as f64 * 100.0 / self.count as f64
        }
    }
}

fn crate_name_for_file(root: &Path, filename: &str) -> Option<String> {
    let normalized = filename.replace('\\', "/");
    let root = root.to_string_lossy().replace('\\', "/");
    let rel = normalized.strip_prefix(&root).unwrap_or(&normalized).trim_start_matches('/');
    if let Some(rest) = rel.strip_prefix("crates/") {
        return rest.split('/').next().map(str::to_owned);
    }
    if rel.starts_with("xtask/src/") {
        return Some("xtask".to_owned());
    }
    None
}

#[derive(Deserialize)]
struct LlvmCovExport {
    data: Vec<LlvmCovData>,
}

#[derive(Deserialize)]
struct LlvmCovData {
    files: Vec<LlvmCovFile>,
    totals: LlvmCovTotals,
}

#[derive(Deserialize)]
struct LlvmCovFile {
    filename: String,
    summary: LlvmCovSummary,
}

#[derive(Deserialize)]
struct LlvmCovTotals {
    lines: JsonMetric,
}

#[derive(Deserialize)]
struct LlvmCovSummary {
    lines: JsonMetric,
}

#[derive(Deserialize)]
struct JsonMetric {
    count: u64,
    covered: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_handles_windows_and_unix_paths() {
        let root = Path::new("D:/work/rcc");
        assert_eq!(
            crate_name_for_file(root, "D:\\work\\rcc\\crates\\rcc_parse\\src\\lib.rs"),
            Some("rcc_parse".to_owned())
        );
        assert_eq!(
            crate_name_for_file(root, "D:/work/rcc/xtask/src/main.rs"),
            Some("xtask".to_owned())
        );
        assert_eq!(crate_name_for_file(root, "D:/work/rcc/docs/testing.md"), None);
    }

    #[test]
    fn metrics_accumulate_and_percentage_is_stable() {
        let mut metric = Metric { count: 10, covered: 7 };
        metric.add(&Metric { count: 30, covered: 27 });
        assert_eq!(metric.count, 40);
        assert_eq!(metric.covered, 34);
        assert!((metric.percent() - 85.0).abs() < f64::EPSILON);
        assert_eq!(Metric::default().percent(), 100.0);
    }

    #[test]
    fn missing_coverage_artifact_is_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "rcc-coverage-test-{}-{}",
            std::process::id(),
            "missing"
        ));
        let missing = dir.join("missing.json");
        let err = check_artifacts(&[&missing]).unwrap_err().to_string();
        assert!(err.contains("coverage artifact missing"));
    }
}
