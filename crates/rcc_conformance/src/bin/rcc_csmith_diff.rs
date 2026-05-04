//! `csmith` differential runner.
//!
//! Generates random C programs, compiles/runs each with both `rcc` and the
//! host C compiler, and reports any exit-code/stdout disagreement.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(name = "rcc_csmith_diff")]
struct Cli {
    #[arg(long, default_value = "target/release/rcc")]
    rcc: PathBuf,
    #[arg(long, default_value = "cc")]
    host_cc: PathBuf,
    #[arg(long)]
    csmith: Option<PathBuf>,
    #[arg(long)]
    runtime_include: Option<PathBuf>,
    #[arg(long, default_value_t = 1)]
    iterations: u32,
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long, default_value_t = 10 * 1024)]
    max_source_bytes: usize,
    #[arg(long, default_value_t = 5)]
    timeout_secs: u64,
    #[arg(long)]
    max_duration_secs: Option<u64>,
    #[arg(long, default_value = "target/csmith-diff")]
    work_dir: PathBuf,
    #[arg(long, default_value = "target/csmith-diff/report.json")]
    output: PathBuf,
    #[arg(long)]
    keep_passing: bool,
}

#[derive(Clone, Debug)]
struct Config {
    rcc: PathBuf,
    host_cc: PathBuf,
    csmith: PathBuf,
    runtime_include: PathBuf,
    iterations: u32,
    seed: u64,
    max_source_bytes: usize,
    timeout: Duration,
    max_duration: Option<Duration>,
    work_dir: PathBuf,
    keep_passing: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    seed: u64,
    iterations: u32,
    cases: Vec<CaseReport>,
}

impl Report {
    fn has_failures(&self) -> bool {
        self.cases.iter().any(|case| {
            !matches!(case.outcome, CaseOutcome::Pass | CaseOutcome::SkippedTooLarge { .. })
        })
    }
}

#[derive(Debug, Serialize)]
struct CaseReport {
    id: u32,
    seed: u64,
    source: PathBuf,
    outcome: CaseOutcome,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CaseOutcome {
    Pass,
    SkippedTooLarge {
        bytes: usize,
    },
    GeneratorFailed {
        stderr: String,
    },
    HostCompileFailed {
        stderr: String,
    },
    RccCompileFailed {
        stderr: String,
    },
    HostRunFailed {
        reason: String,
    },
    RccRunFailed {
        reason: String,
    },
    Disagreement {
        host_status: Option<i32>,
        rcc_status: Option<i32>,
        host_stdout: String,
        rcc_stdout: String,
    },
}

#[derive(Debug)]
struct ExecResult {
    status: Option<i32>,
    stdout: Vec<u8>,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(report) => {
            let failures = report.has_failures();
            eprintln!(
                "csmith differential: {} case(s), {} failure(s)",
                report.cases.len(),
                report
                    .cases
                    .iter()
                    .filter(|case| !matches!(
                        case.outcome,
                        CaseOutcome::Pass | CaseOutcome::SkippedTooLarge { .. }
                    ))
                    .count()
            );
            if failures {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run_cli() -> anyhow::Result<Report> {
    let cli = Cli::parse();
    let root = project_root();
    let config = Config {
        rcc: cli.rcc,
        host_cc: cli.host_cc,
        csmith: cli.csmith.unwrap_or_else(|| default_csmith_path(&root)),
        runtime_include: cli
            .runtime_include
            .unwrap_or_else(|| root.join("third_party/testsuites/csmith/runtime")),
        iterations: cli.iterations,
        seed: cli.seed.unwrap_or_else(default_seed),
        max_source_bytes: cli.max_source_bytes,
        timeout: Duration::from_secs(cli.timeout_secs),
        max_duration: cli.max_duration_secs.map(Duration::from_secs),
        work_dir: cli.work_dir,
        keep_passing: cli.keep_passing,
    };

    let report = run(config)?;
    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&cli.output, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("writing {}", cli.output.display()))?;
    Ok(report)
}

fn run(config: Config) -> anyhow::Result<Report> {
    if config.iterations == 0 {
        bail!("--iterations must be greater than zero");
    }
    std::fs::create_dir_all(&config.work_dir)
        .with_context(|| format!("creating {}", config.work_dir.display()))?;

    let mut cases = Vec::new();
    let start = Instant::now();
    for id in 0..config.iterations {
        if config.max_duration.is_some_and(|limit| start.elapsed() >= limit) {
            break;
        }
        let seed = config.seed + u64::from(id);
        let case_dir = config.work_dir.join(format!("{seed}"));
        std::fs::create_dir_all(&case_dir)
            .with_context(|| format!("creating {}", case_dir.display()))?;
        let source = case_dir.join("case.c");

        let generated = generate_csmith(&config.csmith, seed, config.timeout);
        let outcome = match generated {
            Ok(output) if output.status.success() => {
                std::fs::write(&source, &output.stdout)
                    .with_context(|| format!("writing {}", source.display()))?;
                if output.stdout.len() > config.max_source_bytes {
                    CaseOutcome::SkippedTooLarge { bytes: output.stdout.len() }
                } else {
                    run_case(&config, &case_dir, &source)?
                }
            }
            Ok(output) => {
                CaseOutcome::GeneratorFailed { stderr: truncate_utf8(&output.stderr, 1024) }
            }
            Err(err) => CaseOutcome::GeneratorFailed { stderr: err.to_string() },
        };

        let passed = matches!(outcome, CaseOutcome::Pass | CaseOutcome::SkippedTooLarge { .. });
        cases.push(CaseReport { id, seed, source: source.clone(), outcome });
        if passed && !config.keep_passing {
            let _ = std::fs::remove_dir_all(&case_dir);
        }
    }

    Ok(Report { seed: config.seed, iterations: config.iterations, cases })
}

fn run_case(config: &Config, case_dir: &Path, source: &Path) -> anyhow::Result<CaseOutcome> {
    let host_exe = executable_path(case_dir, "host");
    let rcc_exe = executable_path(case_dir, "rcc");

    let host_compile = compile_with_host_cc(
        &config.host_cc,
        &config.runtime_include,
        source,
        &host_exe,
        config.timeout,
    )?;
    if !host_compile.status.success() {
        return Ok(CaseOutcome::HostCompileFailed {
            stderr: truncate_utf8(&host_compile.stderr, 1024),
        });
    }

    let rcc_compile =
        compile_with_rcc(&config.rcc, &config.runtime_include, source, &rcc_exe, config.timeout)?;
    if !rcc_compile.status.success() {
        return Ok(CaseOutcome::RccCompileFailed {
            stderr: truncate_utf8(&rcc_compile.stderr, 1024),
        });
    }

    let host = match run_executable(&host_exe, config.timeout) {
        Ok(result) => result,
        Err(err) => return Ok(CaseOutcome::HostRunFailed { reason: err.to_string() }),
    };
    let rcc = match run_executable(&rcc_exe, config.timeout) {
        Ok(result) => result,
        Err(err) => return Ok(CaseOutcome::RccRunFailed { reason: err.to_string() }),
    };

    Ok(compare_exec(&host, &rcc))
}

fn generate_csmith(csmith: &Path, seed: u64, timeout: Duration) -> anyhow::Result<Output> {
    let mut cmd = Command::new(csmith);
    cmd.arg("--seed")
        .arg(seed.to_string())
        .arg("--max-funcs")
        .arg("4")
        .arg("--max-block-size")
        .arg("4")
        .arg("--max-expr-complexity")
        .arg("8")
        .arg("--max-array-dim")
        .arg("2")
        .arg("--max-array-len-per-dim")
        .arg("8");
    run_command(&mut cmd, timeout)
}

fn compile_with_host_cc(
    cc: &Path,
    runtime_include: &Path,
    source: &Path,
    exe: &Path,
    timeout: Duration,
) -> anyhow::Result<Output> {
    let mut cmd = Command::new(cc);
    cmd.arg("-std=c99").arg("-O0").arg("-I").arg(runtime_include).arg(source).arg("-o").arg(exe);
    if !cfg!(windows) {
        cmd.arg("-lm");
    }
    run_command(&mut cmd, timeout)
}

fn compile_with_rcc(
    rcc: &Path,
    runtime_include: &Path,
    source: &Path,
    exe: &Path,
    timeout: Duration,
) -> anyhow::Result<Output> {
    let mut cmd = Command::new(rcc);
    cmd.arg("-I").arg(runtime_include).arg(source).arg("-o").arg(exe);
    run_command(&mut cmd, timeout)
}

fn run_executable(exe: &Path, timeout: Duration) -> anyhow::Result<ExecResult> {
    let mut cmd = Command::new(exe);
    let output = run_command(&mut cmd, timeout)?;
    Ok(ExecResult { status: output.status.code(), stdout: output.stdout })
}

fn compare_exec(host: &ExecResult, rcc: &ExecResult) -> CaseOutcome {
    if host.status == rcc.status && normalize_stdout(&host.stdout) == normalize_stdout(&rcc.stdout)
    {
        return CaseOutcome::Pass;
    }
    CaseOutcome::Disagreement {
        host_status: host.status,
        rcc_status: rcc.status,
        host_stdout: truncate_utf8(&host.stdout, 2048),
        rcc_stdout: truncate_utf8(&rcc.stdout, 2048),
    }
}

fn normalize_stdout(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn run_command(cmd: &mut Command, timeout: Duration) -> anyhow::Result<Output> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut out) = stdout_pipe {
            let _ = out.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr_pipe {
            let _ = err.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            let status = child.wait()?;
            let stdout = stdout_handle.join().unwrap_or_default();
            let stderr = stderr_handle.join().unwrap_or_default();
            return Ok(Output { status, stdout, stderr });
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let status = child.wait()?;
            let stdout = stdout_handle.join().unwrap_or_default();
            let stderr = stderr_handle.join().unwrap_or_default();
            return Ok(Output { status, stdout, stderr });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn executable_path(dir: &Path, stem: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{stem}.exe"))
    } else {
        dir.join(stem)
    }
}

fn default_seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_secs())
}

fn default_csmith_path(root: &Path) -> PathBuf {
    let name = if cfg!(windows) { "csmith.exe" } else { "csmith" };
    let built = root.join("third_party/testsuites/csmith/build/src").join(name);
    if built.is_file() {
        built
    } else {
        PathBuf::from(name)
    }
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rcc_conformance lives under crates/")
        .to_path_buf()
}

fn truncate_utf8(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(status: Option<i32>, stdout: &[u8]) -> ExecResult {
        ExecResult { status, stdout: stdout.to_vec() }
    }

    #[test]
    fn stdout_comparison_normalizes_crlf() {
        assert!(matches!(
            compare_exec(&exec(Some(0), b"a\r\n"), &exec(Some(0), b"a\n")),
            CaseOutcome::Pass
        ));
    }

    #[test]
    fn stdout_or_status_mismatch_is_disagreement() {
        assert!(matches!(
            compare_exec(&exec(Some(0), b"a\n"), &exec(Some(1), b"b\n")),
            CaseOutcome::Disagreement { host_status: Some(0), rcc_status: Some(1), .. }
        ));
    }

    #[test]
    fn report_failure_predicate_ignores_large_skips() {
        let report = Report {
            seed: 1,
            iterations: 2,
            cases: vec![
                CaseReport {
                    id: 0,
                    seed: 1,
                    source: PathBuf::from("a.c"),
                    outcome: CaseOutcome::Pass,
                },
                CaseReport {
                    id: 1,
                    seed: 2,
                    source: PathBuf::from("b.c"),
                    outcome: CaseOutcome::SkippedTooLarge { bytes: 42 },
                },
            ],
        };
        assert!(!report.has_failures());
    }

    #[test]
    fn default_csmith_prefers_built_binary_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("third_party/testsuites/csmith/build/src");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) { "csmith.exe" } else { "csmith" });
        std::fs::write(&exe, "").unwrap();
        assert_eq!(default_csmith_path(tmp.path()), exe);
    }
}
