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

    fn run_with_timeout(exe: &Path, timeout: Duration) -> io::Result<Output> {
        let start = Instant::now();
        let mut child = Command::new(exe).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        loop {
            if child.try_wait()?.is_some() {
                return child.wait_with_output();
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                return child.wait_with_output();
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_fixture(fixture: &Fixture) {
        let exe = TempExe::new(&fixture.name);
        compile_fixture(fixture, &exe.path)
            .unwrap_or_else(|err| panic!("{}: compile/link failed:\n{err}", fixture.name));

        let started = Instant::now();
        let output = run_with_timeout(&exe.path, TIMEOUT).unwrap_or_else(|err| {
            panic!("{}: failed to run {}: {err}", fixture.name, exe.path.display())
        });
        let elapsed = started.elapsed();
        assert!(
            elapsed < TIMEOUT,
            "{}: timed out after {:?}\nstdout:\n{}\nstderr:\n{}",
            fixture.name,
            TIMEOUT,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert_eq!(
            output.stdout,
            fixture.stdout,
            "{}: stdout mismatch\nexpected:\n{}\nactual:\n{}",
            fixture.name,
            String::from_utf8_lossy(&fixture.stdout),
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            output.status.code(),
            Some(fixture.status),
            "{}: exit status mismatch\nstderr:\n{}",
            fixture.name,
            String::from_utf8_lossy(&output.stderr)
        );
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
}

#[cfg(windows)]
#[test]
fn e2e_fixtures_require_target_wiring_on_windows() {
    eprintln!("skipping e2e fixtures: Windows-native runnable target is covered by 10-08");
}
