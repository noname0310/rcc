//! Concrete suite adapters. Each one wraps the discovery + execution
//! strategy of a vendored suite. Implementations are stubs pending M0.5
//! follow-up; interfaces are frozen so suites can be added independently.

use std::path::Path;

use crate::{Adapter, Outcome, TestCase};

/// `c-testsuite` adapter. Enumerates `tests/single-exec/*.c` and compares
/// `<name>.expected` stdout vs `rcc`-produced stdout.
pub struct CTestSuiteAdapter;

impl Adapter for CTestSuiteAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "c-testsuite adapter not yet implemented".into() })
    }
}

/// `chibicc` adapter. Runs the `chibicc/test/*.c` files the same way chibicc's
/// Makefile does: compile, link with a tiny runtime, run, check exit code.
pub struct ChibiccAdapter;

impl Adapter for ChibiccAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "chibicc adapter not yet implemented".into() })
    }
}

/// `gcc-torture` adapter (GPL-licensed; gated by `--include-gpl`).
pub struct GccTortureAdapter;

impl Adapter for GccTortureAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "gcc-torture adapter not yet implemented".into() })
    }
}

/// `tcc-tests2` adapter (LGPL).
pub struct TccTests2Adapter;

impl Adapter for TccTests2Adapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "tcc-tests2 adapter not yet implemented".into() })
    }
}

/// `llvm-test-suite` SingleSource adapter.
pub struct LlvmTestSuiteAdapter;

impl Adapter for LlvmTestSuiteAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "llvm-test-suite adapter not yet implemented".into() })
    }
}

/// Differential-fuzzing driver built on top of `csmith`. Not a suite proper —
/// it generates fresh programs each run.
pub struct CsmithDifferentialAdapter;

impl Adapter for CsmithDifferentialAdapter {
    fn discover(&self, _root: &Path) -> anyhow::Result<Vec<TestCase>> {
        Ok(Vec::new())
    }
    fn run(&self, _rcc: &Path, _case: &TestCase) -> anyhow::Result<Outcome> {
        Ok(Outcome::Skip { reason: "csmith differential not yet implemented".into() })
    }
}
