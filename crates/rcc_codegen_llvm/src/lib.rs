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

pub use layout::{LayoutCx, BASELINE_POINTER_LAYOUT};

/// Error returned from codegen.
#[derive(Debug)]
pub enum CodegenError {
    /// The `llvm` feature is not enabled; rebuild with `--features llvm`.
    BackendDisabled,
    /// HIR type lowering failed for the given type id.
    TypeLowering {
        /// Original HIR type that failed to lower.
        ty: TyId,
        /// Human-readable reason.
        reason: String,
    },
    /// Internal error with a human-readable message.
    Internal(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::BackendDisabled => {
                write!(f, "rcc_codegen_llvm built without the `llvm` feature")
            }
            CodegenError::TypeLowering { ty, reason } => {
                write!(f, "failed to lower HIR type {ty:?} to LLVM: {reason}")
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
    use inkwell::module::Linkage as LlvmLinkage;
    use inkwell::module::Module;
    use inkwell::targets::{TargetData, TargetTriple};
    use inkwell::types::{
        AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType,
    };
    use inkwell::values::{FunctionValue, GlobalValue};
    use inkwell::AddressSpace;
    use rcc_hir::{DefKind, FloatKind, IntRank, Linkage as HirLinkage, Qual, RecordKind};

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
        functions: FxHashMap<DefId, FunctionValue<'ctx>>,
        globals: FxHashMap<DefId, GlobalValue<'ctx>>,
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

            Self {
                context,
                module,
                builder,
                target_triple,
                data_layout,
                session,
                tcx,
                hir,
                bodies,
                functions: FxHashMap::default(),
                globals: FxHashMap::default(),
            }
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

        /// Build a type-lowering helper sharing this module's context and HIR.
        pub fn type_cx(&self) -> TypeCx<'a, 'ctx> {
            TypeCx::new(self.context, self.tcx, self.hir)
        }

        /// Declare every HIR function and file-scope object in this LLVM module.
        pub fn declare_all(&mut self) -> Result<(), CodegenError> {
            let defs = self.hir.defs.iter_enumerated().map(|(id, _)| id).collect::<Vec<_>>();
            for def in defs {
                match self.hir.defs[def].kind {
                    DefKind::Function { .. } => {
                        self.declare_function(def)?;
                    }
                    DefKind::Global { .. } => {
                        self.declare_global(def)?;
                    }
                    DefKind::Typedef(_)
                    | DefKind::Record { .. }
                    | DefKind::Enum { .. }
                    | DefKind::Enumerator { .. } => {}
                }
            }
            Ok(())
        }

        /// Declare one HIR function and return the reused or newly-created LLVM value.
        pub fn declare_function(
            &mut self,
            def: DefId,
        ) -> Result<FunctionValue<'ctx>, CodegenError> {
            if let Some(&function) = self.functions.get(&def) {
                return Ok(function);
            }

            let (name, ty, linkage) = {
                let def_data = self.hir.defs.get(def).ok_or_else(|| {
                    CodegenError::Internal(format!("function definition {def:?} is missing"))
                })?;
                let DefKind::Function {
                    ty, has_body, is_static, is_inline, is_extern_inline, ..
                } = &def_data.kind
                else {
                    return Err(CodegenError::Internal(format!(
                        "definition {def:?} is not a function"
                    )));
                };
                (
                    self.def_name(def_data),
                    *ty,
                    function_linkage(*has_body, *is_static, *is_inline, *is_extern_inline),
                )
            };
            let fn_ty = self.type_cx().fn_type_of(ty)?;
            let function = self
                .module
                .get_function(&name)
                .unwrap_or_else(|| self.module.add_function(&name, fn_ty, Some(linkage)));
            function.set_linkage(linkage);
            self.functions.insert(def, function);
            Ok(function)
        }

        /// Declare one HIR file-scope object and return the reused or new LLVM global.
        pub fn declare_global(&mut self, def: DefId) -> Result<GlobalValue<'ctx>, CodegenError> {
            if let Some(&global) = self.globals.get(&def) {
                return Ok(global);
            }

            let (name, ty, linkage, needs_zero_initializer) = {
                let def_data = self.hir.defs.get(def).ok_or_else(|| {
                    CodegenError::Internal(format!("global definition {def:?} is missing"))
                })?;
                let DefKind::Global { ty, linkage, init, .. } = &def_data.kind else {
                    return Err(CodegenError::Internal(format!(
                        "definition {def:?} is not a global"
                    )));
                };
                let llvm_linkage = global_linkage(*linkage);
                (
                    self.def_name(def_data),
                    *ty,
                    llvm_linkage,
                    init.is_some() || llvm_linkage != LlvmLinkage::External,
                )
            };
            let global_ty = self.type_cx().basic_type_of(ty)?;
            let global = self
                .module
                .get_global(&name)
                .unwrap_or_else(|| self.module.add_global(global_ty, None, &name));
            global.set_linkage(linkage);
            if needs_zero_initializer && global.get_initializer().is_none() {
                let zero = global_ty.const_zero();
                global.set_initializer(&zero);
            }
            self.globals.insert(def, global);
            Ok(global)
        }

        /// Return the LLVM function previously declared for a HIR definition.
        pub fn function_decl(&self, def: DefId) -> Option<FunctionValue<'ctx>> {
            self.functions.get(&def).copied()
        }

        /// Return the LLVM global previously declared for a HIR definition.
        pub fn global_decl(&self, def: DefId) -> Option<GlobalValue<'ctx>> {
            self.globals.get(&def).copied()
        }

        /// Return all declared functions keyed by HIR definition id.
        pub fn function_decls(&self) -> &FxHashMap<DefId, FunctionValue<'ctx>> {
            &self.functions
        }

        /// Return all declared globals keyed by HIR definition id.
        pub fn global_decls(&self) -> &FxHashMap<DefId, GlobalValue<'ctx>> {
            &self.globals
        }

        fn def_name(&self, def: &Def) -> String {
            self.session.interner.get(def.name).to_owned()
        }
    }

    /// Recursive `TyId` to LLVM type lowering service for one LLVM context.
    pub struct TypeCx<'a, 'ctx> {
        context: &'ctx Context,
        tcx: &'a TyCtxt,
        hir: &'a HirCrate,
        cache: FxHashMap<TyId, AnyTypeEnum<'ctx>>,
    }

    impl<'a, 'ctx> TypeCx<'a, 'ctx> {
        /// Build a fresh type-lowering context.
        pub fn new(context: &'ctx Context, tcx: &'a TyCtxt, hir: &'a HirCrate) -> Self {
            Self { context, tcx, hir, cache: FxHashMap::default() }
        }

        /// Lower any HIR type representable as an LLVM type.
        pub fn type_of(&mut self, ty: TyId) -> Result<AnyTypeEnum<'ctx>, CodegenError> {
            if let Some(&llvm_ty) = self.cache.get(&ty) {
                return Ok(llvm_ty);
            }

            let llvm_ty = match self.tcx.get(ty) {
                Ty::Void => self.context.void_type().into(),
                Ty::Int { rank, .. } => self.int_type(*rank).into(),
                Ty::Float(kind) => self.float_type(*kind).into(),
                Ty::Complex(kind) => self.complex_type(*kind).into(),
                Ty::Ptr(_) => self.ptr_type().into(),
                Ty::Array { elem, len: Some(len), is_vla: false } => {
                    let elem_ty = self.basic_type_of_qual(*elem)?;
                    let len = array_len(*len, ty)?;
                    elem_ty.array_type(len).into()
                }
                Ty::Array { is_vla: true, .. } => {
                    return self.type_error(ty, "VLA array objects are runtime-sized");
                }
                Ty::Array { len: None, .. } => {
                    return self.type_error(ty, "incomplete arrays have no LLVM object type");
                }
                Ty::Func { .. } => self.fn_type_of(ty)?.into(),
                Ty::Record(def) => self.record_type(ty, *def)?.into(),
                Ty::Enum(def) => basic_type_as_any(self.enum_type(ty, *def)?),
                Ty::Error => return self.type_error(ty, "error type cannot be lowered"),
            };

            self.cache.insert(ty, llvm_ty);
            Ok(llvm_ty)
        }

        /// Lower an object/scalar type. `void` and functions are rejected.
        pub fn basic_type_of(&mut self, ty: TyId) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
            match self.type_of(ty)? {
                AnyTypeEnum::ArrayType(t) => Ok(t.into()),
                AnyTypeEnum::FloatType(t) => Ok(t.into()),
                AnyTypeEnum::IntType(t) => Ok(t.into()),
                AnyTypeEnum::PointerType(t) => Ok(t.into()),
                AnyTypeEnum::StructType(t) => Ok(t.into()),
                AnyTypeEnum::VectorType(t) => Ok(t.into()),
                AnyTypeEnum::ScalableVectorType(t) => Ok(t.into()),
                AnyTypeEnum::FunctionType(_) => self.type_error(ty, "function is not a basic type"),
                AnyTypeEnum::VoidType(_) => self.type_error(ty, "void is not a basic type"),
            }
        }

        /// Lower a C function type to an LLVM function type.
        pub fn fn_type_of(&mut self, ty: TyId) -> Result<FunctionType<'ctx>, CodegenError> {
            let (ret, params, variadic) = match self.tcx.get(ty) {
                Ty::Func { ret, params, variadic, .. } => (*ret, params.clone(), *variadic),
                _ => return self.type_error(ty, "not a function type"),
            };

            let params: Vec<BasicMetadataTypeEnum<'ctx>> = params
                .into_iter()
                .map(|param| self.basic_type_of(param).map(Into::into))
                .collect::<Result<_, _>>()?;

            match self.tcx.get(ret) {
                Ty::Void => Ok(self.context.void_type().fn_type(&params, variadic)),
                _ => Ok(self.basic_type_of(ret)?.fn_type(&params, variadic)),
            }
        }

        /// Number of cached type entries, exposed for reuse tests.
        pub fn cached_type_count(&self) -> usize {
            self.cache.len()
        }

        fn int_type(&self, rank: IntRank) -> inkwell::types::IntType<'ctx> {
            match rank {
                IntRank::Bool => self.context.bool_type(),
                IntRank::Char => self.context.i8_type(),
                IntRank::Short => self.context.i16_type(),
                IntRank::Int => self.context.i32_type(),
                IntRank::Long | IntRank::LongLong => self.context.i64_type(),
            }
        }

        fn float_type(&self, kind: FloatKind) -> inkwell::types::FloatType<'ctx> {
            match kind {
                FloatKind::F32 => self.context.f32_type(),
                FloatKind::F64 => self.context.f64_type(),
                FloatKind::F80 => self.context.x86_f80_type(),
            }
        }

        fn complex_type(&self, kind: FloatKind) -> inkwell::types::StructType<'ctx> {
            let elem: BasicTypeEnum<'ctx> = self.float_type(kind).into();
            self.context.struct_type(&[elem, elem], false)
        }

        fn ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
            self.context.ptr_type(AddressSpace::default())
        }

        fn basic_type_of_qual(&mut self, qual: Qual) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
            self.basic_type_of(qual.ty)
        }

        fn record_type(
            &mut self,
            ty: TyId,
            def: DefId,
        ) -> Result<inkwell::types::StructType<'ctx>, CodegenError> {
            if let Some(AnyTypeEnum::StructType(existing)) = self.cache.get(&ty).copied() {
                return Ok(existing);
            }

            let (kind, field_tys) = {
                let def_data = self.hir.defs.get(def).ok_or_else(|| {
                    type_error(ty, format!("record definition {def:?} is missing"))
                })?;
                let DefKind::Record { kind, fields, .. } = &def_data.kind else {
                    return self
                        .type_error(ty, "record type does not reference a record definition");
                };
                (*kind, fields.iter().map(|field| field.ty).collect::<Vec<_>>())
            };

            let record = self.context.opaque_struct_type(&format!("rcc.record.{}", def.0));
            self.cache.insert(ty, record.into());

            let field_types = match kind {
                RecordKind::Struct => field_tys
                    .into_iter()
                    .map(|field_ty| self.basic_type_of(field_ty))
                    .collect::<Result<Vec<_>, _>>()?,
                RecordKind::Union => {
                    let layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(ty)
                        .map_err(|err| type_error(ty, err.to_string()))?;
                    vec![self.context.i8_type().array_type(array_len(layout.size, ty)?).into()]
                }
            };
            record.set_body(&field_types, false);
            Ok(record)
        }

        fn enum_type(&mut self, ty: TyId, def: DefId) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
            let repr = self
                .hir
                .defs
                .get(def)
                .and_then(|def_data| match &def_data.kind {
                    DefKind::Enum { repr, .. } | DefKind::Enumerator { ty: repr, .. } => {
                        Some(*repr)
                    }
                    _ => None,
                })
                .unwrap_or(self.tcx.int);
            self.basic_type_of(repr)
                .map_err(|_| type_error(ty, format!("enum representation {repr:?} is invalid")))
        }

        fn type_error<T>(&self, ty: TyId, reason: impl Into<String>) -> Result<T, CodegenError> {
            Err(type_error(ty, reason))
        }
    }

    fn array_len(len: u64, ty: TyId) -> Result<u32, CodegenError> {
        u32::try_from(len)
            .map_err(|_| type_error(ty, format!("array length {len} exceeds LLVM u32 limit")))
    }

    fn type_error(ty: TyId, reason: impl Into<String>) -> CodegenError {
        CodegenError::TypeLowering { ty, reason: reason.into() }
    }

    fn basic_type_as_any<'ctx>(ty: BasicTypeEnum<'ctx>) -> AnyTypeEnum<'ctx> {
        match ty {
            BasicTypeEnum::ArrayType(ty) => ty.into(),
            BasicTypeEnum::FloatType(ty) => ty.into(),
            BasicTypeEnum::IntType(ty) => ty.into(),
            BasicTypeEnum::PointerType(ty) => ty.into(),
            BasicTypeEnum::StructType(ty) => ty.into(),
            BasicTypeEnum::VectorType(ty) => ty.into(),
            BasicTypeEnum::ScalableVectorType(ty) => ty.into(),
        }
    }

    fn function_linkage(
        has_body: bool,
        is_static: bool,
        is_inline: bool,
        is_extern_inline: bool,
    ) -> LlvmLinkage {
        match (has_body, is_static, is_inline, is_extern_inline) {
            (_, true, _, _) => LlvmLinkage::Internal,
            (true, false, true, false) => LlvmLinkage::AvailableExternally,
            (_, false, _, _) => LlvmLinkage::External,
        }
    }

    fn global_linkage(linkage: HirLinkage) -> LlvmLinkage {
        match linkage {
            HirLinkage::Internal => LlvmLinkage::Internal,
            HirLinkage::External | HirLinkage::None => LlvmLinkage::External,
        }
    }

    pub(super) fn codegen_impl(
        session: &mut Session,
        tcx: &TyCtxt,
        hir: &HirCrate,
        bodies: &FxHashMap<DefId, Body>,
    ) -> Result<CodegenArtifact, CodegenError> {
        let context = Context::create();
        let mut cx = CodegenCx::new(&context, session, tcx, hir, bodies);
        cx.declare_all()?;
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
    use rcc_hir::{DefKind, Field, IntRank, Qual, RecordKind};
    #[cfg(feature = "llvm")]
    use rcc_hir::{Linkage, ObjectQuals};
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

    #[test]
    fn scalar_layout_table_matches_lp64_sysv_baseline() {
        let mut tcx = TyCtxt::new();
        let ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let cases = [
            ("_Bool", tcx.bool_, Layout { size: 1, align: 1 }),
            ("char", tcx.char_, Layout { size: 1, align: 1 }),
            ("signed char", tcx.schar, Layout { size: 1, align: 1 }),
            ("unsigned char", tcx.uchar, Layout { size: 1, align: 1 }),
            ("short", tcx.short, Layout { size: 2, align: 2 }),
            ("unsigned short", tcx.ushort, Layout { size: 2, align: 2 }),
            ("int", tcx.int, Layout { size: 4, align: 4 }),
            ("unsigned int", tcx.uint, Layout { size: 4, align: 4 }),
            ("long", tcx.long, Layout { size: 8, align: 8 }),
            ("unsigned long", tcx.ulong, Layout { size: 8, align: 8 }),
            ("long long", tcx.long_long, Layout { size: 8, align: 8 }),
            ("unsigned long long", tcx.ulong_long, Layout { size: 8, align: 8 }),
            ("float", tcx.float, Layout { size: 4, align: 4 }),
            ("double", tcx.double, Layout { size: 8, align: 8 }),
            ("long double", tcx.long_double, Layout { size: 16, align: 16 }),
            ("void *", ptr, BASELINE_POINTER_LAYOUT),
        ];
        let layouts = LayoutCx::new(&tcx);

        for (name, ty, expected) in cases {
            assert_eq!(layout_of(&tcx, ty), Ok(expected), "{name}");
            assert_eq!(layouts.layout_of(ty), Ok(expected), "{name}");
        }
    }

    #[test]
    fn signed_and_unsigned_integer_ranks_share_layouts() {
        let tcx = TyCtxt::new();
        let cases = [
            (IntRank::Char, tcx.schar, tcx.uchar),
            (IntRank::Short, tcx.short, tcx.ushort),
            (IntRank::Int, tcx.int, tcx.uint),
            (IntRank::Long, tcx.long, tcx.ulong),
            (IntRank::LongLong, tcx.long_long, tcx.ulong_long),
        ];

        for (rank, signed, unsigned) in cases {
            assert_eq!(layout_of(&tcx, signed), layout_of(&tcx, unsigned), "{rank:?}");
        }
    }

    #[test]
    fn enum_layout_follows_resolved_representation_or_int_fallback() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let enum_def = defs.push(Def {
            id: DefId(0),
            name: Symbol(2),
            span: DUMMY_SP,
            kind: DefKind::Enum { repr: tcx.ulong, variants: Vec::new() },
        });
        defs[enum_def].id = enum_def;
        let enum_ty = tcx.intern(Ty::Enum(enum_def));

        assert_eq!(
            LayoutCx::with_defs(&tcx, &defs).layout_of(enum_ty),
            Ok(Layout { size: 8, align: 8 })
        );
        assert_eq!(LayoutCx::new(&tcx).layout_of(enum_ty), Ok(Layout { size: 4, align: 4 }));
    }

    fn field(ty: TyId) -> Field {
        Field {
            name: None,
            ty,
            quals: rcc_hir::ObjectQuals::none(),
            offset: None,
            bit_width: None,
            span: DUMMY_SP,
        }
    }

    fn bitfield(ty: TyId, width: u32) -> Field {
        Field { bit_width: Some(width), ..field(ty) }
    }

    fn record_def(defs: &mut IndexVec<DefId, Def>, kind: RecordKind, fields: Vec<Field>) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name: Symbol(3),
            span: DUMMY_SP,
            kind: DefKind::Record { kind, layout: None, fields },
        });
        defs[id].id = id;
        id
    }

    #[cfg(feature = "llvm")]
    #[derive(Copy, Clone, Debug, Default)]
    struct FunctionDefOptions {
        has_body: bool,
        is_static: bool,
        is_inline: bool,
        is_extern_inline: bool,
        variadic: bool,
    }

    #[cfg(feature = "llvm")]
    fn function_def(
        defs: &mut IndexVec<DefId, Def>,
        name: Symbol,
        ty: TyId,
        opts: FunctionDefOptions,
    ) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name,
            span: DUMMY_SP,
            kind: DefKind::Function {
                ty,
                has_body: opts.has_body,
                is_static: opts.is_static,
                is_inline: opts.is_inline,
                is_extern_inline: opts.is_extern_inline,
                variadic: opts.variadic,
            },
        });
        defs[id].id = id;
        id
    }

    #[cfg(feature = "llvm")]
    fn global_def(
        defs: &mut IndexVec<DefId, Def>,
        name: Symbol,
        ty: TyId,
        linkage: Linkage,
    ) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name,
            span: DUMMY_SP,
            kind: DefKind::Global { ty, quals: ObjectQuals::none(), linkage, init: None },
        });
        defs[id].id = id;
        id
    }

    #[cfg(feature = "llvm")]
    fn hir_with_defs(defs: IndexVec<DefId, Def>) -> HirCrate {
        HirCrate { defs, ..HirCrate::default() }
    }

    #[test]
    fn record_layout_reports_nested_offsets_and_padding() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let inner =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.char_), field(tcx.int)]);
        let inner_ty = tcx.intern(Ty::Record(inner));
        let outer = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![field(tcx.char_), field(inner_ty), field(tcx.long)],
        );
        let outer_ty = tcx.intern(Ty::Record(outer));

        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(outer_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 24, align: 8 });
        assert_eq!(layout.fields.iter().map(|field| field.offset).collect::<Vec<_>>(), [0, 4, 16]);
    }

    #[test]
    fn record_layout_reports_union_offsets_and_max_size() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(3), is_vla: false });
        let union = record_def(
            &mut defs,
            RecordKind::Union,
            vec![field(tcx.char_), field(tcx.long), field(arr)],
        );
        let union_ty = tcx.intern(Ty::Record(union));

        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(union_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 8, align: 8 });
        assert_eq!(layout.fields.iter().map(|field| field.offset).collect::<Vec<_>>(), [0, 0, 0]);
    }

    #[test]
    fn record_layout_ignores_flexible_array_trailing_size() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let flex =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.double), len: None, is_vla: false });
        let record = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(flex)]);
        let record_ty = tcx.intern(Ty::Record(record));

        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(record_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 8, align: 8 });
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[1].offset, 8);
        assert_eq!(layout.fields[1].storage_size, 0);
    }

    #[test]
    fn record_layout_reports_bitfield_pack_metadata() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let record = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![
                bitfield(tcx.uint, 3),
                bitfield(tcx.uint, 5),
                bitfield(tcx.uint, 0),
                bitfield(tcx.uint, 6),
            ],
        );
        let record_ty = tcx.intern(Ty::Record(record));

        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(record_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 8, align: 4 });
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[0].bit_offset, Some(0));
        assert_eq!(layout.fields[0].bit_width, Some(3));
        assert_eq!(layout.fields[1].offset, 0);
        assert_eq!(layout.fields[1].bit_offset, Some(3));
        assert_eq!(layout.fields[1].bit_width, Some(5));
        assert_eq!(layout.fields[2].offset, 4);
        assert_eq!(layout.fields[2].bit_width, Some(0));
        assert_eq!(layout.fields[3].offset, 4);
        assert_eq!(layout.fields[3].bit_offset, Some(0));
        assert_eq!(layout.fields[3].storage_size, 4);
    }

    #[test]
    fn array_layout_reports_fixed_scalar_size_and_align() {
        let mut tcx = TyCtxt::new();
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.short), len: Some(7), is_vla: false });

        let layout = LayoutCx::new(&tcx).array_layout_of(arr).unwrap();

        assert_eq!(layout.static_size, Some(14));
        assert_eq!(layout.align, 2);
        assert_eq!(layout.elem, Layout { size: 2, align: 2 });
        assert_eq!(layout_of(&tcx, arr), Ok(Layout { size: 14, align: 2 }));
    }

    #[test]
    fn array_layout_reports_record_element_stride() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let record =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.char_), field(tcx.long)]);
        let record_ty = tcx.intern(Ty::Record(record));
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(record_ty), len: Some(3), is_vla: false });

        let layout = LayoutCx::with_defs(&tcx, &defs).array_layout_of(arr).unwrap();

        assert_eq!(layout.elem, Layout { size: 16, align: 8 });
        assert_eq!(layout.static_size, Some(48));
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn array_layout_rejects_incomplete_non_fam_arrays() {
        let mut tcx = TyCtxt::new();
        let arr = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: false });

        assert!(matches!(
            LayoutCx::new(&tcx).array_layout_of(arr),
            Err(LayoutError::Unsized { reason: "incomplete array has no object size", .. })
        ));
    }

    #[test]
    fn array_layout_checks_fixed_array_size_overflow() {
        let mut tcx = TyCtxt::new();
        let len = u64::MAX / 8 + 1;
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.long), len: Some(len), is_vla: false });

        assert!(matches!(
            LayoutCx::new(&tcx).array_layout_of(arr),
            Err(LayoutError::SizeOverflow { ty }) if ty == arr
        ));
    }

    #[test]
    fn array_layout_vla_sentinel_reports_alignment_without_static_size() {
        let mut tcx = TyCtxt::new();
        let vla = tcx.intern(Ty::Array { elem: Qual::plain(tcx.double), len: None, is_vla: true });

        let layout = LayoutCx::new(&tcx).array_layout_of(vla).unwrap();

        assert_eq!(layout.elem, Layout { size: 8, align: 8 });
        assert_eq!(layout.align, 8);
        assert_eq!(layout.static_size, None);
        assert!(layout.is_vla);
        assert!(matches!(
            layout_of(&tcx, vla),
            Err(LayoutError::Unsized { reason: "VLA size is runtime-dependent", .. })
        ));
    }

    #[test]
    fn layoutcx_rejects_error_type() {
        let tcx = TyCtxt::new();

        assert!(matches!(
            layout_of(&tcx, tcx.error),
            Err(LayoutError::Unsized { reason: "error type has no layout", .. })
        ));
    }

    #[test]
    fn pointer_layout_matches_module_data_layout_baseline() {
        assert_eq!(BASELINE_POINTER_LAYOUT, Layout { size: 8, align: 8 });

        #[cfg(feature = "llvm")]
        assert!(backend::BASELINE_DATA_LAYOUT.contains("-p270:32:32-p271:32:32-p272:64:64"));
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

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_declarations_reuse_later_defined_function_symbol() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let fn_ty =
            tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
        let caller_name = session.interner.intern("caller");
        let callee_name = session.interner.intern("callee");
        let mut defs = IndexVec::new();
        let caller = function_def(
            &mut defs,
            caller_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let callee = function_def(
            &mut defs,
            callee_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);

        let first = cx.declare_function(callee).unwrap();
        cx.declare_all().unwrap();
        let second = cx.declare_function(callee).unwrap();

        assert_eq!(first, second);
        assert_eq!(cx.function_decl(callee), Some(first));
        assert!(cx.function_decl(caller).is_some());
        assert_eq!(cx.module().get_function("callee"), Some(first));
        assert_eq!(cx.ir_text().matches("@callee").count(), 1);
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_declarations_select_static_and_external_linkage() {
        use inkwell::module::Linkage as LlvmLinkage;

        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let fn_ty =
            tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
        let static_fn_name = session.interner.intern("static_fn");
        let external_fn_name = session.interner.intern("external_fn");
        let inline_fn_name = session.interner.intern("inline_fn");
        let extern_inline_fn_name = session.interner.intern("extern_inline_fn");
        let static_global_name = session.interner.intern("static_global");
        let external_global_name = session.interner.intern("external_global");
        let mut defs = IndexVec::new();
        let static_fn = function_def(
            &mut defs,
            static_fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, is_static: true, ..FunctionDefOptions::default() },
        );
        let external_fn =
            function_def(&mut defs, external_fn_name, fn_ty, FunctionDefOptions::default());
        let inline_fn = function_def(
            &mut defs,
            inline_fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, is_inline: true, ..FunctionDefOptions::default() },
        );
        let extern_inline_fn = function_def(
            &mut defs,
            extern_inline_fn_name,
            fn_ty,
            FunctionDefOptions {
                has_body: true,
                is_inline: true,
                is_extern_inline: true,
                ..FunctionDefOptions::default()
            },
        );
        let static_global = global_def(&mut defs, static_global_name, tcx.int, Linkage::Internal);
        let external_global =
            global_def(&mut defs, external_global_name, tcx.int, Linkage::External);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);

        cx.declare_all().unwrap();

        assert_eq!(cx.function_decl(static_fn).unwrap().get_linkage(), LlvmLinkage::Internal);
        assert_eq!(cx.function_decl(external_fn).unwrap().get_linkage(), LlvmLinkage::External);
        assert_eq!(
            cx.function_decl(inline_fn).unwrap().get_linkage(),
            LlvmLinkage::AvailableExternally
        );
        assert_eq!(
            cx.function_decl(extern_inline_fn).unwrap().get_linkage(),
            LlvmLinkage::External
        );
        assert_eq!(cx.global_decl(static_global).unwrap().get_linkage(), LlvmLinkage::Internal);
        assert_eq!(cx.global_decl(external_global).unwrap().get_linkage(), LlvmLinkage::External);
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_typecx_reuses_lowered_types() {
        let context = inkwell::context::Context::create();
        let mut tcx = TyCtxt::new();
        let ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let arr = tcx.intern(Ty::Array { elem: Qual::plain(ptr), len: Some(4), is_vla: false });
        let hir = HirCrate::default();
        let mut types = backend::TypeCx::new(&context, &tcx, &hir);

        let first = types.basic_type_of(arr).unwrap();
        let cached = types.cached_type_count();
        let second = types.basic_type_of(arr).unwrap();

        assert_eq!(first, second);
        assert_eq!(cached, types.cached_type_count());
        assert_eq!(first.print_to_string().to_string(), "[4 x ptr]");
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_typecx_terminates_recursive_record_through_pointer() {
        let context = inkwell::context::Context::create();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let node = record_def(&mut defs, RecordKind::Struct, Vec::new());
        let node_ty = tcx.intern(Ty::Record(node));
        let node_ptr = tcx.intern(Ty::Ptr(Qual::plain(node_ty)));
        defs[node].kind = DefKind::Record {
            kind: RecordKind::Struct,
            layout: None,
            fields: vec![field(tcx.int), field(node_ptr)],
        };
        let hir = hir_with_defs(defs);
        let mut types = backend::TypeCx::new(&context, &tcx, &hir);

        let lowered = types.basic_type_of(node_ty).unwrap();

        assert_eq!(lowered.print_to_string().to_string(), "%rcc.record.0 = type { i32, ptr }");
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_typecx_lowers_function_declarations_without_body_codegen() {
        let context = inkwell::context::Context::create();
        let mut tcx = TyCtxt::new();
        let ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let func = tcx.intern(Ty::Func {
            ret: tcx.int,
            params: vec![tcx.double, ptr],
            variadic: true,
            proto: true,
        });
        let hir = HirCrate::default();
        let mut types = backend::TypeCx::new(&context, &tcx, &hir);

        let fn_ty = types.fn_type_of(func).unwrap();

        assert_eq!(fn_ty.print_to_string().to_string(), "i32 (double, ptr, ...)");
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn llvm_typecx_reports_original_ty_for_unlowerable_type() {
        let context = inkwell::context::Context::create();
        let mut tcx = TyCtxt::new();
        let vla = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: true });
        let hir = HirCrate::default();
        let mut types = backend::TypeCx::new(&context, &tcx, &hir);

        assert!(matches!(
            types.type_of(vla),
            Err(CodegenError::TypeLowering { ty, .. }) if ty == vla
        ));
    }
}
