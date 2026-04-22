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
