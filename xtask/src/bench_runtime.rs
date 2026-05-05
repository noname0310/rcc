use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// One small C runtime benchmark.
#[derive(Clone, Debug)]
pub struct RuntimeProgram {
    /// Stable benchmark name.
    pub name: &'static str,
    /// Complete C99 source.
    pub source: &'static str,
    /// Expected stdout after normalizing CRLF to LF.
    pub expected_stdout: &'static str,
}

/// One compiler/runtime measurement row.
#[derive(Clone, Debug)]
pub struct RuntimeRow {
    /// Program name.
    pub program: String,
    /// Compiler label (`rcc` or `host-cc`).
    pub compiler: String,
    /// Wall time spent compiling.
    pub compile_time: Duration,
    /// Average wall time spent running the executable.
    pub runtime_avg: Duration,
    /// Number of run iterations.
    pub iterations: usize,
    /// Normalized program stdout.
    pub stdout: String,
}

/// Runtime benchmark command options.
#[derive(Clone, Debug)]
pub struct BenchRuntimeOptions {
    /// Path to the `rcc` executable under measurement.
    pub rcc: PathBuf,
    /// Host C compiler used as baseline, usually `cc` or `clang`.
    pub host_cc: PathBuf,
    /// Markdown report path.
    pub out: PathBuf,
    /// Number of executable runs per program/compiler pair.
    pub iterations: usize,
}

/// Built-in runtime programs. They deliberately avoid libc headers so `rcc`
/// does not need a system include setup; each program declares `printf`
/// directly and links against the host libc through the normal linker driver.
pub fn programs() -> Vec<RuntimeProgram> {
    vec![
        RuntimeProgram {
            name: "sum_loop",
            source: r#"
int printf(const char *, ...);
int main(void) {
    int acc = 0;
    for (int i = 0; i < 10000; i++) acc += i % 17;
    printf("%d\n", acc);
    return 0;
}
"#,
            expected_stdout: "79974\n",
        },
        RuntimeProgram {
            name: "fib_iter",
            source: r#"
int printf(const char *, ...);
int main(void) {
    int a = 0;
    int b = 1;
    for (int i = 0; i < 24; i++) {
        int next = a + b;
        a = b;
        b = next;
    }
    printf("%d\n", a);
    return 0;
}
"#,
            expected_stdout: "46368\n",
        },
        RuntimeProgram {
            name: "prime_count",
            source: r#"
int printf(const char *, ...);
int is_prime(int n) {
    if (n < 2) return 0;
    for (int d = 2; d * d <= n; d++) {
        if (n % d == 0) return 0;
    }
    return 1;
}
int main(void) {
    int count = 0;
    for (int n = 2; n < 500; n++) count += is_prime(n);
    printf("%d\n", count);
    return 0;
}
"#,
            expected_stdout: "95\n",
        },
        RuntimeProgram {
            name: "array_mix",
            source: r#"
int printf(const char *, ...);
int main(void) {
    int xs[8] = { 3, 1, 4, 1, 5, 9, 2, 6 };
    int acc = 0;
    for (int i = 0; i < 8; i++) acc = acc * 7 + xs[i];
    printf("%d\n", acc);
    return 0;
}
"#,
            expected_stdout: "2660083\n",
        },
        RuntimeProgram {
            name: "switch_table",
            source: r#"
int printf(const char *, ...);
int score(int x) {
    switch (x % 5) {
    case 0: return x + 3;
    case 1: return x * 2;
    case 2: return x - 7;
    case 3: return x / 2;
    default: return x ^ 3;
    }
}
int main(void) {
    int acc = 0;
    for (int i = 0; i < 200; i++) acc += score(i);
    printf("%d\n", acc);
    return 0;
}
"#,
            expected_stdout: "21660\n",
        },
    ]
}

/// Run the benchmark and write the Markdown report.
pub fn run(project_root: &Path, opts: &BenchRuntimeOptions) -> Result<()> {
    if opts.iterations == 0 {
        bail!("--iterations must be greater than zero");
    }
    let work = project_root.join("target").join("bench-runtime");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;

    let mut rows = Vec::new();
    for program in programs() {
        let src = work.join(format!("{}.c", program.name));
        fs::write(&src, program.source).with_context(|| format!("writing {}", src.display()))?;

        rows.push(run_one(&opts.rcc, "rcc", &src, &work, &program, opts.iterations)?);
        rows.push(run_one(&opts.host_cc, "host-cc", &src, &work, &program, opts.iterations)?);
    }

    let report = render_report(&rows, opts);
    if let Some(parent) = opts.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&opts.out, report).with_context(|| format!("writing {}", opts.out.display()))?;
    println!("wrote {}", opts.out.display());
    Ok(())
}

fn run_one(
    compiler: &Path,
    label: &str,
    src: &Path,
    work: &Path,
    program: &RuntimeProgram,
    iterations: usize,
) -> Result<RuntimeRow> {
    let exe = work.join(format!("{}-{}{}", program.name, label, exe_suffix()));
    let compile_start = Instant::now();
    let output = Command::new(compiler)
        .arg("-O2")
        .arg(src)
        .arg("-o")
        .arg(&exe)
        .output()
        .with_context(|| format!("running compiler `{}`", compiler.display()))?;
    let compile_time = compile_start.elapsed();
    if !output.status.success() {
        bail!(
            "{} failed compiling {} with status {}\nstdout:\n{}\nstderr:\n{}",
            label,
            src.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut stdout = String::new();
    let run_start = Instant::now();
    for _ in 0..iterations {
        let output =
            Command::new(&exe).output().with_context(|| format!("running {}", exe.display()))?;
        if !output.status.success() {
            bail!("{} exited with status {}", exe.display(), output.status);
        }
        let normalized = normalize_stdout(&String::from_utf8_lossy(&output.stdout));
        if normalized != program.expected_stdout {
            bail!(
                "{} produced unexpected stdout for {}\nexpected: {:?}\nactual: {:?}",
                label,
                program.name,
                program.expected_stdout,
                normalized
            );
        }
        stdout = normalized;
    }
    let runtime_avg = run_start.elapsed() / iterations as u32;

    Ok(RuntimeRow {
        program: program.name.to_owned(),
        compiler: label.to_owned(),
        compile_time,
        runtime_avg,
        iterations,
        stdout,
    })
}

/// Render a Markdown report.
pub fn render_report(rows: &[RuntimeRow], opts: &BenchRuntimeOptions) -> String {
    let mut out = String::new();
    out.push_str("# Runtime Performance Baseline\n\n");
    out.push_str(&format!("Date: {}\n\n", current_date_label()));
    out.push_str(&format!("Host: {}\n\n", current_host_label()));
    out.push_str("Command:\n\n");
    out.push_str("```text\n");
    out.push_str(&format!(
        "cargo xtask bench-runtime --rcc {} --host-cc {} --iterations {} --out {}\n",
        opts.rcc.display(),
        opts.host_cc.display(),
        opts.iterations,
        opts.out.display()
    ));
    out.push_str("```\n\n");
    out.push_str("Criterion compile-speed checks:\n\n");
    out.push_str("```text\n");
    out.push_str("cargo bench -p rcc_lexer --bench lex\n");
    out.push_str("cargo bench -p rcc_preprocess --bench preprocess\n");
    out.push_str("cargo bench -p rcc_parse --bench parse\n");
    out.push_str("cargo bench -p rcc_driver --bench pipeline\n");
    out.push_str("```\n\n");
    out.push_str(
        "Compile time and generated-code runtime are deliberately separate. \
         These numbers are a baseline, not a pass/fail threshold.\n\n",
    );
    out.push_str("| program | compiler | compile ms | runtime avg us | runs | stdout |\n");
    out.push_str("| --- | --- | ---: | ---: | ---: | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {:.3} | {:.3} | {} | `{}` |\n",
            row.program,
            row.compiler,
            ms(row.compile_time),
            us(row.runtime_avg),
            row.iterations,
            row.stdout.escape_debug()
        ));
    }
    out
}

fn normalize_stdout(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

fn current_date_label() -> String {
    std::env::var("RCC_BENCH_DATE").unwrap_or_else(|_| "generated locally".to_owned())
}

fn current_host_label() -> String {
    std::env::var("RCC_BENCH_HOST").unwrap_or_else(|_| "local developer machine".to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn sample_opts() -> BenchRuntimeOptions {
        BenchRuntimeOptions {
            rcc: PathBuf::from("target/release/rcc"),
            host_cc: PathBuf::from("cc"),
            out: PathBuf::from("docs/perf-baseline.md"),
            iterations: 3,
        }
    }

    #[test]
    fn built_in_program_set_has_at_least_five_entries() {
        let programs = programs();
        assert!(programs.len() >= 5);
        assert!(programs.iter().all(|p| p.source.contains("main")));
        assert!(programs.iter().all(|p| p.expected_stdout.ends_with('\n')));
    }

    #[test]
    fn report_separates_compile_time_from_runtime() {
        let rows = vec![
            RuntimeRow {
                program: "sum_loop".to_owned(),
                compiler: "rcc".to_owned(),
                compile_time: Duration::from_millis(12),
                runtime_avg: Duration::from_micros(34),
                iterations: 3,
                stdout: "79989\n".to_owned(),
            },
            RuntimeRow {
                program: "sum_loop".to_owned(),
                compiler: "host-cc".to_owned(),
                compile_time: Duration::from_millis(4),
                runtime_avg: Duration::from_micros(30),
                iterations: 3,
                stdout: "79989\n".to_owned(),
            },
        ];
        let report = render_report(&rows, &sample_opts());
        assert!(report.contains("| program | compiler | compile ms | runtime avg us |"));
        assert!(report.contains("baseline, not a pass/fail threshold"));
        assert!(report.contains("sum_loop"));
    }
}
