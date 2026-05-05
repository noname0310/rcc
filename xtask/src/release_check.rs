//! Local release-candidate gate runner.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use toml::Value;

/// Options for `cargo xtask release-check`.
#[derive(Debug, Clone)]
pub struct ReleaseCheckOptions {
    /// Directory where command logs and the summary are written.
    pub report_dir: PathBuf,
    /// Skip the LLVM-enabled workspace test even when an LLVM prefix is present.
    pub skip_llvm: bool,
    /// Skip `cargo xtask coverage` even when `cargo-llvm-cov` is installed.
    pub skip_coverage: bool,
    /// Skip the libFuzzer smoke even when `cargo-fuzz` is installed.
    pub skip_fuzz: bool,
    /// Skip dashboard refresh even when a release `rcc` binary and suites exist.
    pub skip_conformance: bool,
    /// Also run the crates.io-facing package archive check. This requires the
    /// internal crate graph to already be registry-resolvable.
    pub registry_package: bool,
}

/// Run the release-candidate gate suite.
pub fn run(root: &Path, opts: &ReleaseCheckOptions) -> Result<()> {
    let report_dir = root.join(&opts.report_dir);
    fs::create_dir_all(&report_dir)
        .with_context(|| format!("creating {}", report_dir.display()))?;

    let mut results = Vec::new();
    validate_publish_package(root)?;
    results.push(GateResult::passed(
        "publish manifest",
        "rcc-compiler package exposes binary `rcc` and a versioned rcc_driver path dependency",
    ));

    results.push(run_gate(root, &report_dir, "fmt", "cargo", &["fmt", "--all", "--check"], true)?);
    results.push(run_gate(
        root,
        &report_dir,
        "clippy",
        "cargo",
        &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        true,
    )?);
    results.push(run_gate(root, &report_dir, "test", "cargo", &["test", "--workspace"], true)?);

    if opts.skip_llvm {
        results.push(GateResult::skipped("llvm tests", "skipped by --skip-llvm"));
    } else if has_llvm_prefix() {
        results.push(run_gate(
            root,
            &report_dir,
            "llvm-tests",
            "cargo",
            &["test", "--workspace", "--features", "rcc_codegen_llvm/llvm"],
            true,
        )?);
    } else {
        results.push(GateResult::skipped(
            "llvm tests",
            "set RCC_LLVM_PREFIX or LLVM_SYS_181_PREFIX to run the LLVM-enabled gate",
        ));
    }

    if opts.skip_coverage {
        results.push(GateResult::skipped("coverage", "skipped by --skip-coverage"));
    } else if tool_available(root, "cargo", &["llvm-cov", "--version"]) {
        results.push(run_gate(
            root,
            &report_dir,
            "coverage",
            "cargo",
            &[
                "xtask",
                "coverage",
                "--lcov",
                "lcov.info",
                "--json",
                "target/coverage/coverage-summary.json",
            ],
            true,
        )?);
    } else {
        results.push(GateResult::skipped(
            "coverage",
            "install cargo-llvm-cov to run the coverage threshold gate",
        ));
    }

    if opts.skip_fuzz {
        results.push(GateResult::skipped("fuzz smoke", "skipped by --skip-fuzz"));
    } else if !cfg!(target_os = "linux") {
        results.push(GateResult::skipped(
            "fuzz smoke",
            "libFuzzer release smoke is a Linux/CI gate for this project",
        ));
    } else if tool_available(root, "cargo", &["fuzz", "--version"]) {
        let fuzz_dir = root.join("fuzz");
        results.push(run_gate_in_dir(
            &fuzz_dir,
            &report_dir,
            "fuzz-smoke",
            "cargo",
            &[
                "+nightly",
                "fuzz",
                "run",
                "lex",
                "--target",
                "x86_64-unknown-linux-gnu",
                "--",
                "-max_total_time=30",
            ],
            true,
        )?);
    } else {
        results.push(GateResult::skipped(
            "fuzz smoke",
            "install cargo-fuzz and a nightly toolchain to run the fuzz smoke gate",
        ));
    }

    if opts.skip_conformance {
        results.push(GateResult::skipped("conformance", "skipped by --skip-conformance"));
    } else if let Some(rcc) = release_rcc_binary(root) {
        if conformance_suites_present(root) {
            let rcc_arg = rcc.to_string_lossy().to_string();
            results.push(run_gate(
                root,
                &report_dir,
                "conformance-run",
                "cargo",
                &[
                    "run",
                    "--release",
                    "--package",
                    "rcc_conformance",
                    "--bin",
                    "rcc_conformance_run",
                    "--",
                    "--rcc",
                    &rcc_arg,
                    "--suite",
                    "c-testsuite",
                    "--suite",
                    "chibicc",
                    "--suite",
                    "tcc-tests2",
                    "--suite",
                    "llvm-test-suite",
                    "--mode",
                    "stage-1-3",
                    "--output",
                    "docs/conformance.json",
                ],
                true,
            )?);
            results.push(run_gate(
                root,
                &report_dir,
                "conformance-render",
                "cargo",
                &[
                    "run",
                    "--release",
                    "--package",
                    "rcc_conformance",
                    "--bin",
                    "rcc_conformance_render",
                    "--",
                    "--input",
                    "docs/conformance.json",
                    "--output",
                    "docs/conformance.md",
                ],
                true,
            )?);
        } else {
            results.push(GateResult::skipped(
                "conformance",
                "fetch testsuites first: cargo xtask fetch-testsuites --include-gpl",
            ));
        }
    } else {
        results.push(GateResult::skipped(
            "conformance",
            "build target/release/rcc with the LLVM backend before refreshing the dashboard",
        ));
    }

    results.push(run_gate(
        root,
        &report_dir,
        "package-wrapper-fmt",
        "cargo",
        &["fmt", "--manifest-path", "crates/rcc_compiler_package/Cargo.toml", "--check"],
        true,
    )?);
    results.push(run_gate(
        root,
        &report_dir,
        "package-wrapper-check",
        "cargo",
        &[
            "check",
            "--manifest-path",
            "crates/rcc_compiler_package/Cargo.toml",
            "--bin",
            "rcc",
            "--no-default-features",
        ],
        true,
    )?);
    if has_llvm_prefix() {
        results.push(run_gate(
            root,
            &report_dir,
            "package-default-llvm-check",
            "cargo",
            &["check", "--manifest-path", "crates/rcc_compiler_package/Cargo.toml", "--bin", "rcc"],
            true,
        )?);
    } else {
        results.push(GateResult::skipped(
            "package default LLVM",
            "set RCC_LLVM_PREFIX or LLVM_SYS_181_PREFIX to verify default cargo install features",
        ));
    }
    if opts.registry_package {
        results.push(run_gate(
            root,
            &report_dir,
            "package-archive",
            "cargo",
            &[
                "package",
                "--manifest-path",
                "crates/rcc_compiler_package/Cargo.toml",
                "--allow-dirty",
                "--no-verify",
            ],
            true,
        )?);
    } else {
        results.push(GateResult::skipped(
            "package archive",
            "task 13-14 must publish or otherwise make internal crates registry-resolvable before running --registry-package",
        ));
    }

    let summary = render_summary(&results);
    fs::write(report_dir.join("summary.md"), &summary)
        .with_context(|| format!("writing {}", report_dir.join("summary.md").display()))?;
    print!("{summary}");

    let failures: Vec<_> = results.iter().filter(|result| result.failed_hard()).collect();
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "release check failed: {}",
            failures.iter().map(|result| result.name).collect::<Vec<_>>().join(", ")
        );
    }
}

fn run_gate(
    root: &Path,
    report_dir: &Path,
    name: &'static str,
    program: &str,
    args: &[&str],
    mandatory: bool,
) -> Result<GateResult> {
    run_gate_in_dir(root, report_dir, name, program, args, mandatory)
}

fn run_gate_in_dir(
    cwd: &Path,
    report_dir: &Path,
    name: &'static str,
    program: &str,
    args: &[&str],
    mandatory: bool,
) -> Result<GateResult> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("spawning {}", format_command(program, args)))?;
    let status = output.status;
    let log = render_log(name, cwd, program, args, &output.stdout, &output.stderr, status);
    fs::write(report_dir.join(format!("{name}.log")), log)
        .with_context(|| format!("writing {name} release-check log"))?;
    if status.success() {
        Ok(GateResult::passed(name, "command succeeded"))
    } else {
        Ok(GateResult::failed(
            name,
            format!("{} exited with {status}", format_command(program, args)),
            mandatory,
        ))
    }
}

fn render_log(
    name: &str,
    cwd: &Path,
    program: &str,
    args: &[&str],
    stdout: &[u8],
    stderr: &[u8],
    status: std::process::ExitStatus,
) -> String {
    format!(
        "# {name}\n\ncwd: {}\ncommand: {}\nstatus: {status}\n\n## stdout\n\n{}\n\n## stderr\n\n{}\n",
        cwd.display(),
        format_command(program, args),
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
}

fn format_command(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_./:=+".contains(ch)) {
        arg.to_string()
    } else {
        format!("\"{}\"", arg.replace('"', "\\\""))
    }
}

fn tool_available(root: &Path, program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .is_ok_and(|out| out.status.success())
}

fn has_llvm_prefix() -> bool {
    ["RCC_LLVM_PREFIX", "LLVM_SYS_181_PREFIX", "LLVM_SYS_180_PREFIX", "LLVM_PREFIX"]
        .iter()
        .any(|name| env::var(name).is_ok_and(|value| !value.is_empty()))
}

fn release_rcc_binary(root: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "rcc.exe" } else { "rcc" };
    let path = root.join("target/release").join(exe);
    path.exists().then_some(path)
}

fn conformance_suites_present(root: &Path) -> bool {
    [
        "third_party/testsuites/c-testsuite/tests/single-exec",
        "third_party/testsuites/chibicc/test",
        "third_party/testsuites/tcc-tests2/tests/tests2",
        "third_party/testsuites/llvm-test-suite/SingleSource/UnitTests",
    ]
    .iter()
    .all(|rel| root.join(rel).is_dir())
}

fn validate_publish_package(root: &Path) -> Result<()> {
    let manifest_path = root.join("crates/rcc_compiler_package/Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    validate_publish_manifest_text(&text)
}

fn validate_publish_manifest_text(text: &str) -> Result<()> {
    let manifest: Value = toml::from_str(text).context("parsing rcc-compiler manifest")?;
    let package =
        manifest.get("package").and_then(Value::as_table).context("manifest missing [package]")?;
    if package.get("name").and_then(Value::as_str) != Some("rcc-compiler") {
        bail!("publish package name must be rcc-compiler");
    }
    if package.get("publish").and_then(Value::as_bool) == Some(false) {
        bail!("rcc-compiler package must be publishable");
    }
    let bins =
        manifest.get("bin").and_then(Value::as_array).context("manifest must define [[bin]]")?;
    if !bins.iter().any(|bin| {
        bin.as_table().and_then(|table| table.get("name")).and_then(Value::as_str) == Some("rcc")
    }) {
        bail!("rcc-compiler package must install a binary named rcc");
    }
    let driver_dep = manifest
        .get("dependencies")
        .and_then(Value::as_table)
        .and_then(|deps| deps.get("rcc_driver"))
        .and_then(Value::as_table)
        .context("rcc-compiler must depend on rcc_driver with explicit version and path")?;
    if driver_dep.get("path").and_then(Value::as_str).is_none() {
        bail!("rcc_driver dependency must keep a local path for development");
    }
    if driver_dep.get("version").and_then(Value::as_str).is_none() {
        bail!("rcc_driver dependency must carry a version for packaging");
    }
    let default_features = manifest
        .get("features")
        .and_then(Value::as_table)
        .and_then(|features| features.get("default"))
        .and_then(Value::as_array)
        .context("rcc-compiler must define default features")?;
    if !default_features.iter().any(|feature| feature.as_str() == Some("llvm")) {
        bail!("cargo install rcc-compiler must enable the LLVM backend by default");
    }
    Ok(())
}

fn render_summary(results: &[GateResult]) -> String {
    let mut out = String::new();
    out.push_str("# Release Check Summary\n\n");
    out.push_str("| gate | status | detail |\n");
    out.push_str("| ---- | ------ | ------ |\n");
    for result in results {
        out.push_str(&format!("| {} | {} | {} |\n", result.name, result.status, result.detail));
    }
    out
}

#[derive(Debug)]
struct GateResult {
    name: &'static str,
    status: &'static str,
    detail: String,
    mandatory: bool,
}

impl GateResult {
    fn passed(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: "pass", detail: detail.into(), mandatory: true }
    }

    fn skipped(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: "skip", detail: detail.into(), mandatory: false }
    }

    fn failed(name: &'static str, detail: impl Into<String>, mandatory: bool) -> Self {
        Self { name, status: "fail", detail: detail.into(), mandatory }
    }

    fn failed_hard(&self) -> bool {
        self.mandatory && self.status == "fail"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_real_distribution_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        validate_publish_package(root).unwrap();
    }

    #[test]
    fn rejects_package_without_rcc_binary() {
        let manifest = r#"
            [package]
            name = "rcc-compiler"
            version = "0.0.1"
            publish = true

            [[bin]]
            name = "not-rcc"

            [dependencies]
            rcc_driver = { version = "0.0.1", path = "../rcc_driver" }

            [features]
            default = ["llvm"]
            llvm = ["rcc_driver/llvm"]
        "#;
        assert!(validate_publish_manifest_text(manifest).is_err());
    }

    #[test]
    fn formats_command_arguments_with_spaces() {
        assert_eq!(
            format_command("cargo", &["package", "--manifest-path", "path with spaces/Cargo.toml"]),
            "cargo package --manifest-path \"path with spaces/Cargo.toml\""
        );
    }
}
