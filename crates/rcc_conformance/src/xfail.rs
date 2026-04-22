//! Expected-failure list parser.
//!
//! `xfail.toml` lives next to each suite under `third_party/testsuites/<suite>/xfail.toml`
//! and contains entries like:
//!
//! ```toml
//! [[xfail]]
//! id     = "c-testsuite::00055"
//! reason = "bit-fields not yet implemented (M4)"
//! ```
//!
//! Lowering the pass/fail bar to a single knob that the conformance harness
//! reads lets us keep CI green while marking known gaps explicitly.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Root object of `xfail.toml`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct XFailFile {
    /// Expected-failure entries.
    #[serde(default)]
    pub xfail: Vec<XFailEntry>,
}

/// One expected-failure entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XFailEntry {
    /// Test-case id (matches `TestCase::id`).
    pub id: String,
    /// Human-readable reason.
    pub reason: String,
}

/// Load `xfail.toml` from `path`. Missing file = empty list.
pub fn load(path: &Path) -> anyhow::Result<XFailFile> {
    if !path.exists() {
        return Ok(XFailFile::default());
    }
    let s = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&s)?)
}
