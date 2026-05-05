//! Concrete suite adapters. Each one wraps the discovery + execution
//! strategy of a vendored suite. Implementations are stubs pending M0.5
//! follow-up; interfaces are frozen so suites can be added independently.

use std::io::Read as _;
use std::path::{Path, PathBuf};
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
        cmd.arg("--emit=pp")
            .arg("-fgnu-permissive-redefinition")
            .arg("-fgnu-named-variadic")
            .arg("-fgnu-permissive-paste")
            .arg("-fgnu-va-args-elision")
            .arg("-I")
            .arg(case_dir)
            .arg(&case.path);
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
            .arg("-fgnu-function-names")
            .arg("-fgnu-va-area")
            .arg("-fgnu89-inline")
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

/// `gcc-torture` adapter.
///
/// The upstream checkout is still fetched via the historical
/// `--include-gpl` opt-in so ordinary `fetch-testsuites` does not pull
/// a large optional suite by accident.
pub struct GccTortureAdapter {
    mode: GccTortureMode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GccTortureMode {
    Smoke,
    FullExecute,
}

impl GccTortureAdapter {
    /// Smoke subset list relative to the fetched gcc-torture checkout.
    pub const SMOKE_SUBSET: &'static str = "smoke-subset.txt";

    /// Run only the tracked smoke subset.
    #[must_use]
    pub fn smoke() -> Self {
        Self { mode: GccTortureMode::Smoke }
    }

    /// Run every `gcc.c-torture/execute/*.c` file.
    #[must_use]
    pub fn full_execute() -> Self {
        Self { mode: GccTortureMode::FullExecute }
    }

    fn read_smoke_subset(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let path = root.join(Self::SMOKE_SUBSET);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| anyhow::anyhow!("cannot read {}: {err}", path.display()))?;
        let mut files = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.contains('\\') || Path::new(line).is_absolute() || line.contains("..") {
                anyhow::bail!(
                    "{}:{}: subset entries must be clean relative paths",
                    path.display(),
                    idx + 1
                );
            }
            files.push(root.join(line));
        }
        anyhow::ensure!(!files.is_empty(), "{} contains no smoke cases", path.display());
        Ok(files)
    }

    fn discover_full_execute(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let execute_dir = root.join("gcc/testsuite/gcc.c-torture/execute");
        anyhow::ensure!(
            execute_dir.is_dir(),
            "gcc-torture execute directory not found: {}",
            execute_dir.display()
        );
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&execute_dir)
            .map_err(|err| anyhow::anyhow!("cannot read {}: {err}", execute_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "c") {
                files.push(path);
            }
        }
        anyhow::ensure!(!files.is_empty(), "{} contains no .c files", execute_dir.display());
        Ok(files)
    }

    fn case_from_path(path: PathBuf) -> anyhow::Result<TestCase> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 filename: {}", path.display()))?;
        Ok(TestCase { id: format!("gcc-torture::execute::{stem}"), path })
    }
}

impl Adapter for GccTortureAdapter {
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
        let files = match self.mode {
            GccTortureMode::Smoke => Self::read_smoke_subset(root)?,
            GccTortureMode::FullExecute => Self::discover_full_execute(root)?,
        };
        let mut cases = Vec::new();
        for path in files {
            anyhow::ensure!(path.is_file(), "gcc-torture case not found: {}", path.display());
            cases.push(Self::case_from_path(path)?);
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let tmp = tempfile::tempdir()?;
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        let mut compile = Command::new(rcc_path);
        compile.arg("-w");
        if self.mode == GccTortureMode::FullExecute {
            compile
                .arg("-fgnu-binary-literals")
                .arg("-fgnu-statement-expressions")
                .arg("-fgnu-omitted-conditional-operand")
                .arg("-fgnu-conditional-void-operand")
                .arg("-fgnu-permissive-redefinition")
                .arg("-fgnu-named-variadic")
                .arg("-fgnu-permissive-paste")
                .arg("-fgnu-va-args-elision")
                .arg("-fgnu-range-designators")
                .arg("-fgnu-attributes")
                .arg("-fgnu-inline-asm")
                .arg("-fgnu-case-ranges")
                .arg("-fgnu-labels-as-values")
                .arg("-fgnu-lvalue-comma")
                .arg("-fgnu-alignof")
                .arg("-fgnu-function-names")
                .arg("-fgnu89-inline")
                .arg("-fgnu-builtin-libcalls");
            if gcc_torture_case_requests_flag(&case.path, "-finstrument-functions") {
                compile.arg("-finstrument-functions");
            }
        }
        compile.arg("-o").arg(&exe_path).arg(&case.path);
        match run_with_timeout(&mut compile, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Ok(Outcome::Fail {
                    reason: format!(
                        "rcc compile/link failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") });
            }
        }

        let mut exec = Command::new(&exe_path);
        match run_with_timeout(&mut exec, TIMEOUT) {
            Ok(o) if o.status.success() => Ok(Outcome::Pass),
            Ok(o) => Ok(Outcome::Fail {
                reason: format!(
                    "non-zero exit code: {}",
                    o.status.code().map_or_else(|| "killed by signal".into(), |c| c.to_string()),
                ),
            }),
            Err(e) => Ok(Outcome::Fail { reason: format!("execution failed: {e}") }),
        }
    }
}

fn gcc_torture_case_requests_flag(path: &Path, flag: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|src| {
        src.lines().filter(|line| line.contains("dg-options")).any(|line| line.contains(flag))
    })
}

/// `tcc-tests2` adapter (LGPL).
pub struct TccTests2Adapter;

impl TccTests2Adapter {
    fn tests2_dir(root: &Path) -> PathBuf {
        root.join("tests").join("tests2")
    }

    /// Pure comparison logic for tests2 `.expect` files.
    pub fn compare_outcome(actual_output: &[u8], expected_path: &Path) -> Outcome {
        Self::compare_outcome_inner(actual_output, expected_path, TccCompareMode::Exact)
    }

    /// Comparison with fixture-specific normalization for known tests2 data drift.
    pub fn compare_outcome_for_stem(
        stem: &str,
        actual_output: &[u8],
        expected_path: &Path,
    ) -> Outcome {
        let mode = match stem {
            // The source prints `"%d "` for every element in every row. The
            // vendored .expect file only kept the trailing space on the last
            // line, while GCC/TCC-compatible execution prints it on all rows.
            "38_multiple_array_index" => TccCompareMode::TrimTrailingSpacesPerLine,
            // The source prints `printf("%d", ...)` without a newline. GCC,
            // TCC, and rcc all produce `17`; the vendored .expect file has a
            // final CRLF.
            "71_macro_empty_arg" => TccCompareMode::AllowFinalNewlineDrift,
            _ => TccCompareMode::Exact,
        };
        Self::compare_outcome_inner(actual_output, expected_path, mode)
    }

    fn compare_outcome_inner(
        actual_output: &[u8],
        expected_path: &Path,
        mode: TccCompareMode,
    ) -> Outcome {
        let expected = match std::fs::read(expected_path) {
            Ok(e) => e,
            Err(e) => {
                return Outcome::Skip {
                    reason: format!("cannot read {}: {e}", expected_path.display()),
                };
            }
        };
        let actual = normalize_tcc_output(actual_output, mode);
        let expected = normalize_tcc_output(&expected, mode);
        if actual == expected {
            Outcome::Pass
        } else {
            Outcome::Fail { reason: "output mismatch".into() }
        }
    }

    fn expected_path(case: &TestCase) -> PathBuf {
        case.path.with_extension("expect")
    }

    fn stem(case: &TestCase) -> anyhow::Result<String> {
        case.path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 filename: {}", case.path.display()))
    }

    fn platform_skip_reason(stem: &str) -> Option<&'static str> {
        match stem {
            "34_array_assignment" => Some("array assignment is not C"),
            "73_arm64" if cfg!(target_arch = "x86_64") => Some("arm64-specific ABI fixture"),
            "98_al_ax_extend" | "99_fastcall" if !cfg!(target_arch = "x86") => {
                Some("i386-specific assembly/calling-convention fixture")
            }
            _ => None,
        }
    }

    fn unsupported_extension_reason(stem: &str) -> Option<&'static str> {
        match stem {
            "60_errors_and_warnings" | "96_nodata_wanted" => {
                Some("tcc -dt diagnostic/data-section test mode is not an rcc feature")
            }
            "76_dollars_in_identifiers" => {
                Some("TinyCC/GNU dollar identifiers are not supported by rcc")
            }
            _ => None,
        }
    }

    fn runtime_args(stem: &str, case: &TestCase) -> Vec<String> {
        match stem {
            "31_args" => {
                ["arg1", "arg2", "arg3", "arg4", "arg5"].into_iter().map(str::to_owned).collect()
            }
            "46_grep" => {
                let file = case
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("46_grep.c")
                    .to_owned();
                vec![r"[^* ]*[:a:d: ]+\:\*-/: $".to_owned(), file]
            }
            _ => Vec::new(),
        }
    }

    fn prepare_runtime_workdir(stem: &str, case: &TestCase, tmp: &Path) -> anyhow::Result<PathBuf> {
        let runtime_dir = tmp.join("run");
        std::fs::create_dir_all(&runtime_dir)?;

        if stem == "46_grep" {
            // The vendored file is CRLF-normalized in our checkout, while
            // the upstream expected output assumes LF input for its `$`
            // pattern. GCC and rcc agree on both inputs; run the fixture
            // under the upstream line-ending condition instead of treating
            // this as a compiler failure.
            let file_name = case
                .path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("missing filename: {}", case.path.display()))?;
            let input_path = runtime_dir.join(file_name);
            let input = std::fs::read(&case.path)?;
            let input = String::from_utf8_lossy(&input).replace("\r\n", "\n");
            std::fs::write(&input_path, input.as_bytes())?;
        }

        Ok(runtime_dir)
    }

    fn compile_flags() -> [&'static str; 21] {
        [
            "-w",
            "-fgnu-binary-literals",
            "-fgnu-statement-expressions",
            "-fgnu-omitted-conditional-operand",
            "-fgnu-conditional-void-operand",
            "-fgnu-permissive-redefinition",
            "-fgnu-named-variadic",
            "-fgnu-permissive-paste",
            "-fgnu-va-args-elision",
            "-fgnu-range-designators",
            "-fgnu-attributes",
            "-fgnu-inline-asm",
            "-fgnu-case-ranges",
            "-fgnu-labels-as-values",
            "-fgnu-lvalue-comma",
            "-fgnu-typeof",
            "-fgnu-alignof",
            "-fgnu-pragma-pack",
            "-fgnu-function-names",
            "-fgnu-builtin-libcalls",
            "-lm",
        ]
    }

    fn combined_output(output: &std::process::Output) -> Vec<u8> {
        let mut bytes = output.stdout.clone();
        bytes.extend_from_slice(&output.stderr);
        bytes
    }
}

impl Adapter for TccTests2Adapter {
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
        let dir = Self::tests2_dir(root);
        anyhow::ensure!(dir.is_dir(), "tcc tests2 directory not found: {}", dir.display());

        let mut cases = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("c") {
                continue;
            }
            let expected = path.with_extension("expect");
            if !expected.exists() {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("non-UTF-8 filename: {}", path.display()))?;
            cases.push(TestCase { id: format!("tcc-tests2::{stem}"), path });
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let stem = Self::stem(case)?;
        if let Some(reason) = Self::platform_skip_reason(&stem) {
            return Ok(Outcome::Skip { reason: reason.to_owned() });
        }
        if let Some(reason) = Self::unsupported_extension_reason(&stem) {
            return Ok(Outcome::Fail { reason: reason.to_owned() });
        }

        let expected_path = Self::expected_path(case);
        let tmp = tempfile::tempdir()?;
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        let mut compile = Command::new(rcc_path);
        compile.args(Self::compile_flags());
        if stem == "95_bitfields_ms" {
            compile.arg("-fms-bitfields");
        }
        compile.arg("-o").arg(&exe_path).arg(&case.path);
        match run_with_timeout(&mut compile, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let actual = Self::combined_output(&o);
                let compared = Self::compare_outcome_for_stem(&stem, &actual, &expected_path);
                return Ok(match compared {
                    Outcome::Pass => Outcome::Pass,
                    _ => Outcome::Fail {
                        reason: format!(
                            "rcc compile/link failed (exit {}): {}",
                            o.status.code().unwrap_or(-1),
                            String::from_utf8_lossy(&actual).chars().take(256).collect::<String>(),
                        ),
                    },
                });
            }
            Err(e) => {
                return Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") });
            }
        }

        let mut exec = Command::new(&exe_path);
        let runtime_dir = Self::prepare_runtime_workdir(&stem, case, tmp.path())?;
        exec.current_dir(&runtime_dir);
        exec.args(Self::runtime_args(&stem, case));
        match run_with_timeout(&mut exec, TIMEOUT) {
            Ok(o) if o.status.success() => {
                let actual = Self::combined_output(&o);
                Ok(Self::compare_outcome_for_stem(&stem, &actual, &expected_path))
            }
            Ok(o) => Ok(Outcome::Fail {
                reason: format!(
                    "non-zero exit code: {}",
                    o.status.code().map_or_else(|| "killed by signal".into(), |c| c.to_string()),
                ),
            }),
            Err(e) => Ok(Outcome::Fail { reason: format!("execution failed: {e}") }),
        }
    }
}

#[cfg(test)]
mod tcc_tests2_adapter_unit_tests {
    use super::*;

    #[test]
    fn runtime_workdir_is_temp_for_side_effecting_tests() {
        let tmp = tempfile::tempdir().unwrap();
        let suite_dir = tmp.path().join("suite/tests/tests2");
        std::fs::create_dir_all(&suite_dir).unwrap();
        let case =
            TestCase { id: "tcc-tests2::40_stdio".into(), path: suite_dir.join("40_stdio.c") };

        let runtime_dir =
            TccTests2Adapter::prepare_runtime_workdir("40_stdio", &case, tmp.path()).unwrap();
        std::fs::write(runtime_dir.join("fred.txt"), b"hello\nhello\n").unwrap();

        assert_ne!(runtime_dir, suite_dir);
        assert!(runtime_dir.starts_with(tmp.path()));
        assert!(!suite_dir.join("fred.txt").exists());
    }

    #[test]
    fn grep_runtime_workdir_gets_lf_normalized_input_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let suite_dir = tmp.path().join("suite/tests/tests2");
        std::fs::create_dir_all(&suite_dir).unwrap();
        let path = suite_dir.join("46_grep.c");
        std::fs::write(&path, b"a\r\nb\r\n").unwrap();
        let case = TestCase { id: "tcc-tests2::46_grep".into(), path };

        let runtime_dir =
            TccTests2Adapter::prepare_runtime_workdir("46_grep", &case, tmp.path()).unwrap();

        assert_eq!(std::fs::read(runtime_dir.join("46_grep.c")).unwrap(), b"a\nb\n");
    }
}

#[derive(Copy, Clone)]
enum TccCompareMode {
    Exact,
    TrimTrailingSpacesPerLine,
    AllowFinalNewlineDrift,
}

fn normalize_tcc_output(bytes: &[u8], mode: TccCompareMode) -> Vec<u8> {
    let normalized = normalize_stdout_newlines(bytes);
    match mode {
        TccCompareMode::Exact => normalized,
        TccCompareMode::AllowFinalNewlineDrift => strip_one_final_newline(normalized),
        TccCompareMode::TrimTrailingSpacesPerLine => {
            let mut out = Vec::with_capacity(normalized.len());
            for line in normalized.split_inclusive(|b| *b == b'\n') {
                let (body, newline) = if let Some(body) = line.strip_suffix(b"\n") {
                    (body, true)
                } else {
                    (line, false)
                };
                let trimmed_len = body.iter().rposition(|b| *b != b' ').map_or(0, |idx| idx + 1);
                out.extend_from_slice(&body[..trimmed_len]);
                if newline {
                    out.push(b'\n');
                }
            }
            out
        }
    }
}

fn strip_one_final_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    bytes
}

/// `llvm-test-suite` SingleSource adapter.
pub struct LlvmTestSuiteAdapter;

const LLVM_SINGLE_SOURCE_SUBSET: &[&str] = &[
    "2002-04-17-PrintfChar",
    "2002-05-02-ArgumentTest",
    "2002-05-03-NotTest",
    "2003-04-22-Switch",
    "2003-07-08-BitOpsTest",
    "2003-10-13-SwitchTest",
];

impl LlvmTestSuiteAdapter {
    /// Pure comparison logic for LLVM `.reference_output` files.
    ///
    /// LLVM SingleSource references include a final `exit N` line after stdout.
    pub fn compare_outcome(stdout: &[u8], exit_code: Option<i32>, expected_path: &Path) -> Outcome {
        let expected = match std::fs::read(expected_path) {
            Ok(e) => normalize_stdout_newlines(&e),
            Err(e) => {
                return Outcome::Skip {
                    reason: format!("cannot read {}: {e}", expected_path.display()),
                };
            }
        };
        let mut actual = normalize_stdout_newlines(stdout);
        actual.extend_from_slice(format!("exit {}\n", exit_code.unwrap_or(-1)).as_bytes());
        if actual == expected {
            Outcome::Pass
        } else {
            Outcome::Fail { reason: "stdout/exit mismatch".into() }
        }
    }

    fn unit_tests_dir(root: &Path) -> PathBuf {
        root.join("SingleSource").join("UnitTests")
    }

    fn reference_path(case: &TestCase) -> PathBuf {
        case.path.with_extension("reference_output")
    }
}

impl Adapter for LlvmTestSuiteAdapter {
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
        let dir = Self::unit_tests_dir(root);
        anyhow::ensure!(dir.is_dir(), "llvm-test-suite UnitTests dir not found: {}", dir.display());
        let mut cases = Vec::new();
        for stem in LLVM_SINGLE_SOURCE_SUBSET {
            let path = dir.join(format!("{stem}.c"));
            let reference = path.with_extension("reference_output");
            if path.is_file() && reference.is_file() {
                cases.push(TestCase { id: format!("llvm-test-suite::{stem}"), path });
            }
        }
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }

    fn run(&self, rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
        let expected_path = Self::reference_path(case);
        let tmp = tempfile::tempdir()?;
        let exe_path =
            if cfg!(windows) { tmp.path().join("test.exe") } else { tmp.path().join("test") };

        let mut compile = Command::new(rcc_path);
        compile.arg("-w").arg("-lm").arg("-o").arg(&exe_path).arg(&case.path);
        match run_with_timeout(&mut compile, TIMEOUT) {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let mut actual = o.stdout;
                actual.extend_from_slice(&o.stderr);
                return Ok(Outcome::Fail {
                    reason: format!(
                        "rcc compile/link failed (exit {}): {}",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&actual).chars().take(256).collect::<String>(),
                    ),
                });
            }
            Err(e) => return Ok(Outcome::Fail { reason: format!("rcc invocation failed: {e}") }),
        }

        let mut exec = Command::new(&exe_path);
        match run_with_timeout(&mut exec, TIMEOUT) {
            Ok(o) => Ok(Self::compare_outcome(&o.stdout, o.status.code(), &expected_path)),
            Err(e) => Ok(Outcome::Fail { reason: format!("execution failed: {e}") }),
        }
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
