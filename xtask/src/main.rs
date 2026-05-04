//! Project-level automation. Invoked with `cargo xtask <subcommand>`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
    }
}

fn project_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR points at xtask/; root is one level up.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must have a parent directory")
        .to_path_buf()
}
