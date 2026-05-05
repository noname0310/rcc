//! Publish the crates.io crate graph in dependency order.

use std::env;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};

/// Publish options.
#[derive(Debug, Clone)]
pub struct ReleasePublishOptions {
    /// Print cargo publish commands without running them.
    pub dry_run: bool,
    /// Pass `--allow-dirty` to cargo publish.
    pub allow_dirty: bool,
    /// Pass `--no-verify` to cargo publish.
    pub no_verify: bool,
    /// Environment variable containing the crates.io token.
    pub token_env: Option<String>,
    /// Resume publishing at this crate/package name.
    pub start_at: Option<String>,
}

/// Crates.io publish order for the internal compiler crate graph.
pub const PUBLISH_ORDER: &[PublishTarget] = &[
    PublishTarget::package("rcc_span"),
    PublishTarget::package("rcc_target"),
    PublishTarget::package("rcc_data_structures"),
    PublishTarget::package("rcc_errors"),
    PublishTarget::package("rcc_session"),
    PublishTarget::package("rcc_ast"),
    PublishTarget::package("rcc_hir"),
    PublishTarget::package("rcc_lexer"),
    PublishTarget::package("rcc_preprocess"),
    PublishTarget::package("rcc_parse"),
    PublishTarget::package("rcc_typeck"),
    PublishTarget::package("rcc_hir_lower"),
    PublishTarget::package("rcc_cfg"),
    PublishTarget::package("rcc_cfg_transform"),
    PublishTarget::package("rcc_codegen_llvm"),
    PublishTarget::package("rcc_driver"),
    PublishTarget::manifest("rcc-compiler", "crates/rcc_compiler_package/Cargo.toml"),
];

/// Publish all crates in the release graph.
pub fn run(root: &Path, opts: &ReleasePublishOptions) -> Result<()> {
    let token = opts
        .token_env
        .as_ref()
        .map(|name| env::var(name).with_context(|| format!("{name} must be set")))
        .transpose()?;

    let targets = targets_from_start(opts.start_at.as_deref())?;
    for target in targets {
        publish_one(root, target, opts, token.as_deref())?;
        if !opts.dry_run {
            // Give crates.io index propagation a small window before publishing
            // crates that depend on the one just uploaded.
            thread::sleep(Duration::from_secs(8));
        }
    }
    Ok(())
}

fn targets_from_start(start_at: Option<&str>) -> Result<&'static [PublishTarget]> {
    let Some(start_at) = start_at else {
        return Ok(PUBLISH_ORDER);
    };
    let Some(pos) = PUBLISH_ORDER.iter().position(|target| target.name == start_at) else {
        bail!("unknown publish start target `{start_at}`");
    };
    Ok(&PUBLISH_ORDER[pos..])
}

fn publish_one(
    root: &Path,
    target: &PublishTarget,
    opts: &ReleasePublishOptions,
    token: Option<&str>,
) -> Result<()> {
    let mut args = vec!["publish".to_string()];
    match target.kind {
        PublishKind::WorkspacePackage => {
            args.push("-p".to_string());
            args.push(target.name.to_string());
        }
        PublishKind::ManifestPath => {
            args.push("--manifest-path".to_string());
            args.push(target.manifest_path.expect("manifest target must have path").to_string());
        }
    }
    if opts.allow_dirty {
        args.push("--allow-dirty".to_string());
    }
    if opts.no_verify {
        args.push("--no-verify".to_string());
    }

    let printable = printable_command(&args);
    if opts.dry_run {
        println!("{printable}");
        return Ok(());
    }

    println!("publishing {} via {printable}", target.name);
    let mut command = Command::new("cargo");
    command.args(&args).current_dir(root);
    if let Some(token) = token {
        command.env("CARGO_REGISTRY_TOKEN", token);
    }
    let status =
        command.status().with_context(|| format!("running publish for {}", target.name))?;
    if !status.success() {
        bail!("cargo publish failed for {} with {status}", target.name);
    }
    Ok(())
}

fn printable_command(args: &[String]) -> String {
    std::iter::once("cargo".to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

/// A crate publish target.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PublishTarget {
    /// Crate/package name.
    pub name: &'static str,
    kind: PublishKind,
    manifest_path: Option<&'static str>,
}

impl PublishTarget {
    const fn package(name: &'static str) -> Self {
        Self { name, kind: PublishKind::WorkspacePackage, manifest_path: None }
    }

    const fn manifest(name: &'static str, path: &'static str) -> Self {
        Self { name, kind: PublishKind::ManifestPath, manifest_path: Some(path) }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PublishKind {
    WorkspacePackage,
    ManifestPath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_order_starts_with_dependency_leaves_and_ends_with_distribution_crate() {
        assert_eq!(PUBLISH_ORDER.first().unwrap().name, "rcc_span");
        assert_eq!(PUBLISH_ORDER.last().unwrap().name, "rcc-compiler");
        let driver = PUBLISH_ORDER.iter().position(|target| target.name == "rcc_driver").unwrap();
        let dist = PUBLISH_ORDER.iter().position(|target| target.name == "rcc-compiler").unwrap();
        assert!(driver < dist);
    }

    #[test]
    fn printable_command_does_not_include_tokens() {
        let args = vec!["publish".to_string(), "-p".to_string(), "rcc_span".to_string()];
        assert_eq!(printable_command(&args), "cargo publish -p rcc_span");
    }

    #[test]
    fn start_at_returns_suffix() {
        let targets = targets_from_start(Some("rcc_ast")).unwrap();
        assert_eq!(targets.first().unwrap().name, "rcc_ast");
        assert_eq!(targets.last().unwrap().name, "rcc-compiler");
        assert!(targets_from_start(Some("missing")).is_err());
    }

    #[test]
    fn dry_run_publish_walks_selected_suffix_without_token() {
        let opts = ReleasePublishOptions {
            dry_run: true,
            allow_dirty: true,
            no_verify: true,
            token_env: None,
            start_at: Some("rcc_driver".to_owned()),
        };

        run(Path::new("."), &opts).unwrap();
    }

    #[test]
    fn missing_token_env_is_reported_before_publish() {
        let token_env = format!("RCC_TEST_MISSING_CARGO_TOKEN_{}", std::process::id());
        let opts = ReleasePublishOptions {
            dry_run: true,
            allow_dirty: false,
            no_verify: false,
            token_env: Some(token_env.clone()),
            start_at: Some("rcc_span".to_owned()),
        };

        let err = run(Path::new("."), &opts).unwrap_err().to_string();
        assert!(err.contains(&format!("{token_env} must be set")));
    }
}
