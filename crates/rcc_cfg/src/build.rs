//! HIR -> CFG lowering. Produces one `Body` per function.

use rcc_data_structures::FxHashMap;
use rcc_hir::{DefId, HirCrate, TyCtxt};
use rcc_session::Session;

use crate::Body;

/// Build CFG bodies for every function in `hir`. Returns a `DefId -> Body` map.
///
/// M3 scope: interface only.
pub fn build_bodies(
    _session: &mut Session,
    _tcx: &TyCtxt,
    _hir: &HirCrate,
) -> FxHashMap<DefId, Body> {
    FxHashMap::default()
}
