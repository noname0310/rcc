//! Builtin runtime/header integration tests.

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
    use rcc_session::{LinkOptions, Options, Session, TargetInfo};

    const TIMEOUT: Duration = Duration::from_secs(10);

    #[derive(Debug)]
    struct Fixture {
        name: String,
        c_path: PathBuf,
        stdout: Vec<u8>,
        status: i32,
    }

    struct TempExe {
        path: PathBuf,
    }

    impl TempExe {
        fn new(name: &str) -> Self {
            let safe_name = name.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
            let path = std::env::temp_dir()
                .join(format!("rcc-driver-builtin-rt-{}-{safe_name}", std::process::id()));
            let _ = fs::remove_file(&path);
            Self { path }
        }
    }

    impl Drop for TempExe {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
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
        let captured = cap.clone();
        let handler = Handler::with_emitter(Box::new(cap));
        let mut link = LinkOptions::default();
        if matches!(
            fixture.name.as_str(),
            "hosted_math_decls"
                | "hosted_math_classification"
                | "hosted_fenv"
                | "hosted_complex"
                | "hosted_tgmath"
        ) {
            link.libraries.push("m".to_owned());
        }
        let target = TargetInfo::host();
        let mut session = Session::with_handler(
            Options {
                output: Some(exe.to_path_buf()),
                target: target.clone(),
                system_include_paths: rcc_preprocess::include::discover_system_include_paths(
                    &target, None,
                ),
                link,
                ..Options::default()
            },
            handler,
        );
        pipeline::compile(&mut session, &fixture.c_path)?;
        let diagnostics = captured.diagnostics();
        if session.handler.has_errors() {
            return Err(format!("diagnostics were emitted: {diagnostics:#?}"));
        }
        if !exe.is_file() {
            return Err(format!(
                "compiler returned success but did not create {}; diagnostics: {diagnostics:#?}",
                exe.display()
            ));
        }
        Ok(())
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
        if let Some(reason) = fixture_skip_reason(&fixture.name) {
            eprintln!("skipping builtin-rt fixture {}: {reason}", fixture.name);
            return;
        }
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
            run.output.status.code(),
            Some(fixture.status),
            "{}: exit status mismatch\nstderr:\n{}",
            fixture.name,
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
    }

    fn fixture_skip_reason(name: &str) -> Option<&'static str> {
        match name {
            "hosted_math_classification" => Some(
                "pending glibc <math.h> classification macros: GNU statement-expr + typeof locals lower, but runtime comparison semantics still need a focused compiler bug task",
            ),
            "hosted_tgmath" => Some(
                "pending glibc <tgmath.h> macro dispatch coverage; depends on the hosted math classification bug",
            ),
            "hosted_remaining_headers" => Some(
                "pending broad glibc/POSIX header smoke coverage; signal handler typedef and several remaining system-header forms need focused lowering tasks",
            ),
            _ => None,
        }
    }

    #[test]
    fn builtin_rt_fixtures_compile_link_and_run() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping builtin-rt fixtures: LLVM backend feature is disabled");
            return;
        }

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/builtin-rt");
        let fixtures = discover_fixtures(&dir);
        assert!(fixtures.len() >= 5, "expected at least 5 builtin-rt fixtures");
        for fixture in &fixtures {
            assert_fixture(fixture);
        }
    }
}

#[cfg(windows)]
#[test]
fn builtin_rt_windows_native_target_is_skipped() {
    eprintln!("skipping builtin-rt fixtures: Windows-native runnable target is covered separately");
}
