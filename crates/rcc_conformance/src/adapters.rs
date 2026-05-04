//! Concrete suite adapters. Each one wraps the discovery + execution
//! strategy of a vendored suite. Implementations are stubs pending M0.5
//! follow-up; interfaces are frozen so suites can be added independently.

use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{metadata, Adapter, Outcome, TestCase};

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

        let actual_stdout = normalize_stdout_newlines(actual_stdout);
        let expected = normalize_stdout_newlines(&expected);

        if actual_stdout != expected {
            return Outcome::Fail { reason: "stdout mismatch".into() };
        }

        Outcome::Pass
    }
}

/// Normalize text-mode stdout for vendored c-testsuite expected files.
///
/// The upstream suite's `.expected` files are text fixtures. On Windows
/// checkouts they may appear with CRLF line endings while Linux test
/// executables print LF. Normalize CRLF to LF so the conformance result does
/// not depend on the host Git checkout policy.
fn normalize_stdout_newlines(bytes: &[u8]) -> Vec<u8> {
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
        if let Some(reason) = metadata::unspecified_eval_order_reason(&case.id) {
            return Ok(Outcome::Skip { reason: reason.to_owned() });
        }

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
        if !cfg!(windows) {
            link_cmd.arg("-lm");
        }
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

/// Execution mode for [`ChibiccAdapter`].
///
/// The same fixture tree is reused for two distinct M5 / M6 KPIs:
/// phase-04 (preprocessor) and phase-07 (full compile + link + run).
/// Rather than duplicating discovery, the adapter carries a mode
/// field selected at construction.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChibiccMode {
    /// Full pipeline: compile every fixture (plus `test/common`), link
    /// with host `cc`, execute, assert exit code 0. This is the
    /// historical chibicc Makefile behaviour.
    Compile,
    /// Preprocess-only (task 04-18): run `rcc --emit=pp -I<testdir>`
    /// against the preprocessor-focused fixtures
    /// (`macro.c`, `typedef.c`, and `include.c` if vendored) and
    /// assert exit code 0. Fixtures outside that set are filtered
    /// out during discovery so they do not count toward the
    /// pass / fail / skip totals.
    Preprocess,
    /// Stage-isolated compile + link + run path for the early chibicc
    /// fixtures (`arith.c`, `control.c`, and `function.c`).
    ///
    /// Unlike [`ChibiccMode::Compile`], this mode never compiles upstream
    /// `test/common` with `rcc`. Most early fixtures link a generated
    /// host-compiled support object containing only the `assert` helper.
    /// `function.c` is the exception: it needs the helper functions from
    /// `test/common`, so that file is compiled with host `cc`.
    Stages1To3,
}

/// `chibicc` adapter. Runs the `chibicc/test/*.c` files the same way chibicc's
/// Makefile does: compile each test together with `test/common`, link, run,
/// check exit code == 0.
pub struct ChibiccAdapter {
    /// Which pipeline stage the adapter exercises per test case.
    pub mode: ChibiccMode,
}

impl ChibiccAdapter {
    /// Adapter wired to the full compile + link + run pipeline
    /// (phase 07 / milestone M6). Equivalent to the unit-variant
    /// form this type used to carry.
    pub const fn compile() -> Self {
        Self { mode: ChibiccMode::Compile }
    }

    /// Adapter restricted to the preprocessor-only subset
    /// (phase 04 / task 04-18 / milestone M5). Discovery emits only
    /// the fixtures whose stem is in [`Self::PREPROCESS_FIXTURES`].
    pub const fn preprocess() -> Self {
        Self { mode: ChibiccMode::Preprocess }
    }

    /// Adapter restricted to the stage-1..3 chibicc slice. Discovery emits
    /// only `arith.c`, `control.c`, and `function.c`. Execution uses host
    /// `cc` for support code so `rcc` failures stay isolated to the selected
    /// fixture.
    pub const fn stages1_to_3() -> Self {
        Self { mode: ChibiccMode::Stages1To3 }
    }

    /// File stems of the chibicc fixtures that exist to exercise
    /// `#define` / `#include` / conditional compilation without
    /// requiring the downstream parser / codegen. Kept in lock-step
    /// with `tasks/04-preprocess/18-chibicc-preprocess-tests.md`.
    pub const PREPROCESS_FIXTURES: &'static [&'static str] = &["macro", "typedef", "include"];

    /// File stems of the stage-isolated chibicc fixtures.
    pub const STAGE_1_TO_3_FIXTURES: &'static [&'static str] = &["arith", "control", "function"];

    /// Whether a stage-isolated case needs upstream chibicc `test/common`
    /// instead of the generated one-function support source.
    #[must_use]
    pub fn uses_stage_common(case_id: &str) -> bool {
        case_id == "chibicc::function"
    }

    /// Path to the support file compiled alongside every test.
    fn common_path(root: &Path) -> std::path::PathBuf {
        root.join("test").join("common")
    }

    /// Minimal host-compiled support source used by stage-isolated mode.
    fn stage_support_source() -> &'static str {
        r#"#include <stdio.h>
#include <stdlib.h>

void assert(int expected, int actual, char *code) {
  if (expected == actual)
    return;
  fprintf(stderr, "%s => %d expected but got %d\n", code, expected, actual);
  exit(1);
}
"#
    }
}

impl Default for ChibiccAdapter {
    fn default() -> Self {
        Self::compile()
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
            if self.mode == ChibiccMode::Preprocess && !Self::PREPROCESS_FIXTURES.contains(&stem) {
                continue;
            }
            if self.mode == ChibiccMode::Stages1To3 && !Self::STAGE_1_TO_3_FIXTURES.contains(&stem)
            {
                continue;
            }
            cases.push(TestCase { id: format!("chibicc::{stem}"), path });
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        if let Some(reason) = metadata::unspecified_eval_order_reason(&case.id) {
            return Ok(Outcome::Skip { reason: reason.to_owned() });
        }

        match self.mode {
            ChibiccMode::Compile => self.run_compile(rcc_path, case),
            ChibiccMode::Preprocess => self.run_preprocess_only(rcc_path, case),
            ChibiccMode::Stages1To3 => self.run_stage_1_to_3(rcc_path, case),
        }
    }
}

impl ChibiccAdapter {
    /// Preprocess-only execution path.
    ///
    /// Invokes `rcc --emit=pp -I<case_dir>` against the fixture and
    /// treats exit code 0 as a pass. The `-I` flag points at the
    /// fixture directory so `#include "test.h"` (which every chibicc
    /// test starts with) resolves against the vendored header. The
    /// driver short-circuits after phase-4 when `--emit=pp` is the
    /// only stage requested (see `rcc_driver::pipeline`) and returns
    /// a non-zero exit code if the preprocessor emitted any error
    /// diagnostics.
    fn run_preprocess_only(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let case_dir = case.path.parent().ok_or_else(|| {
            anyhow::anyhow!("cannot derive directory from {}", case.path.display())
        })?;
        let mut cmd = Command::new(rcc_path);
        cmd.arg("--emit=pp").arg("-I").arg(case_dir).arg(&case.path);
        match run_with_timeout(&mut cmd, TIMEOUT) {
            Ok(o) if o.status.success() => Ok(Outcome::Pass),
            Ok(o) => Ok(Outcome::Fail {
                reason: format!(
                    "rcc --emit=pp failed for {} (exit {}): {}",
                    case.path.display(),
                    o.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                ),
            }),
            Err(e) => Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") }),
        }
    }

    /// Stage-isolated compile + link + execute path.
    ///
    /// The selected fixture is compiled by `rcc`, while support code is
    /// compiled with host `cc`. Most fixtures use a generated `assert` helper;
    /// `function.c` uses upstream `test/common` because it also needs helper
    /// functions such as `true_fn`, `add_all`, and `struct_test4`.
    fn run_stage_1_to_3(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let case_dir = case.path.parent().ok_or_else(|| {
            anyhow::anyhow!("cannot derive directory from {}", case.path.display())
        })?;

        let tmp = tempfile::tempdir()?;
        let test_obj = tmp.path().join("test.o");
        let support_obj = tmp.path().join("stage_support.o");
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        // Step 1: compile the selected test file with rcc. `-I<case_dir>`
        // makes the dependency on chibicc's `test.h` explicit.
        let mut compile_test = Command::new(rcc_path);
        compile_test
            .arg("--emit=obj")
            .arg("-fgnu-binary-literals")
            .arg("-fgnu-statement-expressions")
            .arg("-fgnu-omitted-conditional-operand")
            .arg("-fgnu-conditional-void-operand")
            .arg("-fgnu-case-ranges")
            .arg("-fgnu-labels-as-values")
            .arg("-fgnu-lvalue-comma")
            .arg("-I")
            .arg(case_dir)
            .arg("-o")
            .arg(&test_obj)
            .arg(&case.path);
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

        // Step 2: compile the support helper with host cc. `function.c` needs
        // the upstream helper functions; smaller fixtures only need `assert`.
        let mut compile_support = Command::new("cc");
        compile_support.arg("-std=c99").arg("-c");
        if Self::uses_stage_common(&case.id) {
            let common = case_dir.join("common");
            if !common.exists() {
                return Ok(Outcome::Fail {
                    reason: format!("stage common helper not found: {}", common.display()),
                });
            }
            compile_support.arg("-x").arg("c").arg(&common);
        } else {
            let support_src = tmp.path().join("stage_support.c");
            std::fs::write(&support_src, Self::stage_support_source())?;
            compile_support.arg(&support_src);
        }
        compile_support.arg("-o").arg(&support_obj);
        match run_with_timeout(&mut compile_support, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "stage support compilation failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail {
                    reason: format!("stage support invocation failed: {e}"),
                });
            }
        }

        // Step 3: link both objects with host cc.
        let mut link_cmd = Command::new("cc");
        link_cmd.arg("-o").arg(&exe_path).arg(&test_obj).arg(&support_obj);
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

        // Step 4: execute and check exit code == 0.
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

    /// Historical compile + link + execute path (milestone M6).
    fn run_compile(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
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
