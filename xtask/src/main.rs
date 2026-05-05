//! Project-level automation. Invoked with `cargo xtask <subcommand>`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod fetch;
mod manifest;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "rcc project maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Download / update every external test suite listed in
    /// `third_party/MANIFEST.toml` into `third_party/testsuites/`.
    FetchTestsuites {
        /// Also fetch optional external suites (gcc-torture, tcc-tests2).
        #[arg(long)]
        include_gpl: bool,
        /// Only fetch this named suite.
        #[arg(long)]
        only: Option<String>,
    },
    /// Print the pinned manifest.
    ShowManifest,
    /// Verify every error code in codes.rs has a docs/error-codes.md
    /// entry and vice-versa. CI should run this gate.
    CheckErrorCodes,
    /// Compare xfail.toml entries between two git revisions.
    XfailReport {
        /// Git range in the form OLD..NEW.
        range: String,
    },
    /// Run cargo-llvm-cov and enforce the documented coverage thresholds.
    Coverage {
        /// LCOV artifact path to create.
        #[arg(long, default_value = "lcov.info")]
        lcov: PathBuf,
        /// JSON summary artifact path to create.
        #[arg(long, default_value = "target/coverage/coverage-summary.json")]
        json: PathBuf,
        /// Re-check an existing JSON summary and artifact paths without running tests.
        #[arg(long)]
        check_only: bool,
    },
    /// Promote a reviewed libFuzzer crash artifact into a corpus seed.
    FuzzRegression {
        /// Fuzz target name: lex, preprocess, or parse.
        #[arg(value_parser = ["lex", "preprocess", "parse"])]
        target: String,
        /// Crash artifact path from `fuzz/artifacts/<target>/...`.
        artifact: PathBuf,
        /// Checked-in seed filename to use under `fuzz/corpus/<target>/`.
        #[arg(long)]
        name: Option<String>,
    },
    /// Compare generated-code runtime against a host C compiler.
    BenchRuntime {
        /// Path to a release `rcc` binary with LLVM backend enabled.
        #[arg(long, default_value = "target/release/rcc")]
        rcc: PathBuf,
        /// Host C compiler baseline (`cc`, `clang`, or an absolute path).
        #[arg(long, default_value = "cc")]
        host_cc: PathBuf,
        /// Markdown report path.
        #[arg(long, default_value = "docs/perf-baseline.md")]
        out: PathBuf,
        /// Number of executable runs per program/compiler pair.
        #[arg(long, default_value_t = 5)]
        iterations: usize,
    },
    /// Run local release-candidate gates and write logs under reports/.
    ReleaseCheck {
        /// Directory for command logs and summary.
        #[arg(long, default_value = "reports/release-check/latest")]
        report_dir: PathBuf,
        /// Skip the LLVM-enabled workspace test gate.
        #[arg(long)]
        skip_llvm: bool,
        /// Skip the cargo-llvm-cov coverage gate.
        #[arg(long)]
        skip_coverage: bool,
        /// Skip the libFuzzer smoke gate.
        #[arg(long)]
        skip_fuzz: bool,
        /// Skip the conformance dashboard refresh gate.
        #[arg(long)]
        skip_conformance: bool,
        /// Run the crates.io-facing package archive check.
        #[arg(long)]
        registry_package: bool,
    },
    /// Compute or apply a release version bump.
    ReleaseBump {
        /// Version component to bump: major, minor, or patch.
        #[arg(value_parser = ["major", "minor", "patch"])]
        bump: String,
        /// Only print current/next/tag; do not edit files.
        #[arg(long)]
        dry_run: bool,
    },
    /// Publish the internal crate graph and rcc-compiler distribution crate.
    ReleasePublish {
        /// Print cargo publish commands without running them.
        #[arg(long)]
        dry_run: bool,
        /// Pass --allow-dirty to cargo publish.
        #[arg(long)]
        allow_dirty: bool,
        /// Pass --no-verify to cargo publish.
        #[arg(long)]
        no_verify: bool,
        /// Environment variable containing the crates.io token.
        #[arg(long)]
        token_env: Option<String>,
        /// Resume publishing at this crate/package name.
        #[arg(long)]
        start_at: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::FetchTestsuites { include_gpl, only } => {
            let manifest_path = project_root().join("third_party/MANIFEST.toml");
            let manifest = manifest::load(&manifest_path)
                .with_context(|| format!("reading {}", manifest_path.display()))?;
            fetch::run(&manifest, include_gpl, only.as_deref())?;
            Ok(())
        }
        Cmd::ShowManifest => {
            let manifest_path = project_root().join("third_party/MANIFEST.toml");
            let manifest = manifest::load(&manifest_path)?;
            println!("{manifest:#?}");
            Ok(())
        }
        Cmd::CheckErrorCodes => xtask::check_error_codes::run(&project_root()),
        Cmd::XfailReport { range } => xtask::xfail_report::run(&project_root(), &range),
        Cmd::Coverage { lcov, json, check_only } => {
            xtask::coverage::run(&project_root(), &lcov, &json, check_only)
        }
        Cmd::FuzzRegression { target, artifact, name } => {
            xtask::fuzz_regression::run(&project_root(), &target, &artifact, name.as_deref())?;
            Ok(())
        }
        Cmd::BenchRuntime { rcc, host_cc, out, iterations } => {
            let opts = xtask::bench_runtime::BenchRuntimeOptions { rcc, host_cc, out, iterations };
            xtask::bench_runtime::run(&project_root(), &opts)
        }
        Cmd::ReleaseCheck {
            report_dir,
            skip_llvm,
            skip_coverage,
            skip_fuzz,
            skip_conformance,
            registry_package,
        } => {
            let opts = xtask::release_check::ReleaseCheckOptions {
                report_dir,
                skip_llvm,
                skip_coverage,
                skip_fuzz,
                skip_conformance,
                registry_package,
            };
            xtask::release_check::run(&project_root(), &opts)
        }
        Cmd::ReleaseBump { bump, dry_run } => {
            let bump = xtask::release_bump::BumpKind::parse(&bump)?;
            xtask::release_bump::run(
                &project_root(),
                xtask::release_bump::ReleaseBumpOptions { bump, dry_run },
            )
        }
        Cmd::ReleasePublish { dry_run, allow_dirty, no_verify, token_env, start_at } => {
            let opts = xtask::release_publish::ReleasePublishOptions {
                dry_run,
                allow_dirty,
                no_verify,
                token_env,
                start_at,
            };
            xtask::release_publish::run(&project_root(), &opts)
        }
    }
}

fn project_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR points at xtask/; root is one level up.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must have a parent directory")
        .to_path_buf()
}
