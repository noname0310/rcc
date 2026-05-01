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

use std::collections::BTreeMap;
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
        let cases = match suite.adapter.discover(&suite.root) {
            Ok(c) => c,
            Err(e) => {
                sr.cases.insert("<discovery>".into(), Outcome::Fail { reason: e.to_string() });
                report.suites.push(sr);
                continue;
            }
        };
        for case in cases {
            let outcome = suite
                .adapter
                .run(rrcc_path, &case)
                .unwrap_or_else(|e| Outcome::Fail { reason: e.to_string() });
            sr.cases.insert(case.id, outcome);
        }
        report.suites.push(sr);
    }
    report
}
