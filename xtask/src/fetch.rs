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
        write_suite_readme(suite, &dst)?;

        if suite.name == "csmith" {
            write_csmith_install_md(&dst)?;
        }
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
        run_cmd(Command::new("git").args(["-C", &dst.to_string_lossy(), "checkout", rev]))?;
    } else if let Some(tag) = &suite.tag {
        // Shallow sparse clone pinned to a tag — keeps multi-GB repos manageable.
        run_cmd(Command::new("git").args([
            "clone",
            "--filter=blob:none",
            "--sparse",
            "--depth=1",
            "--branch",
            tag,
            git,
            &dst.to_string_lossy(),
        ]))?;
        let mut sparse = Command::new("git");
        sparse.args(["-C", &dst.to_string_lossy(), "sparse-checkout", "set", "--no-cone"]);
        for p in &suite.sparse {
            sparse.arg(p);
        }
        run_cmd(&mut sparse)?;
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
        run_cmd(Command::new("git").args(["-C", &dst.to_string_lossy(), "checkout", rev]))?;
    }
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

/// Write a README.md into a suite's checkout directory naming the upstream
/// license and warning against copying sources into the repo tree.
fn write_suite_readme(suite: &Suite, dst: &Path) -> Result<()> {
    let readme = dst.join("README.md");
    let content = format!(
        "# {name}\n\
         \n\
         Upstream: {git}\n\
         License: {license}\n\
         \n\
         > **Warning:** do not copy these sources into the rcc repository.\n\
         > This directory is populated by `cargo xtask fetch-testsuites` and\n\
         > is git-ignored. The tests are executed as separate processes and\n\
         > are never linked into any rcc binary.\n",
        name = suite.name,
        git = suite.git.as_deref().unwrap_or("(unknown)"),
        license = suite.license,
    );
    std::fs::write(&readme, content).with_context(|| format!("writing {}", readme.display()))?;
    println!("  readme -> {}", readme.display());
    Ok(())
}

/// Write `INSTALL.md` into the csmith checkout with build instructions.
/// csmith is a build tool (random C program generator), not a test data
/// source. We clone it for future differential fuzzing (phase 12) but do
/// not build it during fetch.
fn write_csmith_install_md(dst: &Path) -> Result<()> {
    let install = dst.join("INSTALL.md");
    let content = "\
# Building csmith

csmith is a random C program generator used for differential fuzzing.
It is **not** built during `cargo xtask fetch-testsuites`; this document
records the manual build steps for the CI runner.

## Prerequisites

- CMake >= 3.10
- A C++ compiler (GCC or Clang)
- `m4` (GNU m4)

## Build commands

```sh
cd third_party/testsuites/csmith
cmake -S . -B build
cmake --build build
```

The `csmith` binary will be at `build/src/csmith` (Linux/macOS) or
`build\\src\\Debug\\csmith.exe` (Windows).

## Usage (differential fuzzing)

See `tasks/12-fuzz-differential/` for the harness that invokes csmith
to generate random C programs and compares rcc output against a
reference compiler.
";
    std::fs::write(&install, content).with_context(|| format!("writing {}", install.display()))?;
    println!("  install -> {}", install.display());
    Ok(())
}

/// After a suite is fetched, copy its license file into `LICENSES/<name>.txt`.
fn copy_license(suite: &Suite, src_dir: &Path, root: &Path) -> Result<()> {
    let candidates = ["LICENSE", "LICENSE.md", "LICENSE.txt", "LICENSE.TXT", "COPYING", "COPYING3"];
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
            tag: None,
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
            tag: None,
            tarball: None,
            sparse: vec![],
        };

        // Should not error, just print a warning.
        copy_license(&suite, &src_dir, &root).unwrap();

        let dst = root.join("LICENSES/empty-suite.txt");
        assert!(!dst.exists(), "no license should be created when source is missing");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_license_finds_license_txt_uppercase() {
        let tmp = std::env::temp_dir().join("rcc_test_copy_license_upper");
        let _ = fs::remove_dir_all(&tmp);
        let src_dir = tmp.join("suite");
        let root = tmp.join("root");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&root).unwrap();

        fs::write(src_dir.join("LICENSE.TXT"), "Apache-2.0\n").unwrap();

        let suite = Suite {
            name: "llvm-test-suite".into(),
            description: String::new(),
            license: "Apache-2.0 WITH LLVM-exception".into(),
            gpl: false,
            git: None,
            rev: None,
            tag: None,
            tarball: None,
            sparse: vec![],
        };

        copy_license(&suite, &src_dir, &root).unwrap();

        let dst = root.join("LICENSES/llvm-test-suite.txt");
        assert!(dst.exists(), "LICENSE.TXT (uppercase) should be found and copied");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "Apache-2.0\n");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn gpl_suite_skipped_without_flag() {
        let manifest = crate::manifest::Manifest {
            suite: vec![Suite {
                name: "gpl-test".into(),
                description: String::new(),
                license: "GPL-3.0".into(),
                gpl: true,
                git: Some("https://example.com/repo.git".into()),
                rev: Some("abc".into()),
                tag: None,
                tarball: None,
                sparse: vec![],
            }],
        };
        // include_gpl = false → should skip and not error
        // (actual git clone is never reached because gpl gate fires first)
        let result = run(&manifest, false, None);
        assert!(result.is_ok(), "skipping GPL suites should not error");
    }

    #[test]
    fn write_csmith_install_md_creates_file() {
        let tmp = std::env::temp_dir().join("rcc_test_csmith_install");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        write_csmith_install_md(&tmp).unwrap();

        let install = tmp.join("INSTALL.md");
        assert!(install.exists(), "INSTALL.md should be created");
        let content = fs::read_to_string(&install).unwrap();
        assert!(content.contains("cmake -S . -B build"), "should document cmake configure");
        assert!(content.contains("cmake --build build"), "should document cmake build");
        assert!(content.contains("m4"), "should mention m4 prerequisite");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_suite_readme_creates_file() {
        let tmp = std::env::temp_dir().join("rcc_test_write_readme");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let suite = Suite {
            name: "gcc-torture".into(),
            description: "GCC C torture tests".into(),
            license: "GPL-3.0-or-later WITH GCC-exception-3.1".into(),
            gpl: true,
            git: Some("https://gcc.gnu.org/git/gcc.git".into()),
            rev: Some("abc123".into()),
            tag: Some("releases/gcc-14.1.0".into()),
            tarball: None,
            sparse: vec![],
        };

        write_suite_readme(&suite, &tmp).unwrap();

        let readme = tmp.join("README.md");
        assert!(readme.exists(), "README.md should be created");
        let content = fs::read_to_string(&readme).unwrap();
        assert!(content.contains("gcc-torture"), "README should name the suite");
        assert!(content.contains("GPL"), "README should mention the license");
        assert!(content.contains("do not copy"), "README should warn about not copying sources");

        let _ = fs::remove_dir_all(&tmp);
    }
}
