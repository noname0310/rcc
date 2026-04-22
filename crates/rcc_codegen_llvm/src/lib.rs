//! `rcc_codegen_llvm`: lower CFG bodies to LLVM IR via `inkwell`.
//!
//! Analogous to `rustc_codegen_llvm`. The design contract exposed here is
//! stable even when the `llvm` feature is disabled, so dependent crates
//! (notably `rcc_driver`) can keep compiling without a local LLVM install.
//!
//! Activate the actual backend with `--features llvm` once LLVM 18 and
//! `llvm-config` are on `PATH`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_cfg::Body;
use rcc_data_structures::FxHashMap;
use rcc_hir::{DefId, HirCrate, Layout, Ty, TyCtxt, TyId};
use rcc_session::Session;

pub mod layout;

pub use layout::LayoutCx;

/// Error returned from codegen.
#[derive(Debug)]
pub enum CodegenError {
    /// The `llvm` feature is not enabled; rebuild with `--features llvm`.
    BackendDisabled,
    /// Internal error with a human-readable message.
    Internal(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::BackendDisabled => {
                write!(f, "rcc_codegen_llvm built without the `llvm` feature")
            }
            CodegenError::Internal(m) => write!(f, "internal codegen error: {m}"),
        }
    }
}

impl std::error::Error for CodegenError {}

/// Result of a codegen run. Backend-specific artifacts are stringified so the
/// driver / tests can assert against textual LLVM IR without depending on
/// `inkwell`.
#[derive(Debug)]
pub struct CodegenArtifact {
    /// Textual LLVM IR module (pretty-printed).
    pub ir_text: String,
}

/// Codegen entry point. Consumes HIR (for globals / layout info) and the
/// CFG body map produced by `rcc_cfg::build_bodies`.
pub fn codegen(
    _session: &mut Session,
    _tcx: &TyCtxt,
    _hir: &HirCrate,
    _bodies: &FxHashMap<DefId, Body>,
) -> Result<CodegenArtifact, CodegenError> {
    #[cfg(feature = "llvm")]
    {
        backend::codegen_impl(_session, _tcx, _hir, _bodies)
    }
    #[cfg(not(feature = "llvm"))]
    {
        Err(CodegenError::BackendDisabled)
    }
}

#[cfg(feature = "llvm")]
mod backend {
    //! The real inkwell-backed codegen. Filled in M3 follow-up.
    use super::*;

    pub(super) fn codegen_impl(
        _session: &mut Session,
        _tcx: &TyCtxt,
        _hir: &HirCrate,
        _bodies: &FxHashMap<DefId, Body>,
    ) -> Result<CodegenArtifact, CodegenError> {
        // TODO: build an `inkwell::context::Context`, translate every `Body`,
        // run `mem2reg`, and serialise the module.
        Ok(CodegenArtifact { ir_text: String::new() })
    }
}

/// Backend-agnostic view of per-type layout (useful for tests).
pub fn layout_of(tcx: &TyCtxt, ty: TyId) -> Layout {
    LayoutCx::new(tcx).layout_of(ty)
}

/// Re-export a trivial `Ty` pretty-printer used by tests. Not backend-specific.
pub fn pretty_ty(tcx: &TyCtxt, ty: TyId) -> String {
    match tcx.get(ty) {
        Ty::Void => "void".into(),
        Ty::Int { signed: true, rank } => format!("i{:?}", rank).to_lowercase(),
        Ty::Int { signed: false, rank } => format!("u{:?}", rank).to_lowercase(),
        Ty::Float(k) => format!("{:?}", k).to_lowercase(),
        Ty::Complex(k) => format!("complex {:?}", k).to_lowercase(),
        Ty::Ptr(q) => format!("ptr({:?})", q),
        Ty::Array { len, is_vla, .. } => format!("array[{:?} vla={}]", len, is_vla),
        Ty::Func { variadic, .. } => format!("func(variadic={})", variadic),
        Ty::Record(d) => format!("record#{}", d.0),
        Ty::Enum(d) => format!("enum#{}", d.0),
        Ty::Error => "<error>".into(),
    }
}
