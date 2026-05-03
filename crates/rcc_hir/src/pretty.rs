//! Stable HIR dump used by the driver `--emit=hir` mode.

use crate::HirCrate;

/// Dump a HIR crate in a deterministic, debug-oriented text format.
#[must_use]
pub fn dump_crate(hir: &HirCrate) -> String {
    format!("{hir:#?}\n")
}
