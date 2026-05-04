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

    struct TempSourceDir {
        path: PathBuf,
    }

    impl TempSourceDir {
        fn new(name: &str) -> Self {
            let safe_name = name.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_");
            let path = std::env::temp_dir()
                .join(format!("rcc-driver-e2e-src-{}-{safe_name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path)
                .unwrap_or_else(|err| panic!("create {}: {err}", path.display()));
            Self { path }
        }
    }

    impl Drop for TempSourceDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
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
        compile_fixture_with_options(fixture, exe, Options::default())
    }

    fn compile_fixture_with_options(
        fixture: &Fixture,
        exe: &Path,
        options: Options,
    ) -> Result<(), String> {
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap));
        let mut session =
            Session::with_handler(Options { output: Some(exe.to_path_buf()), ..options }, handler);
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

    fn assert_source(name: &str, source: &str, stdout: &[u8], status: i32) {
        assert_source_with_options(name, source, stdout, status, Options::default());
    }

    fn assert_source_with_options(
        name: &str,
        source: &str,
        stdout: &[u8],
        status: i32,
        options: Options,
    ) {
        let dir = TempSourceDir::new(name);
        let c_path = dir.path.join(format!("{name}.c"));
        fs::write(&c_path, source)
            .unwrap_or_else(|err| panic!("write {}: {err}", c_path.display()));
        let fixture = Fixture { name: name.to_owned(), c_path, stdout: stdout.to_vec(), status };
        let exe = TempExe::new(name);
        compile_fixture_with_options(&fixture, &exe.path, options)
            .unwrap_or_else(|err| panic!("{}: compile/link failed:\n{err}", fixture.name));
        let run = run_with_timeout(&exe.path, TIMEOUT).unwrap_or_else(|err| {
            panic!("{}: failed to run {}: {err}", fixture.name, exe.path.display())
        });
        assert!(!run.timed_out, "{}: timed out after {:?}", fixture.name, TIMEOUT);
        assert_eq!(run.output.stdout, fixture.stdout, "{}: stdout mismatch", fixture.name);
        assert_eq!(
            run.output.status.code(),
            Some(fixture.status),
            "{}: exit status mismatch",
            fixture.name
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
        let std = if fixture.name.starts_with("gnu_") { "-std=gnu99" } else { "-std=c99" };
        Command::new("cc")
            .arg(std)
            .arg(&fixture.c_path)
            .arg("-o")
            .arg(exe)
            .output()
            .unwrap_or_else(|err| panic!("{}: failed to run host cc: {err}", fixture.name))
    }

    fn host_cc_differential_supported(fixture: &Fixture) -> bool {
        // This fixture remains covered by `e2e_fixtures`. Some host compilers
        // reject its GNU lvalue-comma expression even in GNU dialect modes, so
        // it cannot act as a portable differential oracle.
        fixture.name != "gnu_control_flow"
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
    fn gnu_va_area_fmt_reaches_libc_vsprintf() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping __va_area__ e2e: LLVM backend feature is disabled");
            return;
        }

        let dir = std::env::temp_dir().join(format!("rcc-driver-va-area-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("create {}: {err}", dir.display()));
        let c_path = dir.join("va_area_fmt.c");
        fs::write(
            &c_path,
            r#"
typedef struct {
  unsigned gp_offset;
  unsigned fp_offset;
  void *overflow_arg_area;
  void *reg_save_area;
} __va_elem;
typedef __va_elem va_list[1];

int vsprintf(char *str, const char *fmt, va_list ap);
int strcmp(const char *a, const char *b);
int puts(const char *s);

char *fmtbuf(char *buf, char *fmt, ...) {
  va_list ap;
  *ap = *(__va_elem *)__va_area__;
  vsprintf(buf, fmt, ap);
  return buf;
}

int main(void) {
  char buf[64];
  fmtbuf(buf, "%d %s", 7, "ok");
  puts(buf);
  return strcmp(buf, "7 ok");
}
"#,
        )
        .unwrap_or_else(|err| panic!("write {}: {err}", c_path.display()));

        let fixture = Fixture {
            name: "gnu_va_area_fmt".into(),
            c_path,
            stdout: b"7 ok\n".to_vec(),
            status: 0,
        };
        assert_fixture(&fixture);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gnu_field_alignment_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU field alignment e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_field_alignment",
            r#"
struct s1 { int __attribute__ ((aligned (8))) a; };
struct { char c; struct s1 m; } v;
int main(void) {
  return ((unsigned long)&v.m & 7) ? 1 : 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_scalar_storage_order_bitfield_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU scalar_storage_order e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_scalar_storage_order_bitfield",
            r#"
#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
#define REVERSE_SSO __attribute__((scalar_storage_order("big-endian")))
#else
#define REVERSE_SSO __attribute__((scalar_storage_order("little-endian")))
#endif
struct S {
  short int i : 12;
  char c1 : 1;
  char c2 : 1;
  char c3 : 1;
  char c4 : 1;
} REVERSE_SSO;
int main(void) {
  struct S s = { 341, 1, 1, 1, 1 };
  unsigned char *p = (unsigned char *)&s;
  return sizeof(s) == 2 && p[0] == 21 ? 0 : 1;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_initializer_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector initializer e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_initializer",
            r#"
typedef int v4si __attribute__((vector_size(16)));
int main(void) {
  v4si x = { 1, 2 };
  int *p = (int *)&x;
  if (p[0] != 1) return 1;
  if (p[1] != 2) return 2;
  if (p[2] != 0) return 3;
  if (p[3] != 0) return 4;
  x = (v4si){ 4, 3, 2, 1 };
  if (p[0] != 4) return 5;
  if (p[1] != 3) return 6;
  if (p[2] != 2) return 7;
  if (p[3] != 1) return 8;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_float_zero_fill_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector float zero-fill e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_float_zero_fill",
            r#"
typedef float v4sf __attribute__((vector_size(16)));
int main(void) {
  v4sf x = (v4sf){ 18.0, 20.0, 22 };
  float *p = (float *)&x;
  if (p[0] != 18.0) return 1;
  if (p[1] != 20.0) return 2;
  if (p[2] != 22.0) return 3;
  if (p[3] != 0.0) return 4;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_memcmp_byte_view_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector byte-view e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_memcmp_byte_view",
            r#"
typedef int v4si __attribute__((vector_size(16)));
int memcmp(const void *, const void *, unsigned long);
int main(void) {
  v4si x = { 1, 2, 3, 4 };
  int expect[4] = { 1, 2, 3, 4 };
  return memcmp(&x, expect, sizeof(x));
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_pointer_store_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector pointer-store e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_pointer_store",
            r#"
typedef unsigned char v16qi __attribute__((vector_size(16)));
int main(void) {
  unsigned char b[16] = { 0 };
  v16qi c = { 1, 2, 3, 4, 5, 6, 7, 8,
              9, 10, 11, 12, 13, 14, 15, 16 };
  *(v16qi *)&b[0] = c;
  for (int i = 0; i < 16; i = i + 1)
    if (b[i] != i + 1)
      return i + 1;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_scalar_bitcast_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector scalar-cast e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_scalar_bitcast",
            r#"
typedef int v2si __attribute__((vector_size(8)));
int main(void) {
  long long bits = 0x0000000200000001LL;
  v2si v = (v2si)bits;
  long long roundtrip = (long long)v;
  return roundtrip == bits ? 0 : 1;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_vector_bitcast_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector vector-cast e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_vector_bitcast",
            r#"
typedef int v2si __attribute__((vector_size(8)));
typedef float v2sf __attribute__((vector_size(8)));
int main(void) {
  v2sf f = { 2.0, 6.0 };
  v2si i = (v2si)f;
  v2sf g = (v2sf)i;
  float *p = (float *)&g;
  if (p[0] != 2.0) return 1;
  if (p[1] != 6.0) return 2;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_invalid_cast_diagnostic_probe() {
        let dir = TempSourceDir::new("gnu_vector_invalid_cast");
        let c_path = dir.path.join("gnu_vector_invalid_cast.c");
        fs::write(
            &c_path,
            r#"
typedef int v4si __attribute__((vector_size(16)));
int main(void) {
  long long bits = 0;
  v4si v = (v4si)bits;
  return 0;
}
"#,
        )
        .unwrap_or_else(|err| panic!("write {}: {err}", c_path.display()));
        let fixture = Fixture {
            name: "gnu_vector_invalid_cast".to_owned(),
            c_path,
            stdout: vec![],
            status: 0,
        };
        let exe = TempExe::new("gnu_vector_invalid_cast");
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap.clone()));
        let mut session = Session::with_handler(
            Options { output: Some(exe.path.clone()), gnu_attributes: true, ..Options::default() },
            handler,
        );
        let _ = pipeline::compile(&mut session, &fixture.c_path);

        assert!(session.handler.has_errors(), "invalid vector cast must fail before codegen");
        assert!(
            cap.diagnostics().iter().any(|diag| diag.message.contains("invalid GNU vector cast")),
            "diagnostics: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn gnu_vector_integer_arithmetic_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!(
                "skipping GNU vector integer arithmetic e2e: LLVM backend feature is disabled"
            );
            return;
        }

        assert_source_with_options(
            "gnu_vector_integer_arithmetic",
            r#"
typedef short v8hi __attribute__((vector_size(16)));
short two(void) { return 2; }
int main(void) {
  v8hi a = { 1, 2, 3, 4, 5, 6, 7, 8 };
  v8hi b = two() + a;
  short *p = (short *)&b;
  if (p[0] != 3) return 1;
  if (p[7] != 10) return 2;
  b = a * 2;
  if (p[0] != 2) return 3;
  if (p[7] != 16) return 4;
  b = (a << 1) ^ 3;
  if (p[0] != 1) return 5;
  if (p[1] != 7) return 6;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_float_arithmetic_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector float arithmetic e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_float_arithmetic",
            r#"
typedef float v4sf __attribute__((vector_size(16)));
int main(void) {
  v4sf a = { 1.0, 2.0, 3.0, 4.0 };
  v4sf b = 2 + a;
  float *p = (float *)&b;
  if (p[0] != 3.0) return 1;
  if (p[3] != 6.0) return 2;
  b = a * 2.0;
  if (p[0] != 2.0) return 3;
  if (p[3] != 8.0) return 4;
  b = 12.0 / a;
  if (p[0] != 12.0) return 5;
  if (p[3] != 3.0) return 6;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_argument_abi_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector argument ABI e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_argument_abi",
            r#"
typedef int v2si __attribute__((vector_size(8)));
typedef float v4sf __attribute__((vector_size(16)));
int take_i(v2si x) {
  int *p = (int *)&x;
  return p[0] == 1 && p[1] == 2;
}
int take_f(v4sf x) {
  float *p = (float *)&x;
  return p[0] == 3.0 && p[1] == 4.0 && p[2] == 5.0 && p[3] == 6.0;
}
int main(void) {
  v2si i = { 1, 2 };
  v4sf f = { 3.0, 4.0, 5.0, 6.0 };
  return take_i(i) && take_f(f) ? 0 : 1;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_return_abi_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector return ABI e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_return_abi",
            r#"
typedef int v4si __attribute__((vector_size(16)));
v4si make(int base) {
  return (v4si){ base, base + 1, base + 2, base + 3 };
}
int main(void) {
  v4si x = make(7);
  int *p = (int *)&x;
  if (p[0] != 7) return 1;
  if (p[1] != 8) return 2;
  if (p[2] != 9) return 3;
  if (p[3] != 10) return 4;
  return 0;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_vector_20050316_reduced_call_return_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU vector 20050316 ABI e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_vector_20050316_reduced",
            r#"
typedef int v2si __attribute__((vector_size(8)));
typedef unsigned int v2usi __attribute__((vector_size(8)));
v2si id(v2usi x) {
  return (v2si)x;
}
long long pack(v2si x) {
  return (long long)x;
}
int main(void) {
  v2usi x = { 6, 6 };
  v2si y = id(x);
  int *p = (int *)&y;
  if (p[0] != 6) return 1;
  if (p[1] != 6) return 2;
  return pack(y) == 0x0000000600000006LL ? 0 : 3;
}
"#,
            b"",
            0,
            Options { gnu_attributes: true, ..Options::default() },
        );
    }

    #[test]
    fn chibicc_function_abi_runtime_smoke() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping function ABI smoke: LLVM backend feature is disabled");
            return;
        }

        let cases: &[(&str, &str)] = &[
            (
                "abi_narrow_returns",
                r#"
char ret_char(int x) { return x; }
short ret_short(int x) { return x; }
_Bool bool_add(_Bool x) { return x + 1; }
_Bool bool_sub(_Bool x) { return x - 1; }
int main(void) {
  return !(ret_char(261) == 5
           && ret_short(65531) == -5
           && bool_add(3) == 1
           && bool_sub(3) == 0);
}
"#,
            ),
            (
                "abi_fixed_and_variadic_int_calls",
                r#"
int sprintf(char *, const char *, ...);
int strcmp(const char *, const char *);
int add6(int a, int b, int c, int d, int e, int f) { return a + b + c + d + e + f; }
int add10(int a, int b, int c, int d, int e, int f, int g, int h, int i, int j) {
  return a + b + c + d + e + f + g + h + i + j;
}
int main(void) {
  char buf[32];
  sprintf(buf, "%d %d", 1, 2);
  return !(add6(1, 2, 3, 4, 5, 6) == 21
           && add10(1, 2, 3, 4, 5, 6, 7, 8, 9, 10) == 55
           && strcmp(buf, "1 2") == 0);
}
"#,
            ),
            (
                "abi_builtin_va_arg_ints",
                r#"
typedef __builtin_va_list va_list;
int add_all(int n, ...) {
  va_list ap;
  __builtin_va_start(ap, n);
  int sum = 0;
  for (int i = 0; i < n; i = i + 1)
    sum = sum + __builtin_va_arg(ap, int);
  __builtin_va_end(ap);
  return sum;
}
int main(void) {
  return !(add_all(3, 1, 2, 3) == 6 && add_all(4, 1, 2, 3, -1) == 5);
}
"#,
            ),
            (
                "abi_float_double_calls",
                r#"
int sprintf(char *, const char *, ...);
int strcmp(const char *, const char *);
float add_float3(float x, float y, float z) { return x + y + z; }
double add_double3(double x, double y, double z) { return x + y + z; }
double many_double(double a, double b, double c, double d, double e,
                   double f, double g, double h, double i, double j) {
  return i / j;
}
int main(void) {
  char buf[32];
  sprintf(buf, "%.1f", (float)3.5);
  return !((int)add_float3(2.5, 2.5, 2.5) == 7
           && (int)add_double3(2.5, 2.5, 2.5) == 7
           && (int)many_double(1, 2, 3, 4, 5, 6, 7, 8, 40, 10) == 4
           && strcmp(buf, "3.5") == 0);
}
"#,
            ),
            (
                "abi_mixed_register_stack_args",
                r#"
int many_args3(int a, double b, int c, int d, double e, int f,
               double g, int h, double i, double j, double k,
               double l, double m, int n, int o, double p) {
  return o / p;
}
int main(void) {
  return many_args3(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 80, 10) == 8
             ? 0
             : 1;
}
"#,
            ),
            (
                "abi_struct_args",
                r#"
typedef struct { int a, b; short c; char d; } Ty4;
typedef struct { int a; float b; double c; } Ty5;
typedef struct { unsigned char a[3]; } Ty6;
typedef struct { long a, b, c; } Ty7;
int st4(Ty4 x, int n) { if (n == 0) return x.a; if (n == 1) return x.b; if (n == 2) return x.c; return x.d; }
int st5(Ty5 x, int n) { if (n == 0) return x.a; if (n == 1) return x.b; return x.c; }
int st6(Ty6 x, int n) { return x.a[n]; }
int st7(Ty7 x, int n) { if (n == 0) return x.a; if (n == 1) return x.b; return x.c; }
int main(void) {
  Ty4 a = {10, 20, 30, 40};
  Ty5 b = {10, 20, 30};
  Ty6 c = {10, 20, 30};
  Ty7 d = {10, 20, 30};
  return !(st4(a, 3) == 40 && st5(b, 2) == 30 && st6(c, 1) == 20 && st7(d, 2) == 30);
}
"#,
            ),
            (
                "abi_struct_returns",
                r#"
typedef struct { int a, b; short c; char d; } Ty4;
typedef struct { int a; float b; double c; } Ty5;
typedef struct { unsigned char a[3]; } Ty6;
typedef struct { unsigned char a[10]; } Ty20;
typedef struct { unsigned char a[20]; } Ty21;
Ty4 ret4(void) { return (Ty4){10, 20, 30, 40}; }
Ty5 ret5(void) { return (Ty5){10, 20, 30}; }
Ty6 ret6(void) { return (Ty6){10, 20, 30}; }
Ty20 ret20(void) { return (Ty20){10, 20, 30, 40, 50, 60, 70, 80, 90, 100}; }
Ty21 ret21(void) { return (Ty21){1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
                                  11, 12, 13, 14, 15, 16, 17, 18, 19, 20}; }
int main(void) {
  return !(ret4().d == 40
           && (int)ret5().c == 30
           && ret6().a[2] == 30
           && ret20().a[9] == 100
           && ret21().a[19] == 20);
}
"#,
            ),
            (
                "abi_long_double",
                r#"
int sprintf(char *, const char *, ...);
int strncmp(const char *, const char *, unsigned long);
double to_double(long double x) { return x; }
long double to_ldouble(int x) { return x; }
int main(void) {
  char buf[64];
  sprintf(buf, "%Lf", (long double)12.3);
  return !(to_double(3.5) == 3.5
           && (long double)5.0 == (long double)5.0
           && to_ldouble(5.0) == 5.0
           && strncmp(buf, "12.3", 4) == 0);
}
"#,
            ),
        ];

        for (name, source) in cases {
            assert_source(name, source, b"", 0);
        }
    }

    #[test]
    fn gnu_builtin_llabs_ignores_abort_definition_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU llabs builtin e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_builtin_llabs",
            r#"
long long a = -1;
long long llabs(long long);
void abort(void);
int main(void) {
  if (llabs(a) != 1)
    abort();
  return 0;
}
long long llabs(long long b) {
  abort();
}
"#,
            b"",
            0,
            Options { gnu_builtin_libcalls: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_inline_asm_empty_matching_output_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU inline asm e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_inline_asm_matching_output",
            r#"
int val = -2147483647 - 1;
int main(void) {
  volatile int i = 0;
  asm ("" : "=r" (i) : "0" ((long long)val));
  return i != val;
}
"#,
            b"",
            0,
            Options { gnu_inline_asm: true, ..Options::default() },
        );
    }

    #[test]
    fn gnu_inline_asm_readwrite_operand_evaluates_once_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU inline asm e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu_inline_asm_readwrite_once",
            r#"
int count = 0;
int dummy;
int *bar(void) {
  ++count;
  return &dummy;
}
void foo(void) {
  asm ("" : "+r" (*bar()));
}
int main(void) {
  foo();
  return count != 1;
}
"#,
            b"",
            0,
            Options { gnu_inline_asm: true, ..Options::default() },
        );
    }

    #[test]
    fn instrument_functions_honors_no_instrument_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping -finstrument-functions e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "instrument_functions_no_instrument",
            r#"
#define NOCHK __attribute__((no_instrument_function))
int entry_calls, exit_calls;
void (*last_fn_entered)(void);
void (*last_fn_exited)(void);

void __cyg_profile_func_enter(void *, void *) NOCHK;
void __cyg_profile_func_exit(void *, void *) NOCHK;
int main(void) NOCHK;
void nfoo(void) NOCHK;

void foo(void) {
  if (last_fn_entered != foo)
    __builtin_abort();
}

void nfoo(void) {
  if (entry_calls != 1 || exit_calls != 1)
    __builtin_abort();
  foo();
}

int main(void) {
  if (entry_calls != 0 || exit_calls != 0)
    __builtin_abort();
  foo();
  if (entry_calls != 1 || exit_calls != 1 || last_fn_exited != foo)
    __builtin_abort();
  nfoo();
  return !(entry_calls == 2 && exit_calls == 2 && last_fn_entered == foo);
}

void __cyg_profile_func_enter(void *fn, void *parent) {
  (void)parent;
  entry_calls++;
  last_fn_entered = (void (*)(void))fn;
}
void __cyg_profile_func_exit(void *fn, void *parent) {
  (void)parent;
  exit_calls++;
  last_fn_exited = (void (*)(void))fn;
}
"#,
            b"",
            0,
            Options {
                gnu_attributes: true,
                instrument_functions: true,
                gnu_builtin_libcalls: true,
                ..Options::default()
            },
        );
    }

    #[test]
    fn gnu89_kr_parameter_declaration_runtime_probe() {
        if !llvm_backend_enabled_for_this_build() {
            eprintln!("skipping GNU89 K&R e2e: LLVM backend feature is disabled");
            return;
        }

        assert_source_with_options(
            "gnu89_kr_param_decl",
            r#"
unsigned int a[0x1000];
extern const unsigned long v;

main()
{
  f(v);
  f(v);
  exit(0);
}

f(a)
     unsigned long a;
{
  if (a != 0xdeadbeefL)
    abort();
}

const unsigned long v = 0xdeadbeefL;
"#,
            b"",
            0,
            Options { gnu_builtin_libcalls: true, ..Options::default() },
        );
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
            if !host_cc_differential_supported(fixture) {
                report.push(format!("{}\tskipped\tskipped\t0\t0", fixture.name));
                continue;
            }

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
