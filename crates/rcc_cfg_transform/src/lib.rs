//! `rcc_cfg_transform`: CFG-level passes.
//!
//! Analogous to `rustc_mir_transform`. Most heavy optimisation is delegated
//! to LLVM; passes here are minimalist cleanup / simplification.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_cfg::Body;
use rcc_hir::TyCtxt;
use rcc_session::Session;

/// A single CFG-level pass.
pub trait Pass {
    /// Machine-readable pass name (used in `--emit=mir --unpretty=dumpMir`).
    fn name(&self) -> &'static str;
    /// Mutate one body.
    fn run(&mut self, session: &mut Session, tcx: &TyCtxt, body: &mut Body);
}

/// Run every pass in `passes` on `body` in order.
pub fn run_all(session: &mut Session, tcx: &TyCtxt, body: &mut Body, passes: &mut [Box<dyn Pass>]) {
    for p in passes.iter_mut() {
        p.run(session, tcx, body);
    }
}

/// No-op pass used as a placeholder / baseline.
#[derive(Default)]
pub struct Identity;

impl Pass for Identity {
    fn name(&self) -> &'static str {
        "identity"
    }
    fn run(&mut self, _session: &mut Session, _tcx: &TyCtxt, _body: &mut Body) {}
}
