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
    /// Optional git tag for shallow clones (`--depth=1 --branch <tag>`).
    /// When set, the clone is done at depth 1 using the tag, and `rev`
    /// serves as the expected commit SHA for verification.
    pub tag: Option<String>,
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

    #[test]
    fn chibicc_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "chibicc").expect("chibicc entry");
        let rev = suite.rev.as_deref().expect("chibicc must have a rev");
        assert!(is_sha_rev(rev), "chibicc rev must be a 40-char hex SHA, got: {rev}");
    }

    #[test]
    fn chibicc_sparse_includes_test_and_license() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "chibicc").expect("chibicc entry");
        assert!(
            suite.sparse.iter().any(|p| p == "test"),
            "chibicc sparse list must include 'test'"
        );
        assert!(
            suite.sparse.iter().any(|p| p == "LICENSE"),
            "chibicc sparse list must include 'LICENSE'"
        );
    }

    #[test]
    fn gcc_torture_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "gcc-torture").expect("gcc-torture entry");
        let rev = suite.rev.as_deref().expect("gcc-torture must have a rev");
        assert!(is_sha_rev(rev), "gcc-torture rev must be a 40-char hex SHA, got: {rev}");
    }

    #[test]
    fn gcc_torture_is_gpl_gated() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "gcc-torture").expect("gcc-torture entry");
        assert!(suite.gpl, "gcc-torture must have gpl = true");
    }

    #[test]
    fn gcc_torture_sparse_includes_torture_and_copying() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "gcc-torture").expect("gcc-torture entry");
        assert!(
            suite.sparse.iter().any(|p| p == "gcc/testsuite/gcc.c-torture"),
            "gcc-torture sparse must include the torture test path"
        );
        assert!(
            suite.sparse.iter().any(|p| p == "COPYING3"),
            "gcc-torture sparse must include COPYING3 for license extraction"
        );
    }

    #[test]
    fn gcc_torture_has_tag() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "gcc-torture").expect("gcc-torture entry");
        let tag = suite.tag.as_deref().expect("gcc-torture must have a tag for shallow clone");
        assert!(
            tag.starts_with("releases/gcc-"),
            "gcc-torture tag should be a GCC release tag, got: {tag}"
        );
    }

    #[test]
    fn tcc_tests2_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "tcc-tests2").expect("tcc-tests2 entry");
        let rev = suite.rev.as_deref().expect("tcc-tests2 must have a rev");
        assert!(is_sha_rev(rev), "tcc-tests2 rev must be a 40-char hex SHA, got: {rev}");
    }

    #[test]
    fn tcc_tests2_is_gpl_gated() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "tcc-tests2").expect("tcc-tests2 entry");
        assert!(suite.gpl, "tcc-tests2 must have gpl = true");
    }

    #[test]
    fn tcc_tests2_sparse_includes_tests2_and_copying() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "tcc-tests2").expect("tcc-tests2 entry");
        assert!(
            suite.sparse.iter().any(|p| p == "tests/tests2"),
            "tcc-tests2 sparse must include 'tests/tests2'"
        );
        assert!(
            suite.sparse.iter().any(|p| p == "COPYING"),
            "tcc-tests2 sparse must include 'COPYING' for license extraction"
        );
    }

    #[test]
    fn tcc_tests2_has_tag() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "tcc-tests2").expect("tcc-tests2 entry");
        let tag = suite.tag.as_deref().expect("tcc-tests2 must have a tag for shallow clone");
        assert!(
            tag.starts_with("release_"),
            "tcc-tests2 tag should be a TCC release tag, got: {tag}"
        );
    }

    #[test]
    fn tcc_tests2_uses_github_mirror() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "tcc-tests2").expect("tcc-tests2 entry");
        let git = suite.git.as_deref().expect("tcc-tests2 must have a git URL");
        assert_eq!(
            git, "https://github.com/TinyCC/tinycc.git",
            "repo.or.cz is too flaky for CI runners; use the GitHub mirror"
        );
    }

    #[test]
    fn llvm_test_suite_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite =
            m.suite.iter().find(|s| s.name == "llvm-test-suite").expect("llvm-test-suite entry");
        let rev = suite.rev.as_deref().expect("llvm-test-suite must have a rev");
        assert!(is_sha_rev(rev), "llvm-test-suite rev must be a 40-char hex SHA, got: {rev}");
    }

    #[test]
    fn llvm_test_suite_not_gpl_gated() {
        let m = load_real_manifest();
        let suite =
            m.suite.iter().find(|s| s.name == "llvm-test-suite").expect("llvm-test-suite entry");
        assert!(!suite.gpl, "llvm-test-suite is Apache-2.0, must not be gpl-gated");
    }

    #[test]
    fn csmith_rev_is_pinned_sha() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "csmith").expect("csmith entry");
        let rev = suite.rev.as_deref().expect("csmith must have a rev");
        assert!(is_sha_rev(rev), "csmith rev must be a 40-char hex SHA, got: {rev}");
    }

    #[test]
    fn csmith_not_gpl_gated() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "csmith").expect("csmith entry");
        assert!(!suite.gpl, "csmith is BSD-2-Clause, must not be gpl-gated");
    }

    #[test]
    fn csmith_no_sparse() {
        let m = load_real_manifest();
        let suite = m.suite.iter().find(|s| s.name == "csmith").expect("csmith entry");
        assert!(suite.sparse.is_empty(), "csmith should clone the full repo (no sparse checkout)");
    }

    #[test]
    fn manifest_has_expected_suite_count() {
        let m = load_real_manifest();
        assert_eq!(m.suite.len(), 6, "MANIFEST.toml should contain exactly 6 suites");
    }

    #[test]
    fn all_revs_are_pinned_shas() {
        let m = load_real_manifest();
        for suite in &m.suite {
            if let Some(rev) = &suite.rev {
                assert!(
                    is_sha_rev(rev),
                    "suite `{}` rev must be a 40-char hex SHA, got: {rev}",
                    suite.name
                );
            } else {
                assert!(
                    suite.git.is_none(),
                    "suite `{}` has `git` but no `rev` — every git suite must be pinned",
                    suite.name
                );
            }
        }
    }

    #[test]
    fn show_manifest_output_contains_all_suites() {
        let m = load_real_manifest();
        let output = format!("{m:#?}");
        let expected_names =
            ["c-testsuite", "chibicc", "gcc-torture", "tcc-tests2", "llvm-test-suite", "csmith"];
        for name in &expected_names {
            assert!(output.contains(name), "show-manifest output should contain suite `{name}`");
        }
    }

    #[test]
    fn llvm_test_suite_sparse_includes_unittests_and_license() {
        let m = load_real_manifest();
        let suite =
            m.suite.iter().find(|s| s.name == "llvm-test-suite").expect("llvm-test-suite entry");
        assert!(
            suite.sparse.iter().any(|p| p == "SingleSource/UnitTests"),
            "llvm-test-suite sparse must include 'SingleSource/UnitTests'"
        );
        assert!(
            suite.sparse.iter().any(|p| p == "LICENSE.TXT"),
            "llvm-test-suite sparse must include 'LICENSE.TXT' for license extraction"
        );
        assert!(
            suite.sparse.iter().any(|p| p == "CMakeLists.txt"),
            "llvm-test-suite sparse must include 'CMakeLists.txt' for metadata"
        );
    }
}
