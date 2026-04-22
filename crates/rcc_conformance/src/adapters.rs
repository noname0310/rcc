//! Concrete suite adapters. Each one wraps the discovery + execution
//! strategy of a vendored suite. Implementations are stubs pending M0.5
//! follow-up; interfaces are frozen so suites can be added independently.

use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Adapter, Outcome, TestCase};

/// Per-test timeout for the compile → link → execute pipeline.
const TIMEOUT: Duration = Duration::from_secs(30);

/// `c-testsuite` adapter. Enumerates `tests/single-exec/*.c` and compares
/// `<name>.expected` stdout vs `rcc`-produced stdout.
pub struct CTestSuiteAdapter;

impl CTestSuiteAdapter {
    /// Pure comparison logic extracted for testability.
    ///
    /// Checks `exit_code == 0` and `actual_stdout` matches the contents of
    /// `expected_path` byte-for-byte. Returns the appropriate [`Outcome`].
    pub fn compare_outcome(
        actual_stdout: &[u8],
        exit_code: Option<i32>,
        expected_path: &Path,
    ) -> Outcome {
        let expected = match std::fs::read(expected_path) {
            Ok(e) => e,
            Err(e) => {
                return Outcome::Skip {
                    reason: format!("cannot read {}: {e}", expected_path.display()),
                };
            }
        };

        if exit_code != Some(0) {
            return Outcome::Fail {
                reason: format!(
                    "non-zero exit code: {}",
                    exit_code.map_or_else(|| "killed by signal".into(), |c| c.to_string()),
                ),
            };
        }

        if actual_stdout != expected.as_slice() {
            return Outcome::Fail { reason: "stdout mismatch".into() };
        }

        Outcome::Pass
    }
}

impl Adapter for CTestSuiteAdapter {
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
        let dir = root.join("tests").join("single-exec");
        anyhow::ensure!(
            dir.is_dir(),
            "c-testsuite single-exec directory not found: {}",
            dir.display(),
        );

        let mut cases = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("c") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("non-UTF-8 filename: {}", path.display()))?;
            cases.push(TestCase { id: format!("c-testsuite::{stem}"), path });
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let expected_path = case.path.with_extension("c.expected");
        if !expected_path.exists() {
            return Ok(Outcome::Skip { reason: format!("no .expected file for {}", case.id) });
        }

        let tmp = tempfile::tempdir()?;
        let obj_path = tmp.path().join("test.o");
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        // Step 1: compile with rcc --emit=obj
        let mut compile_cmd = Command::new(rcc_path);
        compile_cmd.arg("--emit=obj").arg("-o").arg(&obj_path).arg(&case.path);
        match run_with_timeout(&mut compile_cmd, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "rcc compilation failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") });
            }
        }

        // Step 2: link with host cc
        let mut link_cmd = Command::new("cc");
        link_cmd.arg("-o").arg(&exe_path).arg(&obj_path);
        match run_with_timeout(&mut link_cmd, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "link failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("link invocation failed: {e}") });
            }
        }

        // Step 3: execute and compare
        let mut exec_cmd = Command::new(&exe_path);
        match run_with_timeout(&mut exec_cmd, TIMEOUT) {
            Ok(output) => {
                Ok(Self::compare_outcome(&output.stdout, output.status.code(), &expected_path))
            }
            Err(e) => Ok(Outcome::Fail { reason: format!("execution failed: {e}") }),
        }
    }
}

/// Spawn a process and wait for completion, killing it if `timeout` elapses.
///
/// stdout and stderr are read in background threads to avoid pipe-buffer
/// deadlocks on large outputs.
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> anyhow::Result<std::process::Output> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_thread = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut r) = stdout_pipe {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });

    let stderr_thread = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut r) = stderr_pipe {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("timed out after {}s", timeout.as_secs());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(std::process::Output { status, stdout, stderr })
}

/// `chibicc` adapter. Runs the `chibicc/test/*.c` files the same way chibicc's
/// Makefile does: compile each test together with `test/common`, link, run,
/// check exit code == 0.
pub struct ChibiccAdapter;

impl ChibiccAdapter {
    /// Path to the support file compiled alongside every test.
    fn common_path(root: &Path) -> std::path::PathBuf {
        root.join("test").join("common")
    }
}

impl Adapter for ChibiccAdapter {
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
        let dir = root.join("test");
        anyhow::ensure!(dir.is_dir(), "chibicc test directory not found: {}", dir.display(),);

        let mut cases = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("c") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("non-UTF-8 filename: {}", path.display()))?;
            if stem == "common" {
                continue;
            }
            cases.push(TestCase { id: format!("chibicc::{stem}"), path });
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let common =
            Self::common_path(case.path.parent().and_then(|p| p.parent()).ok_or_else(|| {
                anyhow::anyhow!("cannot derive suite root from {}", case.path.display())
            })?);

        if !common.exists() {
            return Ok(Outcome::Skip {
                reason: format!("common helper not found: {}", common.display()),
            });
        }

        let tmp = tempfile::tempdir()?;
        let test_obj = tmp.path().join("test.o");
        let common_obj = tmp.path().join("common.o");
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        // Step 1: compile the test file with rcc
        let mut compile_test = Command::new(rcc_path);
        compile_test.arg("--emit=obj").arg("-o").arg(&test_obj).arg(&case.path);
        match run_with_timeout(&mut compile_test, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "rcc compilation failed for {}: exit {}; {}",
                        case.path.display(),
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") });
            }
        }

        // Step 2: compile common with rcc
        let mut compile_common = Command::new(rcc_path);
        compile_common.arg("--emit=obj").arg("-o").arg(&common_obj).arg(&common);
        match run_with_timeout(&mut compile_common, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "rcc compilation failed for common: exit {}; {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail {
                    reason: format!("rcc invocation failed for common: {e}"),
                });
            }
        }

        // Step 3: link both objects with host cc
        let mut link_cmd = Command::new("cc");
        link_cmd.arg("-o").arg(&exe_path).arg(&test_obj).arg(&common_obj);
        match run_with_timeout(&mut link_cmd, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "link failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("link invocation failed: {e}") });
            }
        }

        // Step 4: execute and check exit code == 0
        let mut exec_cmd = Command::new(&exe_path);
        match run_with_timeout(&mut exec_cmd, TIMEOUT) {
            Ok(output) => {
                if output.status.code() == Some(0) {
                    Ok(Outcome::Pass)
                } else {
                    Ok(Outcome::Fail {
                        reason: format!(
                            "non-zero exit code: {}",
                            output
                                .status
                                .code()
                                .map_or_else(|| "killed by signal".into(), |c| c.to_string()),
                        ),
                    })
                }
            }
            Err(e) => Ok(Outcome::Fail { reason: format!("execution failed: {e}") }),
        }
    }
}

/// `gcc-torture` adapter (GPL-licensed; gated by `--include-gpl`).
pub struct GccTortureAdapter;

impl Adapter for GccTortureAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "gcc-torture adapter not yet implemented".into() })
    }
}

/// `tcc-tests2` adapter (LGPL).
pub struct TccTests2Adapter;

impl Adapter for TccTests2Adapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "tcc-tests2 adapter not yet implemented".into() })
    }
}

/// `llvm-test-suite` SingleSource adapter.
pub struct LlvmTestSuiteAdapter;

impl Adapter for LlvmTestSuiteAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "llvm-test-suite adapter not yet implemented".into() })
    }
}

/// Differential-fuzzing driver built on top of `csmith`. Not a suite proper —
/// it generates fresh programs each run.
pub struct CsmithDifferentialAdapter;

impl Adapter for CsmithDifferentialAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "csmith differential not yet implemented".into() })
    }
}
