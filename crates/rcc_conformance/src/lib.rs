//! `rcc_conformance`: run external C test suites and score results.
//!
//! Handles the vendored suites from `third_party/testsuites/`:
//! `c-testsuite`, `chibicc`, `gcc-torture`, `tcc-tests2`, `llvm-test-suite`,
//! plus differential fuzzing driven by `csmith`.
//!
//! Each suite is wrapped by a [`Adapter`] that discovers test cases and runs
//! them against `rcc`. Outcomes are categorised into
//! [`Outcome::Pass`] / [`Outcome::Fail`] / [`Outcome::XFail`] / [`Outcome::Skip`]
//! and rolled up into a [`Report`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod adapters;
pub mod metadata;
pub mod render;
pub mod xfail;

/// Outcome of running a single test case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// Ran and matched the expected output.
    Pass,
    /// Ran but the output or exit code did not match.
    Fail {
        /// Human-readable reason.
        reason: String,
    },
    /// Expected failure: listed in `xfail.toml`; counts as pass.
    #[serde(rename = "xfail")]
    XFail {
        /// `xfail.toml` entry reason.
        reason: String,
    },
    /// Intentionally skipped (unsupported feature).
    Skip {
        /// Skip reason.
        reason: String,
    },
}

/// A single test-case discovered by an adapter.
#[derive(Clone, Debug)]
pub struct TestCase {
    /// Short, stable id (e.g. `c-testsuite::00001`).
    pub id: String,
    /// Absolute path to the primary `.c` file.
    pub path: PathBuf,
}

/// A named test suite wrapping a concrete directory under `third_party/`.
pub struct Suite {
    /// Suite name (shown in reports).
    pub name: String,
    /// Root of the vendored checkout.
    pub root: PathBuf,
    /// Adapter strategy.
    pub adapter: Box<dyn Adapter>,
}

/// Test-suite adapter trait.
pub trait Adapter {
    /// Enumerate every test case the adapter knows about.
    fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>>;
    /// Run one case using the `rcc` binary at `rrcc_path`.
    fn run(&self, rrcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome>;
}

/// Aggregate result for one suite.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SuiteReport {
    /// Suite name.
    pub name: String,
    /// Per-case outcomes keyed by `TestCase::id`.
    pub cases: BTreeMap<String, Outcome>,
}

impl SuiteReport {
    /// Count outcomes of each kind.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for o in self.cases.values() {
            match o {
                Outcome::Pass => c.pass += 1,
                Outcome::Fail { .. } => c.fail += 1,
                Outcome::XFail { .. } => c.xfail += 1,
                Outcome::Skip { .. } => c.skip += 1,
            }
        }
        c
    }

    /// Pass rate as a ratio in the range `0.0..=1.0`.
    ///
    /// Expected failures count as passing for milestone KPI gates, while
    /// skips remain part of the denominator so missing adapter coverage is
    /// visible in the reported percentage.
    #[must_use]
    pub fn pass_rate(&self) -> f64 {
        let c = self.counts();
        let discovered = c.discovered();
        if discovered == 0 {
            0.0
        } else {
            f64::from(c.pass + c.xfail) / f64::from(discovered)
        }
    }
}

/// Outcome roll-up.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Counts {
    /// Passing tests.
    pub pass: u32,
    /// Failing tests.
    pub fail: u32,
    /// Expected-failure tests (count as pass for CI gating).
    pub xfail: u32,
    /// Skipped tests.
    pub skip: u32,
}

impl Counts {
    /// Total discovered cases represented by this count set.
    #[must_use]
    pub fn discovered(self) -> u32 {
        self.pass + self.fail + self.xfail + self.skip
    }
}

/// Top-level report containing every suite.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    /// Per-suite results.
    pub suites: Vec<SuiteReport>,
}

impl Report {
    /// Serialise to pretty JSON.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is always serialisable")
    }
}

/// Run every supplied suite against the `rcc` binary at `rrcc_path`.
///
/// Errors from discovery / execution are converted into `Fail` outcomes so a
/// broken adapter doesn't silently skip a whole suite.
pub fn run_suites(rrcc_path: &Path, suites: &[Suite]) -> Report {
    let mut report = Report::default();
    for suite in suites {
        let mut sr = SuiteReport { name: suite.name.clone(), ..Default::default() };
        let xfails = match xfail::load(&suite.root.join("xfail.toml")) {
            Ok(file) => file.xfail,
            Err(e) => {
                sr.cases.insert("<xfail>".into(), Outcome::Fail { reason: e.to_string() });
                report.suites.push(sr);
                continue;
            }
        };
        let mut xfail_map = BTreeMap::new();
        for entry in xfails {
            xfail_map.insert(entry.id, entry.reason);
        }
        let cases = match suite.adapter.discover(&suite.root) {
            Ok(c) => c,
            Err(e) => {
                sr.cases.insert("<discovery>".into(), Outcome::Fail { reason: e.to_string() });
                report.suites.push(sr);
                continue;
            }
        };
        let discovered_ids: BTreeSet<_> = cases.iter().map(|case| case.id.as_str()).collect();
        for (id, reason) in &xfail_map {
            if !discovered_ids.contains(id.as_str()) {
                sr.cases.insert(
                    id.clone(),
                    Outcome::Skip {
                        reason: format!("xfail entry did not match discovered case: {reason}"),
                    },
                );
            }
        }
        for case in cases {
            let outcome = suite
                .adapter
                .run(rrcc_path, &case)
                .unwrap_or_else(|e| Outcome::Fail { reason: e.to_string() });
            let outcome = match xfail_map.get(&case.id) {
                Some(reason) if !matches!(outcome, Outcome::Pass) => {
                    Outcome::XFail { reason: reason.clone() }
                }
                _ => outcome,
            };
            sr.cases.insert(case.id, outcome);
        }
        report.suites.push(sr);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedAdapter {
        outcome: Outcome,
    }

    impl Adapter for FixedAdapter {
        fn discover(&self, root: &Path) -> anyhow::Result<Vec<TestCase>> {
            Ok(vec![
                TestCase { id: "suite::pass".into(), path: root.join("pass.c") },
                TestCase { id: "suite::known-fail".into(), path: root.join("known-fail.c") },
            ])
        }

        fn run(&self, _rcc_path: &Path, case: &TestCase) -> anyhow::Result<Outcome> {
            if case.id.ends_with("pass") {
                Ok(Outcome::Pass)
            } else {
                Ok(self.outcome.clone())
            }
        }
    }

    #[test]
    fn run_suites_applies_xfail_entries_to_non_passing_outcomes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("xfail.toml"),
            "[[xfail]]\nid = \"suite::known-fail\"\nreason = \"future task\"\n",
        )
        .unwrap();

        let suite = Suite {
            name: "suite".into(),
            root: tmp.path().to_path_buf(),
            adapter: Box::new(FixedAdapter { outcome: Outcome::Fail { reason: "boom".into() } }),
        };

        let report = run_suites(Path::new("rcc"), &[suite]);
        let suite = &report.suites[0];
        assert_eq!(suite.counts().pass, 1);
        assert_eq!(suite.counts().xfail, 1);
        assert_eq!(suite.pass_rate(), 1.0);
    }

    #[test]
    fn run_suites_leaves_xpass_as_pass() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("xfail.toml"),
            "[[xfail]]\nid = \"suite::known-fail\"\nreason = \"stale xfail\"\n",
        )
        .unwrap();

        let suite = Suite {
            name: "suite".into(),
            root: tmp.path().to_path_buf(),
            adapter: Box::new(FixedAdapter { outcome: Outcome::Pass }),
        };

        let report = run_suites(Path::new("rcc"), &[suite]);
        let suite = &report.suites[0];
        assert_eq!(suite.counts().pass, 2);
        assert_eq!(suite.counts().xfail, 0);
        assert_eq!(suite.pass_rate(), 1.0);
    }
}
