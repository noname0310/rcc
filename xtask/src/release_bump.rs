//! Version bump helper used by the release workflow.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Semver component to bump.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BumpKind {
    /// Increment MAJOR and reset MINOR/PATCH.
    Major,
    /// Increment MINOR and reset PATCH.
    Minor,
    /// Increment PATCH.
    Patch,
}

impl BumpKind {
    /// Parse a workflow/CLI bump string.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            "patch" => Ok(Self::Patch),
            _ => bail!("bump must be one of: major, minor, patch"),
        }
    }
}

/// Options for `cargo xtask release-bump`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ReleaseBumpOptions {
    /// Component to bump.
    pub bump: BumpKind,
    /// Print the computed version without editing files.
    pub dry_run: bool,
}

/// Run a release version bump.
pub fn run(root: &Path, opts: ReleaseBumpOptions) -> Result<()> {
    let current = read_workspace_version(root)?;
    let next = bump_version(&current, opts.bump)?;
    let tag = format!("v{next}");
    println!("current={current}");
    println!("next={next}");
    println!("tag={tag}");
    if opts.dry_run {
        return Ok(());
    }

    update_versions(root, &current, &next)?;
    update_changelog(root, &next)?;
    ensure_release_notes(root, &next)?;
    run_cargo_update(root)?;
    Ok(())
}

fn read_workspace_version(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml")).context("reading Cargo.toml")?;
    let mut in_workspace_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        }
        if in_workspace_package && trimmed.starts_with('[') {
            break;
        }
        if in_workspace_package {
            if let Some(version) = parse_version_line(trimmed) {
                return Ok(version.to_string());
            }
        }
    }
    bail!("Cargo.toml missing [workspace.package] version")
}

fn parse_version_line(line: &str) -> Option<&str> {
    line.strip_prefix("version = \"")?.strip_suffix('"')
}

fn bump_version(current: &str, kind: BumpKind) -> Result<String> {
    let parts: Vec<_> = current.split('.').collect();
    if parts.len() != 3 {
        bail!("version must be MAJOR.MINOR.PATCH, got {current}");
    }
    let major: u64 = parts[0].parse().with_context(|| format!("parsing major in {current}"))?;
    let minor: u64 = parts[1].parse().with_context(|| format!("parsing minor in {current}"))?;
    let patch: u64 = parts[2].parse().with_context(|| format!("parsing patch in {current}"))?;
    let (major, minor, patch) = match kind {
        BumpKind::Major => (major + 1, 0, 0),
        BumpKind::Minor => (major, minor + 1, 0),
        BumpKind::Patch => (major, minor, patch + 1),
    };
    Ok(format!("{major}.{minor}.{patch}"))
}

fn update_versions(root: &Path, current: &str, next: &str) -> Result<()> {
    replace_version_literal(&root.join("Cargo.toml"), current, next)?;
    replace_version_literal(&root.join("crates/rcc_compiler_package/Cargo.toml"), current, next)?;
    Ok(())
}

fn replace_version_literal(path: &Path, current: &str, next: &str) -> Result<()> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let from = format!("version = \"{current}\"");
    let to = format!("version = \"{next}\"");
    if !text.contains(&from) {
        bail!("{} does not contain {from}", path.display());
    }
    let updated = text.replace(&from, &to);
    fs::write(path, updated).with_context(|| format!("writing {}", path.display()))
}

fn update_changelog(root: &Path, next: &str) -> Result<()> {
    let path = root.join("CHANGELOG.md");
    let existing = fs::read_to_string(&path).unwrap_or_else(|_| "# Changelog\n\n".to_string());
    let marker = format!("## {next}");
    if existing.contains(&marker) {
        return Ok(());
    }
    let entry = format!(
        "## {next}\n\n- Release candidate prepared by `cargo xtask release-bump`.\n- See `docs/release-notes-v{next}.md` and `docs/conformance.md` for release details.\n\n"
    );
    let updated = if let Some(rest) = existing.strip_prefix("# Changelog\n\n") {
        format!("# Changelog\n\n{entry}{rest}")
    } else {
        format!("# Changelog\n\n{entry}{existing}")
    };
    fs::write(path, updated).context("writing CHANGELOG.md")
}

fn ensure_release_notes(root: &Path, next: &str) -> Result<()> {
    let path = root.join(format!("docs/release-notes-v{next}.md"));
    if path.exists() {
        return Ok(());
    }
    let text = format!(
        "# rcc v{next} Release Notes\n\n\
         ## Supported Surface\n\n\
         - Hosted C99 compiler targeting `x86_64-unknown-linux-gnu` for the M7 release.\n\
         - LLVM 18 backend with hosted libc and external clang-compatible linker tooling.\n\n\
         ## Conformance Snapshot\n\n\
         See [`conformance.md`](conformance.md) for the frozen release dashboard and xfail policy.\n\n\
         ## Known Non-goals\n\n\
         - Windows target support.\n\
         - Bundled libc/glibc/MSVCRT implementation.\n\
         - Native linker implementation.\n\
         - Treating exploratory GNU/C11 suite failures as strict C99 release failures.\n"
    );
    fs::write(path, text).with_context(|| format!("writing release notes for {next}"))
}

fn run_cargo_update(root: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .arg("update")
        .arg("-w")
        .current_dir(root)
        .status()
        .context("running cargo update -w")?;
    if !status.success() {
        bail!("cargo update -w failed with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_math_resets_lower_components() {
        assert_eq!(bump_version("0.0.1", BumpKind::Patch).unwrap(), "0.0.2");
        assert_eq!(bump_version("0.0.1", BumpKind::Minor).unwrap(), "0.1.0");
        assert_eq!(bump_version("0.9.9", BumpKind::Major).unwrap(), "1.0.0");
    }

    #[test]
    fn parses_only_expected_bump_names() {
        assert_eq!(BumpKind::parse("major").unwrap(), BumpKind::Major);
        assert!(BumpKind::parse("release").is_err());
    }
}
