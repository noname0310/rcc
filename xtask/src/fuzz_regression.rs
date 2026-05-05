//! Promote reviewed libFuzzer crash artifacts into checked-in regression seeds.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const TARGETS: &[&str] = &["lex", "preprocess", "parse"];

/// Result of promoting one crash artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Promotion {
    /// Destination path under `fuzz/corpus/<target>/`.
    pub dest: PathBuf,
    /// Command that should reproduce the artifact on the current checkout.
    pub reproduce: String,
    /// Command that minimizes the promoted artifact in place.
    pub minimize: String,
}

/// Copy `artifact` into `fuzz/corpus/<target>/` and print reproduction hints.
pub fn run(root: &Path, target: &str, artifact: &Path, name: Option<&str>) -> Result<Promotion> {
    validate_target(target)?;
    let artifact = absolutize(root, artifact);
    anyhow::ensure!(artifact.is_file(), "artifact not found: {}", artifact.display());

    let bytes = fs::read(&artifact).with_context(|| format!("reading {}", artifact.display()))?;
    let dest_name = match name {
        Some(name) => validate_seed_name(name)?.to_owned(),
        None => default_seed_name(&artifact)?,
    };
    let corpus_dir = root.join("fuzz").join("corpus").join(target);
    fs::create_dir_all(&corpus_dir)
        .with_context(|| format!("creating {}", corpus_dir.display()))?;
    let dest = corpus_dir.join(dest_name);

    if dest.exists() {
        let existing = fs::read(&dest).with_context(|| format!("reading {}", dest.display()))?;
        if existing != bytes {
            bail!("destination already exists with different contents: {}", dest.display());
        }
    } else {
        fs::write(&dest, &bytes).with_context(|| format!("writing {}", dest.display()))?;
    }

    let corpus_arg = format!("corpus/{}/{}", target, dest.file_name().unwrap().to_string_lossy());
    let reproduce = format!("cd fuzz && cargo +nightly fuzz run {target} {corpus_arg} -- -runs=1");
    let minimize = format!("cd fuzz && cargo +nightly fuzz tmin {target} {corpus_arg}");
    println!("promoted {}", dest.display());
    println!("reproduce: {reproduce}");
    println!("minimize:  {minimize}");

    Ok(Promotion { dest, reproduce, minimize })
}

fn validate_target(target: &str) -> Result<()> {
    if TARGETS.contains(&target) {
        Ok(())
    } else {
        bail!("unknown fuzz target `{target}`; expected one of: {}", TARGETS.join(", "))
    }
}

fn validate_seed_name(name: &str) -> Result<&str> {
    if name.is_empty() {
        bail!("seed name must not be empty");
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        bail!("seed name must be a plain filename, got `{name}`");
    }
    Ok(name)
}

fn default_seed_name(artifact: &Path) -> Result<String> {
    let filename = artifact.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        anyhow::anyhow!("artifact path has no UTF-8 filename: {}", artifact.display())
    })?;
    Ok(format!("regression-{}", sanitize_filename(filename)))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '-',
        })
        .collect()
}

fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rejects_unknown_target() {
        let tmp = temp_root("rejects_unknown_target");
        let artifact = tmp.join("crash");
        fs::write(&artifact, b"boom").unwrap();
        let err = run(&tmp, "semantic", &artifact, None).unwrap_err();
        assert!(err.to_string().contains("unknown fuzz target"));
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn promotes_artifact_with_explicit_name() {
        let tmp = temp_root("promotes_artifact_with_explicit_name");
        let artifact = tmp.join("artifact-crash");
        fs::write(&artifact, b"#include \"x.h\"\n").unwrap();

        let promoted =
            run(&tmp, "preprocess", &artifact, Some("recursive-include.rccfuzz")).unwrap();

        assert_eq!(fs::read(&promoted.dest).unwrap(), b"#include \"x.h\"\n");
        assert!(promoted.dest.ends_with("fuzz/corpus/preprocess/recursive-include.rccfuzz"));
        assert!(promoted.reproduce.contains("cargo +nightly fuzz run preprocess"));
        assert!(promoted.minimize.contains("cargo +nightly fuzz tmin preprocess"));
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn refuses_to_overwrite_different_seed() {
        let tmp = temp_root("refuses_to_overwrite_different_seed");
        let corpus = tmp.join("fuzz").join("corpus").join("lex");
        fs::create_dir_all(&corpus).unwrap();
        fs::write(corpus.join("existing"), b"old").unwrap();
        let artifact = tmp.join("crash");
        fs::write(&artifact, b"new").unwrap();

        let err = run(&tmp, "lex", &artifact, Some("existing")).unwrap_err();

        assert!(err.to_string().contains("already exists"));
        let _ = fs::remove_dir_all(tmp);
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("rcc-xtask-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
