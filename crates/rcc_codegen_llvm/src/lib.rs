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
use rcc_data_structures::IndexVec;
use rcc_hir::{Def, DefId, HirCrate, Layout, LayoutError, Ty, TyCtxt, TyId};
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
pub mod backend {
    //! The real inkwell-backed codegen.

    use super::*;

    use inkwell::builder::Builder;
    use inkwell::context::Context;
    use inkwell::module::Module;
    use inkwell::targets::{TargetData, TargetTriple};

    /// First supported backend target: Linux x86-64 SysV.
    pub const BASELINE_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

    /// LLVM data layout for the first supported Linux x86-64 SysV target.
    pub const BASELINE_DATA_LAYOUT: &str =
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128";

    const FALLBACK_MODULE_NAME: &str = "rcc_module";

    /// Shared state for one LLVM module emission.
    pub struct CodegenCx<'a, 'ctx> {
        context: &'ctx Context,
        module: Module<'ctx>,
        builder: Builder<'ctx>,
        target_triple: String,
        data_layout: String,
        session: &'a mut Session,
        tcx: &'a TyCtxt,
        hir: &'a HirCrate,
        bodies: &'a FxHashMap<DefId, Body>,
    }

    impl<'a, 'ctx> CodegenCx<'a, 'ctx> {
        /// Build a codegen context with deterministic module and target metadata.
        pub fn new(
            context: &'ctx Context,
            session: &'a mut Session,
            tcx: &'a TyCtxt,
            hir: &'a HirCrate,
            bodies: &'a FxHashMap<DefId, Body>,
        ) -> Self {
            let module_name = module_name(session);
            let module = context.create_module(&module_name);
            let builder = context.create_builder();
            let target_triple = target_triple(session);
            let data_layout = BASELINE_DATA_LAYOUT.to_owned();

            module.set_triple(&TargetTriple::create(&target_triple));
            let target_data = TargetData::create(&data_layout);
            module.set_data_layout(&target_data.get_data_layout());

            Self { context, module, builder, target_triple, data_layout, session, tcx, hir, bodies }
        }

        /// Return the inkwell context backing this module.
        pub fn context(&self) -> &'ctx Context {
            self.context
        }

        /// Return the LLVM module being emitted.
        pub fn module(&self) -> &Module<'ctx> {
            &self.module
        }

        /// Return the instruction builder for later emission tasks.
        pub fn builder(&self) -> &Builder<'ctx> {
            &self.builder
        }

        /// Return the session used for diagnostics and options.
        pub fn session(&self) -> &Session {
            self.session
        }

        /// Return the typed-HIR context used for type queries.
        pub fn tcx(&self) -> &'a TyCtxt {
            self.tcx
        }

        /// Return the HIR crate being emitted.
        pub fn hir(&self) -> &'a HirCrate {
            self.hir
        }

        /// Return the CFG body map being emitted.
        pub fn bodies(&self) -> &'a FxHashMap<DefId, Body> {
            self.bodies
        }

        /// Return the LLVM target triple attached to this module.
        pub fn target_triple(&self) -> &str {
            &self.target_triple
        }

        /// Return the LLVM data layout attached to this module.
        pub fn data_layout(&self) -> &str {
            &self.data_layout
        }

        /// Verify the current LLVM module and convert verifier text into `CodegenError`.
        pub fn verify_module(&self) -> Result<(), CodegenError> {
            self.module.verify().map_err(|err| {
                CodegenError::Internal(format!("LLVM module verifier failed: {}", err.to_string()))
            })
        }

        /// Render the current LLVM module as textual LLVM IR.
        pub fn ir_text(&self) -> String {
            self.module.print_to_string().to_string()
        }
    }

    pub(super) fn codegen_impl(
        session: &mut Session,
        tcx: &TyCtxt,
        hir: &HirCrate,
        bodies: &FxHashMap<DefId, Body>,
    ) -> Result<CodegenArtifact, CodegenError> {
        let context = Context::create();
        let cx = CodegenCx::new(&context, session, tcx, hir, bodies);
        cx.verify_module()?;
        Ok(CodegenArtifact { ir_text: cx.ir_text() })
    }

    fn target_triple(session: &Session) -> String {
        session
            .opts
            .target
            .as_ref()
            .map(|target| target.0.clone())
            .unwrap_or_else(|| BASELINE_TARGET_TRIPLE.to_owned())
    }

    fn module_name(session: &Session) -> String {
        session
            .source_map
            .read()
            .ok()
            .and_then(|source_map| {
                source_map.files().next().map(|file| file.name.display().to_string())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| FALLBACK_MODULE_NAME.to_owned())
    }
}

/// Backend-agnostic view of per-type layout (useful for tests).
pub fn layout_of(tcx: &TyCtxt, ty: TyId) -> Result<Layout, LayoutError> {
    LayoutCx::new(tcx).layout_of(ty)
}

/// Backend-agnostic layout query with access to HIR definitions.
pub fn layout_of_with_defs(
    tcx: &TyCtxt,
    defs: &IndexVec<DefId, Def>,
    ty: TyId,
) -> Result<Layout, LayoutError> {
    LayoutCx::with_defs(tcx, defs).layout_of(ty)
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

#[cfg(test)]
mod tests {
    use rcc_hir::{Field, Qual, RecordKind};
    use rcc_session::Session;
    use rcc_span::{Symbol, DUMMY_SP};

    use super::*;

    #[test]
    fn codegen_layout_api_reuses_hir_layout_answers() {
        let mut tcx = TyCtxt::new();
        let arr = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
        assert_eq!(layout_of(&tcx, arr), LayoutCx::new(&tcx).layout_of(arr));
    }

    #[test]
    fn codegen_layout_with_defs_matches_hir_for_records() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let record = defs.push(Def {
            id: DefId(0),
            name: Symbol(1),
            span: DUMMY_SP,
            kind: rcc_hir::DefKind::Record {
                kind: RecordKind::Struct,
                layout: None,
                fields: vec![
                    Field {
                        name: None,
                        ty: tcx.char_,
                        quals: rcc_hir::ObjectQuals::none(),
                        offset: None,
                        bit_width: None,
                        span: DUMMY_SP,
                    },
                    Field {
                        name: None,
                        ty: tcx.int,
                        quals: rcc_hir::ObjectQuals::none(),
                        offset: None,
                        bit_width: None,
                        span: DUMMY_SP,
                    },
                ],
            },
        });
        defs[record].id = record;
        let record_ty = tcx.intern(Ty::Record(record));

        assert_eq!(
            layout_of_with_defs(&tcx, &defs, record_ty),
            LayoutCx::with_defs(&tcx, &defs).layout_of(record_ty)
        );
    }

    #[cfg(not(feature = "llvm"))]
    #[test]
    fn codegen_reports_backend_disabled_without_llvm_feature() {
        let (mut session, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();

        assert!(matches!(
            codegen(&mut session, &tcx, &hir, &bodies),
            Err(CodegenError::BackendDisabled)
        ));
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_backend_verifies_empty_module() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);

        assert_eq!(cx.target_triple(), backend::BASELINE_TARGET_TRIPLE);
        assert_eq!(cx.data_layout(), backend::BASELINE_DATA_LAYOUT);
        cx.verify_module().unwrap();
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_codegen_returns_module_header_target_and_layout() {
        let (mut session, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(artifact.ir_text.contains("; ModuleID = 'rcc_module'"));
        assert!(artifact.ir_text.contains("target triple = \"x86_64-unknown-linux-gnu\""));
        assert!(artifact.ir_text.contains("target datalayout = "));
    }
}
