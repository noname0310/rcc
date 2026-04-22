//! `fetch-testsuites` subcommand. Shells out to `git` / `curl` to populate
//! `third_party/testsuites/<name>/` from the manifest.
//!
//! Design goals:
//! * Reproducible: every suite is pinned to a specific `rev`.
//! * License-safe: GPL suites are skipped unless `--include-gpl` is passed.
//! * Offline-friendly: if the checkout already exists at the right rev, skip.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::manifest::{Manifest, Suite};

/// Run the fetch.
pub fn run(manifest: &Manifest, include_gpl: bool, only: Option<&str>) -> Result<()> {
    let root = crate::project_root();
    let out_dir = root.join("third_party/testsuites");
    std::fs::create_dir_all(&out_dir)?;

    for suite in &manifest.suite {
        if let Some(name) = only {
            if suite.name != name {
                continue;
            }
        }
        if suite.gpl && !include_gpl {
            println!("skip {:<18} ({}): pass --include-gpl to fetch", suite.name, suite.license);
            continue;
        }

        let dst = out_dir.join(&suite.name);
        println!("-> {:<18} ({}) -> {}", suite.name, suite.license, dst.display());

        if let Some(git) = &suite.git {
            fetch_git(suite, git, &dst)?;
        } else if let Some(tarball) = &suite.tarball {
            fetch_tarball(suite, tarball, &dst)?;
        } else {
            bail!("suite `{}` has neither `git` nor `tarball`", suite.name);
        }

        copy_license(suite, &dst, &root)?;
    }
    Ok(())
}

fn fetch_git(suite: &Suite, git: &str, dst: &Path) -> Result<()> {
    let rev = suite.rev.as_deref().context("git suite missing `rev`")?;
    if dst.join(".git").is_dir() {
        // Already cloned — fetch + checkout the pinned rev.
        run_cmd(Command::new("git").args(["-C", &dst.to_string_lossy(), "fetch", "--all"]))?;
        run_cmd(Command::new("git").args(["-C", &dst.to_string_lossy(), "checkout", rev]))?;
        return Ok(());
    }
    if suite.sparse.is_empty() {
        run_cmd(Command::new("git").args(["clone", git, &dst.to_string_lossy()]))?;
    } else {
        run_cmd(Command::new("git").args([
            "clone",
            "--filter=blob:none",
            "--sparse",
            git,
            &dst.to_string_lossy(),
        ]))?;
        let mut sparse = Command::new("git");
        sparse.args(["-C", &dst.to_string_lossy(), "sparse-checkout", "set", "--no-cone"]);
        for p in &suite.sparse {
            sparse.arg(p);
        }
        run_cmd(&mut sparse)?;
    }
    run_cmd(Command::new("git").args(["-C", &dst.to_string_lossy(), "checkout", rev]))?;
    Ok(())
}

fn fetch_tarball(suite: &Suite, url: &str, dst: &Path) -> Result<()> {
    if dst.exists() {
        println!("  (tarball destination exists, skipping: {})", dst.display());
        return Ok(());
    }
    let tmp = dst.with_extension("tar.download");
    run_cmd(Command::new("curl").args(["-L", "-o", &tmp.to_string_lossy(), url]))?;
    std::fs::create_dir_all(dst)?;
    run_cmd(Command::new("tar").args([
        "-xf",
        &tmp.to_string_lossy(),
        "-C",
        &dst.to_string_lossy(),
        "--strip-components=1",
    ]))?;
    std::fs::remove_file(&tmp).ok();
    let _ = suite; // rev verification on tarballs is out of scope for the skeleton.
    Ok(())
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("command failed with status {status}: {cmd:?}");
    }
    Ok(())
}

/// After a suite is fetched, copy its license file into `LICENSES/<name>.txt`.
fn copy_license(suite: &Suite, src_dir: &Path, root: &Path) -> Result<()> {
    let candidates = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"];
    let license_src = candidates.iter().map(|f| src_dir.join(f)).find(|p| p.is_file());

    if let Some(src) = license_src {
        let licenses_dir = root.join("LICENSES");
        std::fs::create_dir_all(&licenses_dir)?;
        let dst = licenses_dir.join(format!("{}.txt", suite.name));
        std::fs::copy(&src, &dst)
            .with_context(|| format!("copying {} -> {}", src.display(), dst.display()))?;
        println!("  license -> {}", dst.display());
    } else {
        println!(
            "  warning: no license file found in {} for suite `{}`",
            src_dir.display(),
            suite.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn copy_license_finds_license_file() {
        let tmp = std::env::temp_dir().join("rcc_test_copy_license");
        let _ = fs::remove_dir_all(&tmp);
        let src_dir = tmp.join("suite");
        let root = tmp.join("root");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&root).unwrap();

        fs::write(src_dir.join("LICENSE"), "MIT License\n").unwrap();

        let suite = Suite {
            name: "test-suite".into(),
            description: String::new(),
            license: "MIT".into(),
            gpl: false,
            git: None,
            rev: None,
            tarball: None,
            sparse: vec![],
        };

        copy_license(&suite, &src_dir, &root).unwrap();

        let dst = root.join("LICENSES/test-suite.txt");
        assert!(dst.exists(), "license file should be copied");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "MIT License\n");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_license_no_file_does_not_error() {
        let tmp = std::env::temp_dir().join("rcc_test_copy_license_none");
        let _ = fs::remove_dir_all(&tmp);
        let src_dir = tmp.join("suite");
        let root = tmp.join("root");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&root).unwrap();

        let suite = Suite {
            name: "empty-suite".into(),
            description: String::new(),
            license: "MIT".into(),
            gpl: false,
            git: None,
            rev: None,
            tarball: None,
            sparse: vec![],
        };

        // Should not error, just print a warning.
        copy_license(&suite, &src_dir, &root).unwrap();

        let dst = root.join("LICENSES/empty-suite.txt");
        assert!(!dst.exists(), "no license should be created when source is missing");

        let _ = fs::remove_dir_all(&tmp);
    }
}
