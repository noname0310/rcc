//! CLI driver that runs configured conformance suites and writes
//! the machine-readable `Report` as pretty-printed JSON.

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::Parser;

use rcc_conformance::adapters::{
    CTestSuiteAdapter, ChibiccAdapter, CsmithDifferentialAdapter, GccTortureAdapter,
    LlvmTestSuiteAdapter, TccTests2Adapter,
};
use rcc_conformance::Suite;

/// Per-suite execution mode selected by `--mode`.
///
/// Most suites only have one meaningful pipeline stage to exercise
/// and simply ignore the mode. `chibicc` is multi-purpose: at M5 we
/// run the preprocessor-focused subset via `--emit=pp`, at M2/M6 we
/// can run an early stage-isolated subset, and at M6 we run the full
/// compile + link + execute pipeline. A CLI flag is cheaper than
/// several suite names.
#[derive(Copy, Clone, Debug, clap::ValueEnum)]
enum Mode {
    /// Full compile + link + run (default).
    Compile,
    /// Preprocessor-only (task 04-18): `rcc --emit=pp` + exit-code
    /// check against the chibicc preprocessor fixtures.
    Preprocess,
    /// Stage-isolated chibicc slice: `arith.c`, `control.c`, and
    /// `function.c` only, with a host-compiled minimal support helper
    /// instead of upstream `test/common`.
    #[value(name = "stage-1-3")]
    Stage1To3,
}

/// Run conformance suites against `rcc` and emit a JSON report.
#[derive(Parser)]
#[command(name = "rcc_conformance_run")]
struct Cli {
    /// Path to the `rcc` binary under test.
    #[arg(long)]
    rcc: PathBuf,

    /// Suite name to run (may be repeated).
    #[arg(long = "suite", required = true)]
    suites: Vec<String>,

    /// Output path for the JSON report (default: `docs/conformance.json`).
    #[arg(long, default_value = "docs/conformance.json")]
    output: PathBuf,

    /// Include GPL-licensed suites (e.g. gcc-torture).
    #[arg(long)]
    include_gpl: bool,

    /// Which pipeline stage each test should exercise. Defaults to
    /// the full compile + link + run path. `preprocess` is the
    /// preprocessor-only gate for task 04-18 / milestone M5.
    /// `stage-1-3` isolates chibicc's early arith/control/function
    /// fixtures without compiling upstream `test/common` with `rcc`.
    #[arg(long, value_enum, default_value_t = Mode::Compile)]
    mode: Mode,
}

/// Known suite names and whether they require `--include-gpl`.
const GPL_SUITES: &[&str] = &["gcc-torture"];

fn build_suite(name: &str, include_gpl: bool, mode: Mode) -> anyhow::Result<Suite> {
    if GPL_SUITES.contains(&name) && !include_gpl {
        bail!("suite `{name}` is GPL-licensed; pass --include-gpl to enable it");
    }

    let root = PathBuf::from("third_party/testsuites").join(name);

    let adapter: Box<dyn rcc_conformance::Adapter> = match (name, mode) {
        ("c-testsuite", _) => Box::new(CTestSuiteAdapter),
        ("chibicc", Mode::Compile) => Box::new(ChibiccAdapter::compile()),
        ("chibicc", Mode::Preprocess) => Box::new(ChibiccAdapter::preprocess()),
        ("chibicc", Mode::Stage1To3) => Box::new(ChibiccAdapter::stages1_to_3()),
        ("gcc-torture", _) => Box::new(GccTortureAdapter),
        ("tcc-tests2", _) => Box::new(TccTests2Adapter),
        ("llvm-test-suite", _) => Box::new(LlvmTestSuiteAdapter),
        ("csmith", _) => Box::new(CsmithDifferentialAdapter),
        _ => bail!("unknown suite: `{name}`"),
    };

    Ok(Suite { name: name.to_owned(), root, adapter })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut suites = Vec::new();
    for name in &cli.suites {
        suites.push(build_suite(name, cli.include_gpl, cli.mode)?);
    }

    let report = rcc_conformance::run_suites(&cli.rcc, &suites);

    let json = report.to_json_pretty();

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    std::fs::write(&cli.output, &json)
        .with_context(|| format!("writing report to {}", cli.output.display()))?;

    let mut total = 0;
    let mut pass = 0;
    let mut fail = 0;
    let mut xfail = 0;
    let mut skip = 0;

    for suite in &report.suites {
        let c = suite.counts();
        total += c.discovered();
        pass += c.pass;
        fail += c.fail;
        xfail += c.xfail;
        skip += c.skip;
        eprintln!(
            "suite {}: {} cases: {} pass, {} fail, {} xfail, {} skip; pass_rate={:.3}",
            suite.name,
            c.discovered(),
            c.pass,
            c.fail,
            c.xfail,
            c.skip,
            suite.pass_rate(),
        );
    }

    eprintln!(
        "wrote {} ({total} cases: {pass} pass, {fail} fail, {xfail} xfail, {skip} skip)",
        cli.output.display(),
    );

    Ok(())
}
