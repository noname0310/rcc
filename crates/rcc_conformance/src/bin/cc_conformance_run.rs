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

/// Run conformance suites against `rcc` and emit a JSON report.
#[derive(Parser)]
#[command(name = "cc_conformance_run")]
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
}

/// Known suite names and whether they require `--include-gpl`.
const GPL_SUITES: &[&str] = &["gcc-torture"];

fn build_suite(name: &str, include_gpl: bool) -> anyhow::Result<Suite> {
    if GPL_SUITES.contains(&name) && !include_gpl {
        bail!("suite `{name}` is GPL-licensed; pass --include-gpl to enable it");
    }

    let root = PathBuf::from("third_party/testsuites").join(name);

    let adapter: Box<dyn rcc_conformance::Adapter> = match name {
        "c-testsuite" => Box::new(CTestSuiteAdapter),
        "chibicc" => Box::new(ChibiccAdapter),
        "gcc-torture" => Box::new(GccTortureAdapter),
        "tcc-tests2" => Box::new(TccTests2Adapter),
        "llvm-test-suite" => Box::new(LlvmTestSuiteAdapter),
        "csmith" => Box::new(CsmithDifferentialAdapter),
        _ => bail!("unknown suite: `{name}`"),
    };

    Ok(Suite { name: name.to_owned(), root, adapter })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut suites = Vec::new();
    for name in &cli.suites {
        suites.push(build_suite(name, cli.include_gpl)?);
    }

    let report = rcc_conformance::run_suites(&cli.rcc, &suites);

    let json = report.to_json_pretty();

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    std::fs::write(&cli.output, &json)
        .with_context(|| format!("writing report to {}", cli.output.display()))?;

    let total: u32 = report.suites.iter().map(|s| s.cases.len() as u32).sum();
    let pass: u32 = report.suites.iter().map(|s| s.counts().pass).sum();
    let fail: u32 = report.suites.iter().map(|s| s.counts().fail).sum();
    let xfail: u32 = report.suites.iter().map(|s| s.counts().xfail).sum();
    let skip: u32 = report.suites.iter().map(|s| s.counts().skip).sum();

    eprintln!(
        "wrote {} ({total} cases: {pass} pass, {fail} fail, {xfail} xfail, {skip} skip)",
        cli.output.display(),
    );

    Ok(())
}
