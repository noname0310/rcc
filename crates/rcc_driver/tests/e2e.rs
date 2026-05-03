//! End-to-end compile, link, and run tests.

#[cfg(not(windows))]
mod linux {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use rcc_driver::pipeline;
    use rcc_errors::{CaptureEmitter, Handler};
    use rcc_session::{Options, Session};

    const TIMEOUT: Duration = Duration::from_secs(10);

    struct TempExe {
        path: PathBuf,
    }

    impl TempExe {
        fn new(name: &str) -> Self {
            let safe_name = name.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
            let path = std::env::temp_dir()
                .join(format!("rcc-driver-e2e-{}-{safe_name}", std::process::id()));
            let _ = fs::remove_file(&path);
            Self { path }
        }
    }

    impl Drop for TempExe {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[derive(Debug)]
    struct Fixture {
        name: String,
        c_path: PathBuf,
        stdout: Vec<u8>,
        status: i32,
    }

    struct RunResult {
        output: Output,
        timed_out: bool,
    }

    fn llvm_backend_enabled_for_this_build() -> bool {
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap));
        let mut session = Session::with_handler(Options::default(), handler);
        let tcx = rcc_hir::TyCtxt::new();
        let hir = rcc_hir::HirCrate::default();
        let bodies = rcc_data_structures::FxHashMap::default();
        !matches!(
            rcc_codegen_llvm::codegen(&mut session, &tcx, &hir, &bodies),
            Err(rcc_codegen_llvm::CodegenError::BackendDisabled)
        )
    }

    fn discover_fixtures(dir: &Path) -> Vec<Fixture> {
        let mut fixtures = Vec::new();
        for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        {
            let path = entry.expect("read fixture entry").path();
            if path.extension() != Some(OsStr::new("c")) {
                continue;
            }
            let name = path.file_stem().and_then(OsStr::to_str).expect("utf-8 fixture").to_owned();
            let stdout = fs::read(path.with_extension("stdout"))
                .unwrap_or_else(|err| panic!("read expected stdout for {name}: {err}"));
            let status_text = fs::read_to_string(path.with_extension("status"))
                .unwrap_or_else(|err| panic!("read expected status for {name}: {err}"));
            let status = status_text
                .trim()
                .parse::<i32>()
                .unwrap_or_else(|err| panic!("parse expected status for {name}: {err}"));
            fixtures.push(Fixture { name, c_path: path, stdout, status });
        }
        fixtures.sort_by(|a, b| a.name.cmp(&b.name));
        fixtures
    }

    fn compile_fixture(fixture: &Fixture, exe: &Path) -> Result<(), String> {
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap));
        let mut session = Session::with_handler(
            Options { output: Some(exe.to_path_buf()), ..Options::default() },
            handler,
        );
        pipeline::compile(&mut session, &fixture.c_path)
    }

    fn run_with_timeout(exe: &Path, timeout: Duration) -> io::Result<RunResult> {
        let start = Instant::now();
        let mut child = Command::new(exe).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        loop {
            if child.try_wait()?.is_some() {
                return Ok(RunResult { output: child.wait_with_output()?, timed_out: false });
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                return Ok(RunResult { output: child.wait_with_output()?, timed_out: true });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_fixture(fixture: &Fixture) {
        let exe = TempExe::new(&fixture.name);
        compile_fixture(fixture, &exe.path)
            .unwrap_or_else(|err| panic!("{}: compile/link failed:\n{err}", fixture.name));

        let run = run_with_timeout(&exe.path, TIMEOUT).unwrap_or_else(|err| {
            panic!("{}: failed to run {}: {err}", fixture.name, exe.path.display())
        });
        assert!(
            !run.timed_out,
            "{}: timed out after {:?}\nstdout:\n{}\nstderr:\n{}",
            fixture.name,
            TIMEOUT,
            String::from_utf8_lossy(&run.output.stdout),
            String::from_utf8_lossy(&run.output.stderr)
        );

        assert_eq!(
            run.output.stdout,
            fixture.stdout,
            "{}: stdout mismatch\nexpected:\n{}\nactual:\n{}",
            fixture.name,
            String::from_utf8_lossy(&fixture.stdout),
            String::from_utf8_lossy(&run.output.stdout)
        );
        assert_eq!(
            run.output.status.code(),
            Some(fixture.status),
            "{}: exit status mismatch\nstderr:\n{}",
            fixture.name,
            String::from_utf8_lossy(&run.output.stderr)
        );
    }

    fn host_cc_available() -> bool {
        Command::new("cc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    fn compile_with_host_cc(fixture: &Fixture, exe: &Path) -> Output {
        Command::new("cc")
            .arg("-std=c99")
            .arg(&fixture.c_path)
            .arg("-o")
            .arg(exe)
            .output()
            .unwrap_or_else(|err| panic!("{}: failed to run host cc: {err}", fixture.name))
    }

    fn report_path() -> PathBuf {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
        target.join("rcc-driver-e2e").join("differential.tsv")
    }

    fn write_differential_report(lines: &[String]) {
        let path = report_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("create {}: {err}", parent.display()));
        }
        let mut text =
            String::from("fixture\trcc_status\tcc_status\trcc_stdout_len\tcc_stdout_len\n");
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        fs::write(&path, text).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
        eprintln!("wrote differential report: {}", path.display());
    }

    fn stdout_preview(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).escape_debug().to_string()
    }

    #[test]
    fn e2e_fixtures() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping e2e fixtures: LLVM backend feature is disabled");
            return;
        }

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/e2e");
        let fixtures = discover_fixtures(&dir);
        assert!(fixtures.len() >= 10, "expected at least 10 e2e fixtures");
        for fixture in &fixtures {
            assert_fixture(fixture);
        }
    }

    #[test]
    fn differential_vs_host_cc() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping differential e2e: LLVM backend feature is disabled");
            return;
        }
        if !host_cc_available() {
            eprintln!("skipping differential e2e: host cc is unavailable");
            return;
        }

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/e2e");
        let fixtures = discover_fixtures(&dir);
        assert!(fixtures.len() >= 5, "expected at least 5 e2e fixtures");

        let mut failures = Vec::new();
        let mut report = Vec::new();
        for fixture in &fixtures {
            let rcc_exe = TempExe::new(&format!("{}-rcc", fixture.name));
            let cc_exe = TempExe::new(&format!("{}-cc", fixture.name));
            compile_fixture(fixture, &rcc_exe.path)
                .unwrap_or_else(|err| panic!("{}: rcc compile/link failed:\n{err}", fixture.name));

            let cc_compile = compile_with_host_cc(fixture, &cc_exe.path);
            if !cc_compile.status.success() {
                panic!(
                    "{}: host cc failed with {}\nstdout:\n{}\nstderr:\n{}",
                    fixture.name,
                    cc_compile.status,
                    String::from_utf8_lossy(&cc_compile.stdout),
                    String::from_utf8_lossy(&cc_compile.stderr)
                );
            }

            let rcc = run_with_timeout(&rcc_exe.path, TIMEOUT)
                .unwrap_or_else(|err| panic!("{}: failed to run rcc binary: {err}", fixture.name));
            let cc = run_with_timeout(&cc_exe.path, TIMEOUT)
                .unwrap_or_else(|err| panic!("{}: failed to run cc binary: {err}", fixture.name));
            let rcc_status = rcc.output.status.code();
            let cc_status = cc.output.status.code();
            report.push(format!(
                "{}\t{:?}\t{:?}\t{}\t{}",
                fixture.name,
                rcc_status,
                cc_status,
                rcc.output.stdout.len(),
                cc.output.stdout.len()
            ));

            if rcc.timed_out
                || cc.timed_out
                || rcc_status != cc_status
                || rcc.output.stdout != cc.output.stdout
            {
                failures.push(format!(
                    "{name}: rcc vs cc mismatch\n  rcc timeout: {rcc_timeout}, status: {rcc_status:?}, stdout: {rcc_stdout:?}\n  cc  timeout: {cc_timeout}, status: {cc_status:?}, stdout: {cc_stdout:?}",
                    name = fixture.name,
                    rcc_timeout = rcc.timed_out,
                    cc_timeout = cc.timed_out,
                    rcc_stdout = stdout_preview(&rcc.output.stdout),
                    cc_stdout = stdout_preview(&cc.output.stdout),
                ));
            }
        }

        write_differential_report(&report);
        assert!(
            failures.is_empty(),
            "{} differential fixture(s) failed:\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

#[cfg(windows)]
#[test]
fn e2e_fixtures_require_target_wiring_on_windows() {
    eprintln!("skipping e2e fixtures: Windows-native runnable target is covered by 10-08");
}
