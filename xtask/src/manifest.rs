//! `third_party/MANIFEST.toml` schema + loader. Every suite is pinned to a
//! specific commit / tag so fetch-testsuites is reproducible.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Parsed root of `third_party/MANIFEST.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Every suite listed in the manifest.
    pub suite: Vec<Suite>,
}

/// One suite entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suite {
    /// Stable short name. Also used as directory name under `third_party/testsuites/`.
    pub name: String,
    /// Upstream description.
    pub description: String,
    /// SPDX license identifier.
    pub license: String,
    /// Whether fetching this suite requires `--include-gpl`.
    #[serde(default)]
    pub gpl: bool,
    /// Upstream git URL, or `None` for non-git sources.
    pub git: Option<String>,
    /// Upstream revision (commit/tag/branch). Required when `git` is set.
    pub rev: Option<String>,
    /// Optional tarball URL (used when `git` is absent).
    pub tarball: Option<String>,
    /// Optional sparse-checkout prefixes (git only).
    #[serde(default)]
    pub sparse: Vec<String>,
}

/// Load a manifest from disk.
pub fn load(path: &Path) -> anyhow::Result<Manifest> {
    let s = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_sha_rev(s: &str) -> bool {
        s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
    }

    fn load_real_manifest() -> Manifest {
        let root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let path = root.join("third_party/MANIFEST.toml");
        load(&path).expect("MANIFEST.toml should parse")
    }

    #[test]
    fn c_testsuite_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "c-testsuite").expect("c-testsuite entry");
        let rev = suite.rev.as_deref().expect("c-testsuite must have a rev");
        assert!(is_sha_rev(rev), "c-testsuite rev must be a 40-char hex SHA, got: {rev}");
    }
}
