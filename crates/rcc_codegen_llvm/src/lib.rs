//! `rcc_codegen_llvm`: lower CFG bodies to LLVM IR via `inkwell`.
//!
//! Analogous to `rustc_codegen_llvm`. The design contract exposed here is
//! stable even when the `llvm` feature is disabled, so dependent crates
//! (notably `rcc_driver`) can keep compiling without a local LLVM install.
//!
//! Activate the actual backend with `--features llvm` once LLVM 18 and
//! `llvm-config` are on `PATH`.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use rcc_cfg::Body;
use rcc_data_structures::FxHashMap;
use rcc_data_structures::IndexVec;
use rcc_hir::{Def, DefId, DefKind, FloatKind, HirCrate, Layout, LayoutError, Ty, TyCtxt, TyId};
#[cfg(feature = "llvm")]
use rcc_hir::{GlobalInit, GlobalInitDesignator, GlobalInitEntry, GlobalInitValue};
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
    /// Target assembly, present when `EmitKind::Asm` was requested.
    pub assembly_text: Option<String>,
    /// Native object bytes, present for final linking or `EmitKind::Obj`.
    pub object_bytes: Option<Vec<u8>>,
}

/// Final SysV x86-64 ABI class assigned to a parameter/return eightbyte.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AbiClass {
    /// Empty or padding-only eightbyte before cleanup.
    #[default]
    NoClass,
    /// General-purpose integer register class.
    Integer,
    /// SSE/vector register class.
    Sse,
    /// Upper half of a vector register.
    SseUp,
    /// x87 long-double payload class.
    X87,
    /// x87 long-double exponent/padding class.
    X87Up,
    /// Complex long-double class.
    ComplexX87,
    /// Stack memory class.
    Memory,
}

/// ABI lowering for one C function parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiParam {
    /// Original HIR type being lowered.
    pub source: TyId,
    /// ABI-passing strategy.
    pub kind: AbiParamKind,
    /// Final eightbyte classes after SysV cleanup.
    pub classes: Vec<AbiClass>,
}

impl AbiParam {
    /// Number of LLVM IR parameters emitted for this one C parameter.
    pub fn llvm_param_count(&self) -> usize {
        match &self.kind {
            AbiParamKind::Direct(units) => units.len(),
            AbiParamKind::Indirect { .. } => 1,
        }
    }
}

/// ABI lowering for one C function return value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiReturn {
    /// Original HIR return type being lowered.
    pub source: TyId,
    /// ABI return strategy.
    pub kind: AbiReturnKind,
    /// Final eightbyte classes after SysV cleanup.
    pub classes: Vec<AbiClass>,
    /// Whether the LLVM return value needs `zeroext` normalization.
    pub zeroext: bool,
}

impl AbiReturn {
    /// Number of hidden LLVM IR parameters emitted for this return value.
    pub fn llvm_param_count(&self) -> usize {
        match self.kind {
            AbiReturnKind::Void | AbiReturnKind::Direct { .. } => 0,
            AbiReturnKind::Indirect { .. } => 1,
        }
    }
}

/// ABI return strategy for one C function return value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiReturnKind {
    /// `void` return.
    Void,
    /// Return directly in one or more ABI-classified registers.
    Direct {
        /// LLVM return units. Multiple units are wrapped in an LLVM struct.
        units: Vec<AbiParamUnit>,
    },
    /// Return indirectly through a hidden caller-provided pointer.
    Indirect {
        /// Whether LLVM should mark the hidden pointer as `sret`.
        sret: bool,
        /// Required alignment of the pointed-to storage in bytes.
        align: u32,
        /// Size of the original returned object in bytes.
        size: u64,
    },
}

/// ABI-passing strategy for one C function parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiParamKind {
    /// Pass directly as one or more LLVM scalar/vector parameters.
    Direct(Vec<AbiParamUnit>),
    /// Pass indirectly through a pointer to caller-owned storage.
    Indirect {
        /// Whether LLVM should treat the pointer as a by-value aggregate.
        byval: bool,
        /// Required alignment of the pointed-to storage in bytes.
        align: u32,
        /// Size of the original object in bytes.
        size: u64,
    },
}

/// One LLVM IR parameter produced by direct ABI lowering.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AbiParamUnit {
    /// SysV ABI class for this unit.
    pub class: AbiClass,
    /// LLVM type shape for this unit.
    pub kind: AbiParamUnitKind,
}

/// LLVM type shape for a direct ABI parameter unit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AbiParamUnitKind {
    /// Use the source type's natural LLVM lowering.
    Source(TyId),
    /// Coerce an aggregate eightbyte to an integer of this bit width.
    Integer {
        /// Integer bit width.
        bits: u32,
    },
    /// Coerce an aggregate eightbyte to a floating-point scalar.
    Float(FloatKind),
    /// Coerce an aggregate eightbyte to a fixed-width vector.
    Vector {
        /// Element floating-point kind.
        elem: FloatKind,
        /// Number of vector lanes.
        lanes: u32,
    },
}

/// ABI lowering for a whole C function signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnAbi {
    /// Lowered return value.
    pub ret: AbiReturn,
    /// Lowered fixed parameters in source order.
    pub params: Vec<AbiParam>,
    /// Whether the C function has an ellipsis.
    pub variadic: bool,
    /// LLVM IR parameter index where variadic call-site arguments begin.
    pub fixed_param_count: usize,
}

/// Classify a C function type's parameters for the baseline SysV x86-64 ABI.
pub fn sysv_fn_abi(
    tcx: &TyCtxt,
    defs: &IndexVec<DefId, Def>,
    ty: TyId,
) -> Result<FnAbi, CodegenError> {
    let (ret, params, variadic) = match tcx.get(ty) {
        Ty::Func { ret, params, variadic, .. } => (*ret, params.clone(), *variadic),
        _ => return Err(type_lowering_error(ty, "not a function type")),
    };

    let ret = sysv_return_abi(tcx, defs, ret)?;
    let mut lowered = Vec::with_capacity(params.len());
    for param in params {
        lowered.push(sysv_param_abi(tcx, defs, param)?);
    }
    let fixed_param_count =
        ret.llvm_param_count() + lowered.iter().map(AbiParam::llvm_param_count).sum::<usize>();

    Ok(FnAbi { ret, params: lowered, variadic, fixed_param_count })
}

/// Classify one C function return type for the baseline SysV x86-64 ABI.
pub fn sysv_return_abi(
    tcx: &TyCtxt,
    defs: &IndexVec<DefId, Def>,
    ty: TyId,
) -> Result<AbiReturn, CodegenError> {
    SysvParamClassifier::new(tcx, defs).classify_return(ty)
}

/// Classify one C function parameter for the baseline SysV x86-64 ABI.
pub fn sysv_param_abi(
    tcx: &TyCtxt,
    defs: &IndexVec<DefId, Def>,
    ty: TyId,
) -> Result<AbiParam, CodegenError> {
    SysvParamClassifier::new(tcx, defs).classify_param(ty)
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

const EIGHTBYTE_SIZE: u64 = 8;
const MAX_REGISTER_EIGHTBYTES: usize = 2;

struct SysvParamClassifier<'tcx> {
    tcx: &'tcx TyCtxt,
    defs: &'tcx IndexVec<DefId, Def>,
    layout: LayoutCx<'tcx>,
}

impl<'tcx> SysvParamClassifier<'tcx> {
    fn new(tcx: &'tcx TyCtxt, defs: &'tcx IndexVec<DefId, Def>) -> Self {
        Self { tcx, defs, layout: LayoutCx::with_defs(tcx, defs) }
    }

    fn classify_param(&self, ty: TyId) -> Result<AbiParam, CodegenError> {
        match self.tcx.get(ty) {
            Ty::Void => Err(type_lowering_error(ty, "void is not a parameter type")),
            Ty::Func { .. } => Err(type_lowering_error(ty, "function parameters must decay")),
            Ty::Error => Err(type_lowering_error(ty, "error type cannot be ABI-classified")),
            Ty::Int { .. } | Ty::Ptr(_) | Ty::Enum(_) | Ty::Float(_) => Ok(self.scalar_param(ty)),
            Ty::BuiltinVaList | Ty::Complex(_) | Ty::Array { .. } | Ty::Record(_) => {
                self.aggregate_param(ty)
            }
        }
    }

    fn classify_return(&self, ty: TyId) -> Result<AbiReturn, CodegenError> {
        match self.tcx.get(ty) {
            Ty::Void => Ok(AbiReturn {
                source: ty,
                kind: AbiReturnKind::Void,
                classes: Vec::new(),
                zeroext: false,
            }),
            Ty::Func { .. } => Err(type_lowering_error(ty, "function return types must decay")),
            Ty::Error => Err(type_lowering_error(ty, "error type cannot be ABI-classified")),
            Ty::Int { .. } | Ty::Ptr(_) | Ty::Enum(_) | Ty::Float(_) => Ok(self.scalar_return(ty)),
            Ty::Complex(FloatKind::F80) => Ok(AbiReturn {
                source: ty,
                kind: AbiReturnKind::Direct {
                    units: vec![AbiParamUnit {
                        class: AbiClass::ComplexX87,
                        kind: AbiParamUnitKind::Source(ty),
                    }],
                },
                classes: vec![AbiClass::ComplexX87],
                zeroext: false,
            }),
            Ty::BuiltinVaList | Ty::Complex(_) | Ty::Array { .. } | Ty::Record(_) => {
                self.aggregate_return(ty)
            }
        }
    }

    fn scalar_param(&self, ty: TyId) -> AbiParam {
        let classes = match self.tcx.get(ty) {
            Ty::Float(FloatKind::F80) => vec![AbiClass::X87, AbiClass::X87Up],
            Ty::Float(FloatKind::F32 | FloatKind::F64) => vec![AbiClass::Sse],
            Ty::Int { .. } | Ty::Ptr(_) | Ty::Enum(_) => vec![AbiClass::Integer],
            _ => unreachable!("scalar_param called for non-scalar type"),
        };
        AbiParam {
            source: ty,
            kind: AbiParamKind::Direct(vec![AbiParamUnit {
                class: classes[0],
                kind: AbiParamUnitKind::Source(ty),
            }]),
            classes,
        }
    }

    fn scalar_return(&self, ty: TyId) -> AbiReturn {
        let classes = match self.tcx.get(ty) {
            Ty::Float(FloatKind::F80) => vec![AbiClass::X87, AbiClass::X87Up],
            Ty::Float(FloatKind::F32 | FloatKind::F64) => vec![AbiClass::Sse],
            Ty::Int { .. } | Ty::Ptr(_) | Ty::Enum(_) => vec![AbiClass::Integer],
            _ => unreachable!("scalar_return called for non-scalar type"),
        };
        AbiReturn {
            source: ty,
            kind: AbiReturnKind::Direct {
                units: vec![AbiParamUnit { class: classes[0], kind: AbiParamUnitKind::Source(ty) }],
            },
            classes,
            zeroext: matches!(self.tcx.get(ty), Ty::Int { rank: rcc_hir::IntRank::Bool, .. }),
        }
    }

    fn aggregate_param(&self, ty: TyId) -> Result<AbiParam, CodegenError> {
        let layout =
            self.layout.layout_of(ty).map_err(|err| type_lowering_error(ty, err.to_string()))?;
        let eightbytes = eightbyte_count(layout.size, ty)?;
        if eightbytes > 4 {
            return Ok(indirect_param(ty, layout));
        }

        let mut chunks = vec![Eightbyte::default(); eightbytes];
        self.classify_ty_into(ty, 0, &mut chunks)?;
        post_cleanup(&mut chunks);

        if chunks.iter().any(|chunk| chunk.class == AbiClass::Memory)
            || needs_memory_after_cleanup(&chunks)
        {
            return Ok(indirect_param(ty, layout));
        }

        let mut units = Vec::with_capacity(chunks.len());
        for (idx, chunk) in chunks.iter().enumerate() {
            if chunk.class == AbiClass::NoClass {
                continue;
            }
            let size = eightbyte_payload_size(layout.size, idx);
            units.push(AbiParamUnit { class: chunk.class, kind: unit_kind(chunk, size, ty)? });
        }
        let classes = units.iter().map(|unit| unit.class).collect();
        Ok(AbiParam { source: ty, kind: AbiParamKind::Direct(units), classes })
    }

    fn aggregate_return(&self, ty: TyId) -> Result<AbiReturn, CodegenError> {
        let param = self.aggregate_param(ty)?;
        let ret = match param.kind {
            AbiParamKind::Direct(units) => AbiReturn {
                source: ty,
                kind: AbiReturnKind::Direct { units },
                classes: param.classes,
                zeroext: false,
            },
            AbiParamKind::Indirect { align, size, .. } => AbiReturn {
                source: ty,
                kind: AbiReturnKind::Indirect { sret: true, align, size },
                classes: vec![AbiClass::Memory],
                zeroext: false,
            },
        };
        Ok(ret)
    }

    fn classify_ty_into(
        &self,
        ty: TyId,
        offset: u64,
        chunks: &mut [Eightbyte],
    ) -> Result<(), CodegenError> {
        match self.tcx.get(ty) {
            Ty::Int { .. } | Ty::Ptr(_) | Ty::Enum(_) => {
                let layout = self
                    .layout
                    .layout_of(ty)
                    .map_err(|err| type_lowering_error(ty, err.to_string()))?;
                merge_range(chunks, offset, layout.size, AbiClass::Integer, ty)?;
                self.record_integer_part(offset, layout.size, chunks, ty)
            }
            Ty::Float(FloatKind::F32) => self.classify_float(offset, FloatKind::F32, chunks, ty),
            Ty::Float(FloatKind::F64) => self.classify_float(offset, FloatKind::F64, chunks, ty),
            Ty::Float(FloatKind::F80) => {
                merge_range(chunks, offset, 8, AbiClass::X87, ty)?;
                merge_range(chunks, offset + 8, 8, AbiClass::X87Up, ty)
            }
            Ty::Complex(FloatKind::F32) => {
                self.classify_float(offset, FloatKind::F32, chunks, ty)?;
                self.classify_float(offset + 4, FloatKind::F32, chunks, ty)
            }
            Ty::Complex(FloatKind::F64) => {
                self.classify_float(offset, FloatKind::F64, chunks, ty)?;
                self.classify_float(offset + 8, FloatKind::F64, chunks, ty)
            }
            Ty::Complex(FloatKind::F80) => {
                merge_range(chunks, offset, 32, AbiClass::ComplexX87, ty)
            }
            Ty::Array { elem, .. } => self.classify_array(ty, elem.ty, offset, chunks),
            Ty::Record(_) => self.classify_record(ty, offset, chunks),
            Ty::BuiltinVaList => self.classify_record(ty, offset, chunks),
            Ty::Void | Ty::Func { .. } | Ty::Error => {
                Err(type_lowering_error(ty, "type cannot appear inside an ABI aggregate"))
            }
        }
    }

    fn classify_float(
        &self,
        offset: u64,
        kind: FloatKind,
        chunks: &mut [Eightbyte],
        ty: TyId,
    ) -> Result<(), CodegenError> {
        let size = float_size(kind);
        merge_range(chunks, offset, size, AbiClass::Sse, ty)?;
        let idx = chunk_index(offset, ty)?;
        let Some(chunk) = chunks.get_mut(idx) else {
            return Err(type_lowering_error(ty, "ABI float escaped its aggregate"));
        };
        chunk.floats.push(FloatPart { offset: (offset % EIGHTBYTE_SIZE) as u8, kind });
        Ok(())
    }

    fn record_integer_part(
        &self,
        offset: u64,
        size: u64,
        chunks: &mut [Eightbyte],
        ty: TyId,
    ) -> Result<(), CodegenError> {
        if size == 0 || chunk_index(offset, ty)? != chunk_index(offset + size - 1, ty)? {
            return Ok(());
        }
        let idx = chunk_index(offset, ty)?;
        let Some(chunk) = chunks.get_mut(idx) else {
            return Err(type_lowering_error(ty, "ABI integer escaped its aggregate"));
        };
        chunk.ints.push(IntPart {
            offset: (offset % EIGHTBYTE_SIZE) as u8,
            bits: bits_for_size(size, ty)?,
        });
        Ok(())
    }

    fn classify_array(
        &self,
        ty: TyId,
        elem: TyId,
        offset: u64,
        chunks: &mut [Eightbyte],
    ) -> Result<(), CodegenError> {
        let array = self.array_layout(ty)?;
        let Some(len) = array.len else {
            return Err(type_lowering_error(ty, "incomplete arrays cannot be ABI-classified"));
        };
        for idx in 0..len {
            let elem_offset = offset
                .checked_add(
                    idx.checked_mul(array.elem.size)
                        .ok_or_else(|| type_lowering_error(ty, "array ABI offset overflow"))?,
                )
                .ok_or_else(|| type_lowering_error(ty, "array ABI offset overflow"))?;
            self.classify_ty_into(elem, elem_offset, chunks)?;
        }
        Ok(())
    }

    fn classify_record(
        &self,
        ty: TyId,
        offset: u64,
        chunks: &mut [Eightbyte],
    ) -> Result<(), CodegenError> {
        let Ty::Record(def) = self.tcx.get(ty) else {
            unreachable!("classify_record called for non-record type")
        };
        let record = self
            .layout
            .record_layout_of(ty)
            .map_err(|err| type_lowering_error(ty, err.to_string()))?;
        let def_data = self.defs.get(*def).ok_or_else(|| {
            type_lowering_error(ty, format!("record definition {def:?} is missing"))
        })?;
        let DefKind::Record { fields, .. } = &def_data.kind else {
            return Err(type_lowering_error(
                ty,
                "record type does not reference a record definition",
            ));
        };

        for (field, field_layout) in fields.iter().zip(record.fields.iter()) {
            if field_layout.storage_size == 0 {
                continue;
            }
            let field_offset = offset
                .checked_add(field_layout.offset)
                .ok_or_else(|| type_lowering_error(ty, "record field ABI offset overflow"))?;
            if field_offset % u64::from(field_layout.storage_align) != 0 {
                mark_memory(chunks);
                return Ok(());
            }
            self.classify_ty_into(field.ty, field_offset, chunks)?;
        }
        Ok(())
    }

    fn array_layout(&self, ty: TyId) -> Result<rcc_hir::ArrayLayout, CodegenError> {
        self.layout.array_layout_of(ty).map_err(|err| type_lowering_error(ty, err.to_string()))
    }
}

#[derive(Clone, Debug, Default)]
struct Eightbyte {
    class: AbiClass,
    ints: Vec<IntPart>,
    floats: Vec<FloatPart>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct FloatPart {
    offset: u8,
    kind: FloatKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct IntPart {
    offset: u8,
    bits: u32,
}

fn indirect_param(ty: TyId, layout: Layout) -> AbiParam {
    AbiParam {
        source: ty,
        kind: AbiParamKind::Indirect { byval: true, align: layout.align, size: layout.size },
        classes: vec![AbiClass::Memory],
    }
}

fn merge_range(
    chunks: &mut [Eightbyte],
    offset: u64,
    size: u64,
    class: AbiClass,
    ty: TyId,
) -> Result<(), CodegenError> {
    if size == 0 {
        return Ok(());
    }
    let start = chunk_index(offset, ty)?;
    let end = chunk_index(
        offset
            .checked_add(size - 1)
            .ok_or_else(|| type_lowering_error(ty, "ABI range overflow"))?,
        ty,
    )?;
    for idx in start..=end {
        let Some(chunk) = chunks.get_mut(idx) else {
            return Err(type_lowering_error(ty, "ABI range escaped its aggregate"));
        };
        chunk.class = merge_class(chunk.class, class);
    }
    Ok(())
}

fn merge_class(lhs: AbiClass, rhs: AbiClass) -> AbiClass {
    use AbiClass::{ComplexX87, Integer, Memory, NoClass, Sse, X87Up, X87};

    match (lhs, rhs) {
        (a, b) if a == b => a,
        (NoClass, b) => b,
        (a, NoClass) => a,
        (Memory, _) | (_, Memory) => Memory,
        (Integer, _) | (_, Integer) => Integer,
        (X87 | X87Up | ComplexX87, _) | (_, X87 | X87Up | ComplexX87) => Memory,
        _ => Sse,
    }
}

fn post_cleanup(chunks: &mut [Eightbyte]) {
    for idx in 0..chunks.len() {
        if chunks[idx].class == AbiClass::SseUp
            && (idx == 0 || !matches!(chunks[idx - 1].class, AbiClass::Sse | AbiClass::SseUp))
        {
            chunks[idx].class = AbiClass::Sse;
        }
    }
}

fn needs_memory_after_cleanup(chunks: &[Eightbyte]) -> bool {
    if chunks.len() <= MAX_REGISTER_EIGHTBYTES {
        return false;
    }
    chunks.first().map(|chunk| chunk.class != AbiClass::Sse).unwrap_or(false)
        || chunks.iter().skip(1).any(|chunk| chunk.class != AbiClass::SseUp)
}

fn mark_memory(chunks: &mut [Eightbyte]) {
    for chunk in chunks {
        chunk.class = AbiClass::Memory;
    }
}

fn unit_kind(chunk: &Eightbyte, size: u64, ty: TyId) -> Result<AbiParamUnitKind, CodegenError> {
    match chunk.class {
        AbiClass::Integer => {
            let mut ints = chunk.ints.clone();
            ints.sort_by_key(|part| part.offset);
            match ints.as_slice() {
                [IntPart { offset: 0, bits }] => Ok(AbiParamUnitKind::Integer { bits: *bits }),
                _ => Ok(AbiParamUnitKind::Integer { bits: bits_for_size(size, ty)? }),
            }
        }
        AbiClass::Sse => Ok(sse_unit_kind(chunk)),
        AbiClass::SseUp => Ok(sse_unit_kind(chunk)),
        AbiClass::NoClass
        | AbiClass::X87
        | AbiClass::X87Up
        | AbiClass::ComplexX87
        | AbiClass::Memory => Err(type_lowering_error(ty, "unsupported direct ABI class")),
    }
}

fn sse_unit_kind(chunk: &Eightbyte) -> AbiParamUnitKind {
    let mut floats = chunk.floats.clone();
    floats.sort_by_key(|part| part.offset);
    match floats.as_slice() {
        [FloatPart { offset: 0, kind: FloatKind::F32 }, FloatPart { offset: 4, kind: FloatKind::F32 }] => {
            AbiParamUnitKind::Vector { elem: FloatKind::F32, lanes: 2 }
        }
        [FloatPart { offset: 0, kind }] => AbiParamUnitKind::Float(*kind),
        _ => AbiParamUnitKind::Float(FloatKind::F64),
    }
}

fn eightbyte_count(size: u64, ty: TyId) -> Result<usize, CodegenError> {
    let rounded = size
        .checked_add(EIGHTBYTE_SIZE - 1)
        .ok_or_else(|| type_lowering_error(ty, "ABI size overflow"))?
        / EIGHTBYTE_SIZE;
    usize::try_from(rounded).map_err(|_| type_lowering_error(ty, "ABI size overflow"))
}

fn eightbyte_payload_size(total_size: u64, idx: usize) -> u64 {
    let offset = (idx as u64) * EIGHTBYTE_SIZE;
    (total_size - offset).min(EIGHTBYTE_SIZE)
}

fn bits_for_size(size: u64, ty: TyId) -> Result<u32, CodegenError> {
    let bits =
        size.checked_mul(8).ok_or_else(|| type_lowering_error(ty, "ABI integer width overflow"))?;
    u32::try_from(bits).map_err(|_| type_lowering_error(ty, "ABI integer width overflow"))
}

fn chunk_index(offset: u64, ty: TyId) -> Result<usize, CodegenError> {
    usize::try_from(offset / EIGHTBYTE_SIZE)
        .map_err(|_| type_lowering_error(ty, "ABI chunk index overflow"))
}

fn float_size(kind: FloatKind) -> u64 {
    match kind {
        FloatKind::F32 => 4,
        FloatKind::F64 => 8,
        FloatKind::F80 => 16,
    }
}

fn type_lowering_error(ty: TyId, reason: impl Into<String>) -> CodegenError {
    CodegenError::TypeLowering { ty, reason: reason.into() }
}

#[cfg(feature = "llvm")]
pub mod backend {
    //! The real inkwell-backed codegen.

    use super::*;

    use std::cell::RefCell;

    use inkwell::attributes::{Attribute, AttributeLoc};
    use inkwell::basic_block::BasicBlock as LlvmBasicBlock;
    use inkwell::builder::Builder;
    use inkwell::context::Context;
    use inkwell::debug_info::{
        AsDIScope, DICompileUnit, DIFile, DIFlags, DIFlagsConstants, DIScope, DISubprogram, DIType,
        DWARFEmissionKind, DWARFSourceLanguage, DebugInfoBuilder,
    };
    use inkwell::module::FlagBehavior;
    use inkwell::module::Linkage as LlvmLinkage;
    use inkwell::module::Module;
    #[cfg(test)]
    use inkwell::passes::PassBuilderOptions;
    use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target};
    use inkwell::targets::{TargetData, TargetMachine, TargetTriple};
    use inkwell::types::{
        AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType,
    };
    use inkwell::values::{
        BasicMetadataValueEnum, BasicValue, BasicValueEnum, CallSiteValue, FloatValue,
        FunctionValue, GlobalValue, InstructionOpcode, IntValue, PointerValue, StructValue,
    };
    use inkwell::OptimizationLevel;
    use inkwell::{AddressSpace, FloatPredicate, IntPredicate};
    use rcc_cfg::UnOp;
    use rcc_cfg::{
        BasicBlockId, BinOp, Body, CastKind, ConstKind, Operand, Place, Projection, Rvalue,
        Statement, StatementKind, TerminatorKind,
    };
    use rcc_hir::{
        DefKind, FloatKind, IntRank, Linkage as HirLinkage, Local, ObjectQuals, Qual, RecordKind,
        Ty,
    };
    use rcc_span::{FileId, Span};

    /// First supported backend target: Linux x86-64 SysV.
    pub const BASELINE_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

    /// LLVM data layout for the first supported Linux x86-64 SysV target.
    pub const BASELINE_DATA_LAYOUT: &str =
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128";

    const FALLBACK_MODULE_NAME: &str = "rcc_module";

    /// LLVM storage addresses for CFG locals.
    pub type LocalMap<'ctx> = IndexVec<Local, PointerValue<'ctx>>;

    /// Saved stack tokens for VLA locals, restored on `StorageDead`.
    pub type VlaStackMap<'ctx> = IndexVec<Local, Option<PointerValue<'ctx>>>;

    #[derive(Copy, Clone, Debug)]
    struct RecordFieldAccess {
        kind: RecordKind,
        ty: TyId,
        quals: ObjectQuals,
        layout: rcc_hir::layout::FieldLayout,
    }

    #[derive(Copy, Clone, Debug)]
    struct BitfieldAccess<'ctx> {
        storage_addr: PointerValue<'ctx>,
        storage_ty: inkwell::types::IntType<'ctx>,
        field_ty: TyId,
        bit_offset: u32,
        bit_width: u32,
        storage_bits: u32,
        volatile: bool,
    }

    struct DebugInfoCx<'ctx> {
        builder: DebugInfoBuilder<'ctx>,
        compile_unit: DICompileUnit<'ctx>,
        subprograms: RefCell<FxHashMap<DefId, DISubprogram<'ctx>>>,
        types: RefCell<FxHashMap<TyId, DIType<'ctx>>>,
    }

    impl<'ctx> DebugInfoCx<'ctx> {
        fn new(context: &'ctx Context, module: &Module<'ctx>, session: &Session) -> Self {
            let debug_metadata_version = context.i32_type().const_int(3, false);
            module.add_basic_value_flag(
                "Debug Info Version",
                FlagBehavior::Warning,
                debug_metadata_version,
            );

            let (filename, directory) = debug_compile_unit_path(session);
            let (builder, compile_unit) = module.create_debug_info_builder(
                true,
                DWARFSourceLanguage::C,
                &filename,
                &directory,
                "rcc",
                false,
                "",
                0,
                "",
                DWARFEmissionKind::Full,
                0,
                false,
                false,
                "",
                "",
            );

            Self {
                builder,
                compile_unit,
                subprograms: RefCell::new(FxHashMap::default()),
                types: RefCell::new(FxHashMap::default()),
            }
        }

        fn file(&self) -> DIFile<'ctx> {
            self.compile_unit.get_file()
        }

        fn scope(&self) -> DIScope<'ctx> {
            self.compile_unit.as_debug_info_scope()
        }
    }

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
        debug: Option<DebugInfoCx<'ctx>>,
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
            let debug =
                session.opts.debug_info.then(|| DebugInfoCx::new(context, &module, session));

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
                debug,
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

        /// Run just LLVM's mem2reg pass for backend tests that assert the
        /// alloca-based CFG emission is promotable. Production optimization
        /// pipelines are owned by later driver/quality tasks.
        #[cfg(test)]
        pub fn run_mem2reg_for_tests(&self) -> Result<(), CodegenError> {
            Target::initialize_x86(&InitializationConfig::default());
            let triple = TargetTriple::create(self.target_triple());
            let target = Target::from_triple(&triple).map_err(|err| {
                CodegenError::Internal(format!(
                    "failed to resolve target for mem2reg test: {}",
                    err.to_string()
                ))
            })?;
            let machine = target
                .create_target_machine(
                    &triple,
                    "generic",
                    "",
                    OptimizationLevel::None,
                    RelocMode::Default,
                    CodeModel::Default,
                )
                .ok_or_else(|| {
                    CodegenError::Internal(
                        "failed to create target machine for mem2reg test".to_owned(),
                    )
                })?;
            let options = PassBuilderOptions::create();
            self.module.run_passes("mem2reg", &machine, options).map_err(|err| {
                CodegenError::Internal(format!("LLVM mem2reg pass failed: {}", err.to_string()))
            })?;
            self.verify_module()
        }

        /// Render the current LLVM module as textual LLVM IR.
        pub fn ir_text(&self) -> String {
            self.module.print_to_string().to_string()
        }

        /// Emit the current module as target assembly text.
        pub fn assembly_text(&self) -> Result<String, CodegenError> {
            let bytes = self.emit_to_memory_buffer(FileType::Assembly)?;
            String::from_utf8(bytes).map_err(|err| {
                CodegenError::Internal(format!("LLVM assembly output was not UTF-8: {err}"))
            })
        }

        /// Emit the current module as native object bytes.
        pub fn object_bytes(&self) -> Result<Vec<u8>, CodegenError> {
            self.emit_to_memory_buffer(FileType::Object)
        }

        fn emit_to_memory_buffer(&self, file_type: FileType) -> Result<Vec<u8>, CodegenError> {
            let machine = self.target_machine()?;
            let buffer =
                machine.write_to_memory_buffer(&self.module, file_type).map_err(|err| {
                    CodegenError::Internal(format!(
                        "LLVM target emission failed: {}",
                        err.to_string()
                    ))
                })?;
            Ok(buffer.as_slice().to_vec())
        }

        fn target_machine(&self) -> Result<TargetMachine, CodegenError> {
            Target::initialize_x86(&InitializationConfig::default());
            let triple = TargetTriple::create(self.target_triple());
            let target = Target::from_triple(&triple).map_err(|err| {
                CodegenError::Internal(format!(
                    "failed to resolve target '{}': {}",
                    self.target_triple(),
                    err.to_string()
                ))
            })?;
            target
                .create_target_machine(
                    &triple,
                    "generic",
                    "",
                    llvm_opt_level(self.session.opts.opt_level),
                    RelocMode::PIC,
                    CodeModel::Default,
                )
                .ok_or_else(|| {
                    CodegenError::Internal(format!(
                        "failed to create target machine for '{}'",
                        self.target_triple()
                    ))
                })
        }

        fn finalize_debug_info(&self) {
            if let Some(debug) = &self.debug {
                debug.builder.finalize();
            }
        }

        fn debug_subprogram(
            &self,
            def: DefId,
            function: FunctionValue<'ctx>,
        ) -> Result<Option<DISubprogram<'ctx>>, CodegenError> {
            let Some(debug) = &self.debug else {
                return Ok(None);
            };
            if let Some(subprogram) = debug.subprograms.borrow().get(&def).copied() {
                return Ok(Some(subprogram));
            }

            let def_data = self.hir.defs.get(def).ok_or_else(|| {
                CodegenError::Internal(format!("function definition {def:?} is missing"))
            })?;
            let DefKind::Function { ty, has_body, .. } = &def_data.kind else {
                return Err(CodegenError::Internal(format!(
                    "definition {def:?} is not a function"
                )));
            };
            let Ty::Func { ret, params, .. } = self.tcx.get(*ty) else {
                return Err(type_lowering_error(
                    *ty,
                    "function definition does not name a function type",
                ));
            };

            let return_ty = if *ret == self.tcx.void { None } else { Some(self.debug_type(*ret)?) };
            let mut param_tys = Vec::with_capacity(params.len());
            for param in params {
                param_tys.push(self.debug_type(*param)?);
            }
            let subroutine_ty = debug.builder.create_subroutine_type(
                debug.file(),
                return_ty,
                &param_tys,
                DIFlags::PUBLIC,
            );
            let loc = self.debug_line_col(def_data.span);
            let name = self.def_name(def_data);
            let subprogram = debug.builder.create_function(
                debug.scope(),
                &name,
                None,
                debug.file(),
                loc.line,
                subroutine_ty,
                true,
                *has_body,
                loc.line,
                DIFlags::PUBLIC,
                false,
            );
            function.set_subprogram(subprogram);
            debug.subprograms.borrow_mut().insert(def, subprogram);
            Ok(Some(subprogram))
        }

        fn emit_debug_declarations(
            &self,
            function: FunctionValue<'ctx>,
            body: &Body,
            locals: &LocalMap<'ctx>,
        ) -> Result<(), CodegenError> {
            let Some(debug) = &self.debug else {
                return Ok(());
            };
            let Some(def) = body.def else {
                return Ok(());
            };
            let Some(subprogram) = self.debug_subprogram(def, function)? else {
                return Ok(());
            };
            let Some(entry) = function.get_first_basic_block() else {
                return Ok(());
            };

            let mut param_index = 1u32;
            for (local, decl) in body.locals.iter_enumerated() {
                let Some(name) = decl.name else {
                    continue;
                };
                if matches!(self.tcx.get(decl.ty), Ty::Array { is_vla: true, .. }) {
                    continue;
                }
                let Some(&storage) = locals.get(local) else {
                    continue;
                };
                if storage.is_null() {
                    continue;
                }

                let loc = self.debug_line_col(decl.span);
                let name = self.session.interner.get(name);
                let ty = self.debug_type(decl.ty)?;
                let var = if decl.is_param {
                    let var = debug.builder.create_parameter_variable(
                        subprogram.as_debug_info_scope(),
                        name,
                        param_index,
                        debug.file(),
                        loc.line,
                        ty,
                        true,
                        DIFlags::PUBLIC,
                    );
                    param_index = param_index.saturating_add(1);
                    var
                } else {
                    let layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(decl.ty)
                        .unwrap_or(Layout { size: 0, align: 1 });
                    debug.builder.create_auto_variable(
                        subprogram.as_debug_info_scope(),
                        name,
                        debug.file(),
                        loc.line,
                        ty,
                        true,
                        DIFlags::PUBLIC,
                        layout.align.saturating_mul(8),
                    )
                };

                let debug_loc = debug.builder.create_debug_location(
                    self.context,
                    loc.line,
                    loc.col,
                    subprogram.as_debug_info_scope(),
                    None,
                );
                debug.builder.insert_declare_at_end(storage, Some(var), None, debug_loc, entry);
            }

            self.clear_debug_location();
            Ok(())
        }

        fn set_debug_location(&self, def: Option<DefId>, span: Span) {
            let Some(debug) = &self.debug else {
                return;
            };
            let scope = def
                .and_then(|def| debug.subprograms.borrow().get(&def).copied())
                .map(|subprogram| subprogram.as_debug_info_scope())
                .unwrap_or_else(|| debug.scope());
            let loc = self.debug_line_col(span);
            let debug_loc =
                debug.builder.create_debug_location(self.context, loc.line, loc.col, scope, None);
            self.builder.set_current_debug_location(debug_loc);
        }

        fn clear_debug_location(&self) {
            if self.debug.is_some() {
                self.builder.unset_current_debug_location();
            }
        }

        fn debug_type(&self, ty: TyId) -> Result<DIType<'ctx>, CodegenError> {
            let debug = self
                .debug
                .as_ref()
                .ok_or_else(|| CodegenError::Internal("debug info is disabled".to_owned()))?;
            if let Some(ty) = debug.types.borrow().get(&ty).copied() {
                return Ok(ty);
            }

            let di_ty = match self.tcx.get(ty) {
                Ty::Void => debug
                    .builder
                    .create_basic_type("void", 0, 0x00, DIFlags::PUBLIC)
                    .map_err(|e| type_lowering_error(ty, e))?
                    .as_type(),
                Ty::Int { signed, rank } => {
                    let (name, bits, encoding) = debug_int_type(*signed, *rank);
                    debug
                        .builder
                        .create_basic_type(name, bits, encoding, DIFlags::PUBLIC)
                        .map_err(|e| type_lowering_error(ty, e))?
                        .as_type()
                }
                Ty::Float(kind) => {
                    let (name, bits) = match kind {
                        FloatKind::F32 => ("float", 32),
                        FloatKind::F64 => ("double", 64),
                        FloatKind::F80 => ("long double", 128),
                    };
                    debug
                        .builder
                        .create_basic_type(name, bits, 0x04, DIFlags::PUBLIC)
                        .map_err(|e| type_lowering_error(ty, e))?
                        .as_type()
                }
                Ty::Complex(kind) => {
                    let name = match kind {
                        FloatKind::F32 => "_Complex float",
                        FloatKind::F64 => "_Complex double",
                        FloatKind::F80 => "_Complex long double",
                    };
                    let layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(ty)
                        .map_err(|e| type_lowering_error(ty, e.to_string()))?;
                    debug
                        .builder
                        .create_basic_type(name, layout.size * 8, 0x04, DIFlags::PUBLIC)
                        .map_err(|e| type_lowering_error(ty, e))?
                        .as_type()
                }
                Ty::Ptr(pointee) => {
                    let pointee = self.debug_type(pointee.ty)?;
                    debug
                        .builder
                        .create_pointer_type("", pointee, 64, 64, AddressSpace::default())
                        .as_type()
                }
                Ty::Array { elem, len, is_vla } => {
                    let elem_ty = self.debug_type(elem.ty)?;
                    let elem_layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(elem.ty)
                        .map_err(|e| type_lowering_error(ty, e.to_string()))?;
                    let count = if *is_vla { 0 } else { len.unwrap_or(0) };
                    let size = elem_layout.size.saturating_mul(count).saturating_mul(8);
                    let range_end = i64::try_from(count).unwrap_or(i64::MAX);
                    debug
                        .builder
                        .create_array_type(
                            elem_ty,
                            size,
                            elem_layout.align.saturating_mul(8),
                            &[0..range_end],
                        )
                        .as_type()
                }
                Ty::Record(def) => self.debug_record_type(ty, *def)?,
                Ty::Enum(def) => {
                    let name = self
                        .hir
                        .defs
                        .get(*def)
                        .map(|def| self.session.interner.get(def.name).to_owned())
                        .unwrap_or_else(|| "enum".to_owned());
                    debug
                        .builder
                        .create_basic_type(&format!("enum {name}"), 32, 0x05, DIFlags::PUBLIC)
                        .map_err(|e| type_lowering_error(ty, e))?
                        .as_type()
                }
                Ty::Func { .. } => debug
                    .builder
                    .create_pointer_type(
                        "function pointer",
                        self.debug_type(self.tcx.void)?,
                        64,
                        64,
                        AddressSpace::default(),
                    )
                    .as_type(),
                Ty::BuiltinVaList => debug
                    .builder
                    .create_pointer_type(
                        "__builtin_va_list",
                        self.debug_type(self.tcx.char_)?,
                        64,
                        64,
                        AddressSpace::default(),
                    )
                    .as_type(),
                Ty::Error => {
                    return Err(type_lowering_error(ty, "error type has no debug metadata"));
                }
            };

            debug.types.borrow_mut().insert(ty, di_ty);
            Ok(di_ty)
        }

        fn debug_record_type(&self, ty: TyId, def: DefId) -> Result<DIType<'ctx>, CodegenError> {
            let debug = self
                .debug
                .as_ref()
                .ok_or_else(|| CodegenError::Internal("debug info is disabled".to_owned()))?;
            let def_data = self
                .hir
                .defs
                .get(def)
                .ok_or_else(|| type_lowering_error(ty, "record definition is missing"))?;
            let DefKind::Record { kind, .. } = &def_data.kind else {
                return Err(type_lowering_error(ty, "record definition id does not name a record"));
            };
            let layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                .layout_of(ty)
                .map_err(|e| type_lowering_error(ty, e.to_string()))?;
            let tag = match kind {
                RecordKind::Struct => "struct",
                RecordKind::Union => "union",
            };
            let name = self.session.interner.get(def_data.name);
            let loc = self.debug_line_col(def_data.span);
            let di_ty = match kind {
                RecordKind::Struct => debug.builder.create_struct_type(
                    debug.scope(),
                    name,
                    debug.file(),
                    loc.line,
                    layout.size.saturating_mul(8),
                    layout.align.saturating_mul(8),
                    DIFlags::PUBLIC,
                    None,
                    &[],
                    0,
                    None,
                    &format!("rcc.{tag}.{name}"),
                ),
                RecordKind::Union => debug.builder.create_union_type(
                    debug.scope(),
                    name,
                    debug.file(),
                    loc.line,
                    layout.size.saturating_mul(8),
                    layout.align.saturating_mul(8),
                    DIFlags::PUBLIC,
                    &[],
                    0,
                    &format!("rcc.{tag}.{name}"),
                ),
            };
            Ok(di_ty.as_type())
        }

        fn debug_line_col(&self, span: Span) -> rcc_span::LineCol {
            if span.file == FileId::DUMMY {
                return rcc_span::LineCol { line: 0, col: 0 };
            }
            self.session
                .source_map
                .read()
                .ok()
                .map(|source_map| source_map.lookup_line_col(span.file, span.lo))
                .unwrap_or(rcc_span::LineCol { line: 0, col: 0 })
        }

        /// Build a type-lowering helper sharing this module's context and HIR.
        pub fn type_cx(&self) -> TypeCx<'a, 'ctx> {
            TypeCx::new(self.context, self.tcx, self.hir)
        }

        /// Declare every HIR function and file-scope object in this LLVM module.
        pub fn declare_all(&mut self) -> Result<(), CodegenError> {
            let defs = self.hir.defs.iter_enumerated().map(|(id, _)| id).collect::<Vec<_>>();
            for def in defs {
                match &self.hir.defs[def].kind {
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
            let abi = sysv_fn_abi(self.tcx, &self.hir.defs, ty)?;
            let function = match self.module.get_function(&name) {
                Some(function) => function,
                None => {
                    let function = self.module.add_function(&name, fn_ty, Some(linkage));
                    self.apply_param_abi_attrs(function, &abi)?;
                    function
                }
            };
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

        // ---------------------------------------------------------------
        // 09-12: Basic block and terminator wiring
        // ---------------------------------------------------------------

        /// Emit every CFG body in HIR definition order.
        pub fn codegen_all_bodies(&self) -> Result<(), CodegenError> {
            for (def, def_data) in self.hir.defs.iter_enumerated() {
                if !matches!(&def_data.kind, DefKind::Function { has_body: true, .. }) {
                    continue;
                }
                let Some(body) = self.bodies.get(&def) else {
                    continue;
                };
                let function = self.functions.get(&def).copied().ok_or_else(|| {
                    CodegenError::Internal(format!("function definition {def:?} was not declared"))
                })?;
                self.codegen_body(function, body)?;
            }
            Ok(())
        }

        /// Lower one CFG body into its declared LLVM function.
        pub fn codegen_body(
            &self,
            function: FunctionValue<'ctx>,
            body: &Body,
        ) -> Result<(), CodegenError> {
            rcc_cfg::verify::verify_body_with_hir(body, self.tcx, self.hir).map_err(|errors| {
                CodegenError::Internal(format!(
                    "CFG verifier failed before LLVM codegen: {}",
                    format_cfg_errors(&errors)
                ))
            })?;
            let mut fn_codegen = FnCodegen::new(self, function, body)?;
            fn_codegen.codegen_body()
        }

        fn def_name(&self, def: &Def) -> String {
            self.session.interner.get(def.name).to_owned()
        }

        fn apply_param_abi_attrs(
            &self,
            function: FunctionValue<'ctx>,
            abi: &FnAbi,
        ) -> Result<(), CodegenError> {
            self.apply_return_abi_attrs(function, &abi.ret)?;

            let byval_kind = Attribute::get_named_enum_kind_id("byval");
            let align_kind = Attribute::get_named_enum_kind_id("align");
            let mut param_index = u32::try_from(abi.ret.llvm_param_count()).map_err(|_| {
                CodegenError::Internal("function parameter index overflowed".to_owned())
            })?;

            for param in &abi.params {
                match &param.kind {
                    AbiParamKind::Direct(units) => {
                        param_index = param_index
                            .checked_add(u32::try_from(units.len()).map_err(|_| {
                                CodegenError::Internal(
                                    "function parameter index overflowed".to_owned(),
                                )
                            })?)
                            .ok_or_else(|| {
                                CodegenError::Internal(
                                    "function parameter index overflowed".to_owned(),
                                )
                            })?;
                    }
                    AbiParamKind::Indirect { byval, align, .. } => {
                        if *byval {
                            let pointee = self.type_cx().basic_type_of(param.source)?;
                            let attr = self
                                .context
                                .create_type_attribute(byval_kind, basic_type_as_any(pointee));
                            function.add_attribute(AttributeLoc::Param(param_index), attr);
                        }
                        let attr =
                            self.context.create_enum_attribute(align_kind, u64::from(*align));
                        function.add_attribute(AttributeLoc::Param(param_index), attr);
                        param_index = param_index.checked_add(1).ok_or_else(|| {
                            CodegenError::Internal("function parameter index overflowed".to_owned())
                        })?;
                    }
                }
            }

            Ok(())
        }

        fn apply_return_abi_attrs(
            &self,
            function: FunctionValue<'ctx>,
            ret: &AbiReturn,
        ) -> Result<(), CodegenError> {
            if ret.zeroext {
                let zeroext_kind = Attribute::get_named_enum_kind_id("zeroext");
                let attr = self.context.create_enum_attribute(zeroext_kind, 0);
                function.add_attribute(AttributeLoc::Return, attr);
            }

            if let AbiReturnKind::Indirect { sret, align, .. } = &ret.kind {
                if *sret {
                    let sret_kind = Attribute::get_named_enum_kind_id("sret");
                    let pointee = self.type_cx().basic_type_of(ret.source)?;
                    let attr =
                        self.context.create_type_attribute(sret_kind, basic_type_as_any(pointee));
                    function.add_attribute(AttributeLoc::Param(0), attr);
                }
                let align_kind = Attribute::get_named_enum_kind_id("align");
                let attr = self.context.create_enum_attribute(align_kind, u64::from(*align));
                function.add_attribute(AttributeLoc::Param(0), attr);
            }

            Ok(())
        }

        fn apply_call_abi_attrs(
            &self,
            call: CallSiteValue<'ctx>,
            abi: &FnAbi,
        ) -> Result<(), CodegenError> {
            if abi.ret.zeroext {
                let zeroext_kind = Attribute::get_named_enum_kind_id("zeroext");
                let attr = self.context.create_enum_attribute(zeroext_kind, 0);
                call.add_attribute(AttributeLoc::Return, attr);
            }

            let sret_kind = Attribute::get_named_enum_kind_id("sret");
            let byval_kind = Attribute::get_named_enum_kind_id("byval");
            let align_kind = Attribute::get_named_enum_kind_id("align");

            let mut param_index = 0u32;
            if let AbiReturnKind::Indirect { sret, align, .. } = &abi.ret.kind {
                if *sret {
                    let pointee = self.type_cx().basic_type_of(abi.ret.source)?;
                    let attr =
                        self.context.create_type_attribute(sret_kind, basic_type_as_any(pointee));
                    call.add_attribute(AttributeLoc::Param(param_index), attr);
                }
                let attr = self.context.create_enum_attribute(align_kind, u64::from(*align));
                call.add_attribute(AttributeLoc::Param(param_index), attr);
                param_index = param_index.checked_add(1).ok_or_else(|| {
                    CodegenError::Internal("call parameter index overflowed".to_owned())
                })?;
            }

            for param in &abi.params {
                match &param.kind {
                    AbiParamKind::Direct(units) => {
                        param_index = param_index
                            .checked_add(u32::try_from(units.len()).map_err(|_| {
                                CodegenError::Internal("call parameter index overflowed".to_owned())
                            })?)
                            .ok_or_else(|| {
                                CodegenError::Internal("call parameter index overflowed".to_owned())
                            })?;
                    }
                    AbiParamKind::Indirect { byval, align, .. } => {
                        if *byval {
                            let pointee = self.type_cx().basic_type_of(param.source)?;
                            let attr = self
                                .context
                                .create_type_attribute(byval_kind, basic_type_as_any(pointee));
                            call.add_attribute(AttributeLoc::Param(param_index), attr);
                        }
                        let attr =
                            self.context.create_enum_attribute(align_kind, u64::from(*align));
                        call.add_attribute(AttributeLoc::Param(param_index), attr);
                        param_index = param_index.checked_add(1).ok_or_else(|| {
                            CodegenError::Internal("call parameter index overflowed".to_owned())
                        })?;
                    }
                }
            }

            Ok(())
        }

        // ---------------------------------------------------------------
        // 09-10: Entry alloca and local materialization
        // ---------------------------------------------------------------

        /// Allocate storage for every CFG local in the function entry block and
        /// initialize parameter locals from LLVM IR parameters.
        pub fn materialize_locals(
            &self,
            function: FunctionValue<'ctx>,
            body: &Body,
        ) -> Result<LocalMap<'ctx>, CodegenError> {
            let mut locals = LocalMap::with_capacity(body.locals.len());

            for (local, decl) in body.locals.iter_enumerated() {
                let is_vla = matches!(self.tcx.get(decl.ty), Ty::Array { is_vla: true, .. });
                if is_vla {
                    let placeholder = self.context.ptr_type(AddressSpace::default()).const_null();
                    locals.push(placeholder);
                } else {
                    let storage_ty = self.local_storage_type(decl.ty)?;
                    let alloca = self.build_entry_alloca(
                        function,
                        storage_ty,
                        &self.local_storage_name(local, decl),
                    )?;
                    locals.push(alloca);
                }
            }

            self.store_function_params(function, body, &mut locals)?;
            Ok(locals)
        }

        /// Emit an LLVM lifetime.start marker for a CFG `StorageLive`, or
        /// perform dynamic stack allocation for a VLA local.
        pub fn emit_storage_live(
            &self,
            local: Local,
            locals: &mut LocalMap<'ctx>,
            vla_stacks: &mut VlaStackMap<'ctx>,
            body: &Body,
        ) -> Result<(), CodegenError> {
            let decl = body.locals.get(local).ok_or_else(|| {
                CodegenError::Internal(format!("local {local:?} is missing from body"))
            })?;
            match self.tcx.get(decl.ty) {
                Ty::Array { is_vla: true, elem, .. } => {
                    let len_local = decl.vla_len.ok_or_else(|| {
                        CodegenError::Internal(format!("VLA local {local:?} is missing vla_len"))
                    })?;
                    let len_ptr = *locals.get(len_local).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "missing LLVM storage for vla_len local {len_local:?}"
                        ))
                    })?;
                    let len_llvm_ty = self.type_cx().basic_type_of(self.tcx.ulong)?;
                    let len_val = self
                        .builder
                        .build_load(len_llvm_ty, len_ptr, "vla_len")
                        .map_err(builder_error)?
                        .into_int_value();
                    let elem_llvm_ty = self.type_cx().basic_type_of(elem.ty)?;
                    let stack_token = self.emit_stack_save()?;
                    let alloca = self
                        .builder
                        .build_array_alloca(
                            elem_llvm_ty,
                            len_val,
                            &self.local_storage_name(local, decl),
                        )
                        .map_err(builder_error)?;
                    locals[local] = alloca;
                    vla_stacks[local] = Some(stack_token);
                    Ok(())
                }
                _ => self.emit_lifetime_marker("llvm.lifetime.start.p0", local, locals, body),
            }
        }

        /// Emit an LLVM lifetime.end marker for a CFG `StorageDead`, or restore
        /// the saved stack position for a VLA local.
        pub fn emit_storage_dead(
            &self,
            local: Local,
            locals: &LocalMap<'ctx>,
            vla_stacks: &mut VlaStackMap<'ctx>,
            body: &Body,
        ) -> Result<(), CodegenError> {
            let decl = body.locals.get(local).ok_or_else(|| {
                CodegenError::Internal(format!("local {local:?} is missing from body"))
            })?;
            if matches!(self.tcx.get(decl.ty), Ty::Array { is_vla: true, .. }) {
                let stack_token =
                    vla_stacks.get_mut(local).and_then(Option::take).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "VLA local {local:?} reached StorageDead before StorageLive"
                        ))
                    })?;
                self.emit_stack_restore(stack_token)
            } else {
                self.emit_lifetime_marker("llvm.lifetime.end.p0", local, locals, body)
            }
        }

        fn local_storage_type(&self, ty: TyId) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
            match self.tcx.get(ty) {
                Ty::Void => Ok(self.context.i8_type().into()),
                Ty::Array { is_vla: true, .. } => Err(CodegenError::Internal(
                    "VLA local materialization is deferred to task 09-17".to_owned(),
                )),
                _ => self.type_cx().basic_type_of(ty),
            }
        }

        fn local_storage_name(&self, local: Local, decl: &rcc_cfg::LocalDecl) -> String {
            if local == Local(0) {
                return "ret.addr".to_owned();
            }

            match (decl.is_param, decl.name) {
                (true, Some(name)) => format!("param{}.addr", name.0),
                (false, Some(name)) => format!("local{}.addr", name.0),
                (true, None) => format!("param{}.addr", local.0),
                (false, None) => format!("tmp{}.addr", local.0),
            }
        }

        fn build_entry_alloca(
            &self,
            function: FunctionValue<'ctx>,
            ty: BasicTypeEnum<'ctx>,
            name: &str,
        ) -> Result<PointerValue<'ctx>, CodegenError> {
            let entry = function.get_first_basic_block().ok_or_else(|| {
                CodegenError::Internal(format!(
                    "function {} has no entry block",
                    function.get_name().to_string_lossy()
                ))
            })?;
            let saved_block = self.builder.get_insert_block();

            if let Some(first_non_alloca) =
                entry.get_instructions().find(|inst| inst.get_opcode() != InstructionOpcode::Alloca)
            {
                self.builder.position_before(&first_non_alloca);
            } else {
                self.builder.position_at_end(entry);
            }

            let alloca = self.builder.build_alloca(ty, name).map_err(builder_error)?;
            if let Some(block) = saved_block {
                self.builder.position_at_end(block);
            }
            Ok(alloca)
        }

        fn store_function_params(
            &self,
            function: FunctionValue<'ctx>,
            body: &Body,
            locals: &mut LocalMap<'ctx>,
        ) -> Result<(), CodegenError> {
            if body.locals.iter().all(|decl| !decl.is_param) {
                return Ok(());
            }

            if let Some(abi) = self.body_abi(body)? {
                let mut llvm_index = u32::try_from(abi.ret.llvm_param_count()).map_err(|_| {
                    CodegenError::Internal("function parameter index overflowed".to_owned())
                })?;
                for ((local, _decl), param_abi) in body
                    .locals
                    .iter_enumerated()
                    .filter(|(_, decl)| decl.is_param)
                    .zip(abi.params.iter())
                {
                    self.store_abi_param(function, local, param_abi, &mut llvm_index, locals)?;
                }
            } else {
                for (llvm_index, (local, _decl)) in
                    body.locals.iter_enumerated().filter(|(_, decl)| decl.is_param).enumerate()
                {
                    let llvm_index = u32::try_from(llvm_index).map_err(|_| {
                        CodegenError::Internal("function parameter index overflowed".to_owned())
                    })?;
                    let value = function.get_nth_param(llvm_index).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "missing LLVM parameter {} for local {:?}",
                            llvm_index, local
                        ))
                    })?;
                    self.builder.build_store(locals[local], value).map_err(builder_error)?;
                }
            }

            Ok(())
        }

        fn body_abi(&self, body: &Body) -> Result<Option<FnAbi>, CodegenError> {
            let Some(def) = body.def else {
                return Ok(None);
            };
            let def_data = self.hir.defs.get(def).ok_or_else(|| {
                CodegenError::Internal(format!("function definition {def:?} is missing"))
            })?;
            let DefKind::Function { ty, .. } = &def_data.kind else {
                return Err(CodegenError::Internal(format!(
                    "body definition {def:?} is not a function"
                )));
            };
            sysv_fn_abi(self.tcx, &self.hir.defs, *ty).map(Some)
        }

        fn store_abi_param(
            &self,
            function: FunctionValue<'ctx>,
            local: Local,
            param_abi: &AbiParam,
            llvm_index: &mut u32,
            locals: &mut LocalMap<'ctx>,
        ) -> Result<(), CodegenError> {
            match &param_abi.kind {
                AbiParamKind::Direct(units) => {
                    for (unit_idx, _) in units.iter().enumerate() {
                        let value = function.get_nth_param(*llvm_index).ok_or_else(|| {
                            CodegenError::Internal(format!(
                                "missing LLVM parameter {} for local {:?}",
                                *llvm_index, local
                            ))
                        })?;
                        let offset = u64::try_from(unit_idx)
                            .map_err(|_| {
                                CodegenError::Internal(
                                    "ABI parameter unit index overflowed".to_owned(),
                                )
                            })?
                            .checked_mul(8)
                            .ok_or_else(|| {
                                CodegenError::Internal(
                                    "ABI parameter unit offset overflowed".to_owned(),
                                )
                            })?;
                        self.store_abi_unit(locals[local], offset, value)?;
                        *llvm_index = llvm_index.checked_add(1).ok_or_else(|| {
                            CodegenError::Internal("function parameter index overflowed".to_owned())
                        })?;
                    }
                }
                AbiParamKind::Indirect { .. } => {
                    let value = function.get_nth_param(*llvm_index).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "missing LLVM parameter {} for local {:?}",
                            *llvm_index, local
                        ))
                    })?;
                    locals[local] = value.into_pointer_value();
                    *llvm_index = llvm_index.checked_add(1).ok_or_else(|| {
                        CodegenError::Internal("function parameter index overflowed".to_owned())
                    })?;
                }
            }

            Ok(())
        }

        fn store_abi_unit(
            &self,
            local_addr: PointerValue<'ctx>,
            offset: u64,
            value: BasicValueEnum<'ctx>,
        ) -> Result<(), CodegenError> {
            let offset = self.context.i64_type().const_int(offset, false);
            let byte_ptr =
                self.build_gep(self.context.i8_type(), local_addr, &[offset], "param.unit")?;
            self.builder.build_store(byte_ptr, value).map(|_| ()).map_err(builder_error)
        }

        fn emit_lifetime_marker(
            &self,
            intrinsic: &str,
            local: Local,
            locals: &LocalMap<'ctx>,
            body: &Body,
        ) -> Result<(), CodegenError> {
            let ptr = *locals.get(local).ok_or_else(|| {
                CodegenError::Internal(format!("missing LLVM storage for local {local:?}"))
            })?;
            let decl = body.locals.get(local).ok_or_else(|| {
                CodegenError::Internal(format!("local {local:?} is missing from body"))
            })?;
            let size = self.local_lifetime_size(decl.ty)?;
            let intrinsic = self.lifetime_intrinsic(intrinsic);
            self.builder
                .build_call(intrinsic, &[size.into(), ptr.into()], "")
                .map(|_| ())
                .map_err(builder_error)
        }

        fn emit_stack_save(&self) -> Result<PointerValue<'ctx>, CodegenError> {
            let intrinsic = self.stack_save_intrinsic();
            let args: &[BasicMetadataValueEnum<'ctx>] = &[];
            let call =
                self.builder.build_call(intrinsic, args, "vla_stack").map_err(builder_error)?;
            let value = call.try_as_basic_value().left().ok_or_else(|| {
                CodegenError::Internal("llvm.stacksave did not produce a value".to_owned())
            })?;
            Ok(value.into_pointer_value())
        }

        fn emit_stack_restore(&self, stack_token: PointerValue<'ctx>) -> Result<(), CodegenError> {
            let intrinsic = self.stack_restore_intrinsic();
            self.builder
                .build_call(intrinsic, &[stack_token.into()], "")
                .map(|_| ())
                .map_err(builder_error)
        }

        fn local_lifetime_size(
            &self,
            ty: TyId,
        ) -> Result<inkwell::values::IntValue<'ctx>, CodegenError> {
            let size = match self.tcx.get(ty) {
                Ty::Void => 0,
                Ty::Array { is_vla: true, .. } => {
                    return Err(CodegenError::Internal(
                        "VLA locals use stackrestore instead of lifetime markers".to_owned(),
                    ));
                }
                _ => {
                    LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(ty)
                        .map_err(|err| type_error(ty, err.to_string()))?
                        .size
                }
            };
            Ok(self.context.i64_type().const_int(size, false))
        }

        fn lifetime_intrinsic(&self, name: &str) -> FunctionValue<'ctx> {
            self.module.get_function(name).unwrap_or_else(|| {
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                let fn_ty =
                    self.context.void_type().fn_type(&[i64_ty.into(), ptr_ty.into()], false);
                self.module.add_function(name, fn_ty, None)
            })
        }

        fn stack_save_intrinsic(&self) -> FunctionValue<'ctx> {
            self.module.get_function("llvm.stacksave.p0").unwrap_or_else(|| {
                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                let fn_ty = ptr_ty.fn_type(&[], false);
                self.module.add_function("llvm.stacksave.p0", fn_ty, None)
            })
        }

        fn stack_restore_intrinsic(&self) -> FunctionValue<'ctx> {
            self.module.get_function("llvm.stackrestore.p0").unwrap_or_else(|| {
                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                let fn_ty = self.context.void_type().fn_type(&[ptr_ty.into()], false);
                self.module.add_function("llvm.stackrestore.p0", fn_ty, None)
            })
        }

        // ---------------------------------------------------------------
        // 09-09: Place address, operand load, and store helpers
        // ---------------------------------------------------------------

        fn place_is_volatile(&self, place: &Place, body: &Body) -> Result<bool, CodegenError> {
            let decl = body.locals.get(place.base).ok_or_else(|| {
                CodegenError::Internal(format!("place base local {:?} is missing", place.base))
            })?;
            let mut current_ty = decl.ty;
            let mut current_volatile = decl.quals.is_volatile;
            for proj in &place.projection {
                match proj {
                    Projection::Deref => {
                        let Ty::Ptr(pointee) = self.tcx.get(current_ty) else {
                            return Err(invalid_place_projection("dereference", current_ty));
                        };
                        current_volatile = pointee.is_volatile;
                        current_ty = pointee.ty;
                    }
                    Projection::Field(idx) => {
                        let (_, field_ty, quals) = self.record_field_info(current_ty, *idx)?;
                        current_volatile |= quals.is_volatile;
                        current_ty = field_ty;
                    }
                    Projection::Index(_) => match self.tcx.get(current_ty) {
                        Ty::Array { elem, .. } => {
                            current_volatile |= elem.is_volatile;
                            current_ty = elem.ty;
                        }
                        Ty::Ptr(pointee) => {
                            current_volatile = pointee.is_volatile;
                            current_ty = pointee.ty;
                        }
                        _ => return Err(invalid_place_projection("index", current_ty)),
                    },
                }
            }
            Ok(current_volatile)
        }

        /// `ObjectQuals` on the object path (local and struct fields) for the
        /// first `prefix_len` projections — used for volatile *pointer* loads
        /// (`int *volatile p`), not for `volatile int *p` pointee quals.
        ///
        /// For arrays of pointers, `int *volatile a[N]` stores each element as
        /// `Qual { ty: Ptr(..), is_volatile: true }`; indexing must treat that
        /// element slot like a volatile pointer object (`volatile int *` keeps
        /// volatility inside [`Ty::Ptr`] only — excluded here via `elem.ty`).
        fn place_prefix_object_volatile(
            &self,
            place: &Place,
            body: &Body,
            prefix_len: usize,
        ) -> Result<bool, CodegenError> {
            let decl = body.locals.get(place.base).ok_or_else(|| {
                CodegenError::Internal(format!("place base local {:?} is missing", place.base))
            })?;
            let mut vol = decl.quals.is_volatile;
            let mut current_ty = decl.ty;
            for proj in place.projection.iter().take(prefix_len) {
                match proj {
                    Projection::Field(idx) => {
                        let (_, field_ty, quals) = self.record_field_info(current_ty, *idx)?;
                        vol |= quals.is_volatile;
                        current_ty = field_ty;
                    }
                    Projection::Index(_) => {
                        current_ty = match self.tcx.get(current_ty) {
                            Ty::Array { elem, .. } => {
                                vol |= Self::array_elem_volatile_pointer_slot(self.tcx, elem);
                                elem.ty
                            }
                            Ty::Ptr(pointee) => {
                                vol = pointee.is_volatile;
                                pointee.ty
                            }
                            _ => return Err(invalid_place_projection("index", current_ty)),
                        };
                    }
                    Projection::Deref => {
                        let Ty::Ptr(pointee) = self.tcx.get(current_ty) else {
                            return Err(invalid_place_projection("dereference", current_ty));
                        };
                        vol = pointee.is_volatile;
                        current_ty = pointee.ty;
                    }
                }
            }
            Ok(vol)
        }

        /// `true` when an array element stores a pointer value in a volatile-qualified slot
        /// (`int *volatile a[]`), but not when only the pointee is volatile (`volatile int *a[]`).
        fn array_elem_volatile_pointer_slot(tcx: &TyCtxt, elem: &Qual) -> bool {
            matches!(tcx.get(elem.ty), Ty::Ptr(_)) && elem.is_volatile
        }

        fn emit_memory_load(
            &self,
            llvm_ty: BasicTypeEnum<'ctx>,
            addr: PointerValue<'ctx>,
            name: &str,
            volatile: bool,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let v = self.builder.build_load(llvm_ty, addr, name).map_err(builder_error)?;
            if volatile {
                let Some(inst) = v.as_instruction_value() else {
                    return Err(CodegenError::Internal(
                        "load result is not an instruction".to_owned(),
                    ));
                };
                inst.set_volatile(true).map_err(|e| CodegenError::Internal(e.to_string()))?;
            }
            Ok(v)
        }

        fn emit_memory_store(
            &self,
            addr: PointerValue<'ctx>,
            value: BasicValueEnum<'ctx>,
            volatile: bool,
        ) -> Result<(), CodegenError> {
            let inst = self.builder.build_store(addr, value).map_err(builder_error)?;
            if volatile {
                inst.set_volatile(true).map_err(|e| CodegenError::Internal(e.to_string()))?;
            }
            Ok(())
        }

        fn build_byte_gep(
            &self,
            ptr: PointerValue<'ctx>,
            offset: u64,
            name: &str,
        ) -> Result<PointerValue<'ctx>, CodegenError> {
            let i8_ty = self.context.i8_type();
            let offset = self.context.i64_type().const_int(offset, false);
            self.build_gep(i8_ty, ptr, &[offset], name)
        }

        fn emit_bitfield_access(
            &self,
            place: &Place,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<Option<BitfieldAccess<'ctx>>, CodegenError> {
            let Some((Projection::Field(field_idx), prefix)) = place.projection.split_last() else {
                return Ok(None);
            };
            let prefix_place = Place { base: place.base, projection: prefix.to_vec() };
            let owner_ty = self.place_ty(&prefix_place, body)?;
            let field = self.record_field_access(owner_ty, *field_idx)?;
            let Some(bit_width) = field.layout.bit_width else {
                return Ok(None);
            };
            if bit_width == 0 {
                return Err(CodegenError::Internal(
                    "zero-width bit-field cannot be accessed as a value".to_owned(),
                ));
            }
            let bit_offset = field.layout.bit_offset.ok_or_else(|| {
                CodegenError::Internal("bit-field layout is missing bit offset".to_owned())
            })?;
            let storage_bits = storage_bits_for_bitfield(field.layout.storage_size)?;
            if bit_offset.checked_add(bit_width).is_none_or(|end| end > storage_bits) {
                return Err(CodegenError::Internal(format!(
                    "bit-field range offset {} width {} exceeds {}-bit storage unit",
                    bit_offset, bit_width, storage_bits
                )));
            }

            let owner_addr = self.emit_place_addr(&prefix_place, locals, body)?;
            let storage_addr =
                self.build_byte_gep(owner_addr, field.layout.offset, "bf.storage")?;
            let storage_ty = self.context.custom_width_int_type(storage_bits);
            let volatile = self.place_is_volatile(place, body)?;
            Ok(Some(BitfieldAccess {
                storage_addr,
                storage_ty,
                field_ty: field.ty,
                bit_offset,
                bit_width,
                storage_bits,
                volatile,
            }))
        }

        fn try_emit_bitfield_load(
            &self,
            place: &Place,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
            let Some(access) = self.emit_bitfield_access(place, locals, body)? else {
                return Ok(None);
            };
            let storage = self
                .emit_memory_load(
                    access.storage_ty.as_basic_type_enum(),
                    access.storage_addr,
                    "bf.load",
                    access.volatile,
                )?
                .into_int_value();
            let shifted = if access.bit_offset == 0 {
                storage
            } else {
                let amount = access.storage_ty.const_int(u64::from(access.bit_offset), false);
                self.builder
                    .build_right_shift(storage, amount, false, "bf.lshr")
                    .map_err(builder_error)?
            };
            let mask = access.storage_ty.const_int(bit_mask(access.bit_width)?, false);
            let masked = if access.bit_width == access.storage_bits {
                shifted
            } else {
                self.builder.build_and(shifted, mask, "bf.mask").map_err(builder_error)?
            };
            let narrow_ty = self.context.custom_width_int_type(access.bit_width);
            let narrow = if access.bit_width == access.storage_bits {
                masked
            } else {
                self.builder
                    .build_int_truncate(masked, narrow_ty, "bf.trunc")
                    .map_err(builder_error)?
            };
            let BasicTypeEnum::IntType(field_ty) = self.type_cx().basic_type_of(access.field_ty)?
            else {
                return Err(type_lowering_error(access.field_ty, "bit-field type is not integer"));
            };
            let signed = self.is_signed_integer_ty(access.field_ty)?;
            let value = self.cast_int_value(narrow, field_ty, signed, "bf.ext")?;
            Ok(Some(value.as_basic_value_enum()))
        }

        fn try_emit_bitfield_store(
            &self,
            place: &Place,
            value: BasicValueEnum<'ctx>,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<bool, CodegenError> {
            let Some(access) = self.emit_bitfield_access(place, locals, body)? else {
                return Ok(false);
            };
            let BasicValueEnum::IntValue(value) = value else {
                return Err(CodegenError::Internal(format!(
                    "bit-field store expected integer value, got {:?}",
                    value.get_type()
                )));
            };
            let value = self.cast_int_value(value, access.storage_ty, false, "bf.store.cast")?;
            let width_mask = bit_mask(access.bit_width)?;
            let value_mask = access.storage_ty.const_int(width_mask, false);
            let value =
                self.builder.build_and(value, value_mask, "bf.value").map_err(builder_error)?;
            let shifted_value = if access.bit_offset == 0 {
                value
            } else {
                let amount = access.storage_ty.const_int(u64::from(access.bit_offset), false);
                self.builder.build_left_shift(value, amount, "bf.shl").map_err(builder_error)?
            };
            let storage = self
                .emit_memory_load(
                    access.storage_ty.as_basic_type_enum(),
                    access.storage_addr,
                    "bf.old",
                    access.volatile,
                )?
                .into_int_value();
            let field_mask = width_mask.checked_shl(access.bit_offset).ok_or_else(|| {
                CodegenError::Internal("bit-field mask shift overflowed".to_owned())
            })?;
            let storage_mask = bit_mask(access.storage_bits)?;
            let clear_mask = access.storage_ty.const_int(storage_mask & !field_mask, false);
            let kept =
                self.builder.build_and(storage, clear_mask, "bf.keep").map_err(builder_error)?;
            let merged =
                self.builder.build_or(kept, shifted_value, "bf.merge").map_err(builder_error)?;
            self.emit_memory_store(
                access.storage_addr,
                merged.as_basic_value_enum(),
                access.volatile,
            )?;
            Ok(true)
        }

        fn cast_int_value(
            &self,
            value: IntValue<'ctx>,
            to_ty: inkwell::types::IntType<'ctx>,
            signed: bool,
            name: &str,
        ) -> Result<IntValue<'ctx>, CodegenError> {
            let from_width = value.get_type().get_bit_width();
            let to_width = to_ty.get_bit_width();
            if from_width == to_width {
                return Ok(value);
            }
            if from_width > to_width {
                return self.builder.build_int_truncate(value, to_ty, name).map_err(builder_error);
            }
            if signed {
                self.builder.build_int_s_extend(value, to_ty, name).map_err(builder_error)
            } else {
                self.builder.build_int_z_extend(value, to_ty, name).map_err(builder_error)
            }
        }

        /// Compute the LLVM pointer for a CFG [`Place`].
        ///
        /// Walks the projection chain and emits `getelementptr` / pointer
        /// loads as needed. The returned `PointerValue` is the address where
        /// the place's value lives.
        ///
        /// `locals` maps each [`Local`] index to the alloca created for it
        /// (populated by the entry-block alloca pass, task 09-10).
        pub fn emit_place_addr(
            &self,
            place: &Place,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<PointerValue<'ctx>, CodegenError> {
            let mut ptr = *locals.get(place.base).ok_or_else(|| {
                CodegenError::Internal(format!("missing LLVM storage for local {:?}", place.base))
            })?;
            let mut current_ty = body
                .locals
                .get(place.base)
                .ok_or_else(|| {
                    CodegenError::Internal(format!("place base local {:?} is missing", place.base))
                })?
                .ty;

            for (i, proj) in place.projection.iter().enumerate() {
                let prefix_object_vol = self.place_prefix_object_volatile(place, body, i)?;
                match proj {
                    Projection::Deref => {
                        let Ty::Ptr(pointee) = self.tcx.get(current_ty) else {
                            return Err(invalid_place_projection("dereference", current_ty));
                        };
                        let ptr_ty = self.context.ptr_type(AddressSpace::default());
                        let v = self.emit_memory_load(
                            ptr_ty.as_basic_type_enum(),
                            ptr,
                            "deref_load",
                            prefix_object_vol,
                        )?;
                        ptr = v.into_pointer_value();
                        current_ty = pointee.ty;
                    }
                    Projection::Field(idx) => {
                        let field = self.record_field_access(current_ty, *idx)?;
                        if field.layout.bit_width.is_some() {
                            return Err(CodegenError::Internal(
                                "cannot take the address of a bit-field".to_owned(),
                            ));
                        }
                        if field.kind == RecordKind::Struct && field.layout.offset != 0 {
                            ptr = self.build_byte_gep(ptr, field.layout.offset, "field_gep")?;
                        }
                        current_ty = field.ty;
                    }
                    Projection::Index(index_op) => {
                        let index_val = match self.emit_operand_value(index_op, locals, body)? {
                            BasicValueEnum::IntValue(value) => value,
                            other => {
                                return Err(CodegenError::Internal(format!(
                                    "place index must be an integer, got {:?}",
                                    other.get_type()
                                )));
                            }
                        };
                        match self.tcx.get(current_ty) {
                            Ty::Array { elem, is_vla: true, .. } => {
                                let elem_ty = self.type_cx().basic_type_of(elem.ty)?;
                                ptr = self.build_gep(elem_ty, ptr, &[index_val], "index_gep")?;
                                current_ty = elem.ty;
                            }
                            Ty::Array { elem, .. } => {
                                let zero = self.context.i32_type().const_zero();
                                let array_ty = self.type_cx().basic_type_of(current_ty)?;
                                ptr =
                                    self.build_gep(array_ty, ptr, &[zero, index_val], "index_gep")?;
                                current_ty = elem.ty;
                            }
                            Ty::Ptr(pointee) => {
                                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                                let base = self
                                    .emit_memory_load(
                                        ptr_ty.as_basic_type_enum(),
                                        ptr,
                                        "index_base_load",
                                        prefix_object_vol,
                                    )?
                                    .into_pointer_value();
                                let elem_ty = self.type_cx().basic_type_of(pointee.ty)?;
                                ptr = self.build_gep(elem_ty, base, &[index_val], "index_gep")?;
                                current_ty = pointee.ty;
                            }
                            _ => return Err(invalid_place_projection("index", current_ty)),
                        }
                    }
                }
            }

            Ok(ptr)
        }

        /// Load the value of a CFG [`Operand`] as an LLVM `BasicValueEnum`.
        ///
        /// - `Operand::Copy(place)` / `Operand::Move(place)` → address + load.
        /// - `Operand::Const(c)` → materialise the constant.
        pub fn emit_operand_value(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match operand {
                Operand::Copy(place) | Operand::Move(place) => {
                    if let Some(value) = self.try_emit_bitfield_load(place, locals, body)? {
                        return Ok(value);
                    }
                    let addr = self.emit_place_addr(place, locals, body)?;
                    let ty = self.place_ty(place, body)?;
                    let llvm_ty = self.type_cx().basic_type_of(ty)?;
                    let volatile = self.place_is_volatile(place, body)?;
                    self.emit_memory_load(llvm_ty, addr, "load", volatile)
                }
                Operand::Const(c) => self.emit_const(c),
            }
        }

        /// Emit the subset of CFG rvalues whose memory semantics are owned by this task.
        ///
        /// Later lowering tasks extend this entry point for arithmetic, casts, calls,
        /// and aggregate intrinsics; `AddressOf` is handled here so address formation
        /// stays centralized with place projection.
        pub fn emit_rvalue_value(
            &self,
            rvalue: &Rvalue,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match rvalue {
                Rvalue::Use(operand) => self.emit_operand_value(operand, locals, body),
                Rvalue::BinaryOp(op, lhs, rhs) => self.emit_binop(*op, lhs, rhs, locals, body),
                Rvalue::UnaryOp(op, operand) => self.emit_unop(*op, operand, locals, body),
                Rvalue::Cast { op, to, kind } => self.emit_cast(op, *to, *kind, locals, body),
                Rvalue::ComplexFromReal { real, to } => {
                    self.emit_complex_from_real(real, *to, locals, body)
                }
                Rvalue::RealFromComplex { complex, to } => {
                    self.emit_real_from_complex(complex, *to, locals, body)
                }
                Rvalue::AddressOf(place) => {
                    Ok(self.emit_place_addr(place, locals, body)?.as_basic_value_enum())
                }
                Rvalue::LoadGlobal { def, ty } => self.emit_global_object_load(*def, *ty),
                Rvalue::Len(place) => {
                    let base = place.base;
                    let decl = body.locals.get(base).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "Rvalue::Len base local {base:?} is missing from body"
                        ))
                    })?;
                    let len_local = decl.vla_len.ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "Rvalue::Len local {base:?} is missing vla_len"
                        ))
                    })?;
                    let len_ptr = *locals.get(len_local).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "missing LLVM storage for vla_len local {len_local:?}"
                        ))
                    })?;
                    let len_llvm_ty = self.type_cx().basic_type_of(self.tcx.ulong)?;
                    self.builder.build_load(len_llvm_ty, len_ptr, "len").map_err(builder_error)
                }
                Rvalue::BuiltinVaArg { ap, ty } => self.emit_va_arg(ap, *ty, locals, body),
            }
        }

        fn emit_va_arg(
            &self,
            ap: &Operand,
            ty: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let ap_place = match ap {
                Operand::Copy(place) | Operand::Move(place) => place,
                Operand::Const(_) => {
                    return Err(CodegenError::Internal(
                        "va_arg operand must be an addressable place".to_owned(),
                    ));
                }
            };
            let ap_ptr = self.emit_place_addr(ap_place, locals, body)?;
            let llvm_ty = self.type_cx().basic_type_of(ty)?;
            self.builder.build_va_arg(ap_ptr, llvm_ty, "va_arg").map_err(builder_error)
        }

        fn emit_complex_from_real(
            &self,
            real: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let Ty::Complex(kind) = self.tcx.get(to) else {
                return Err(type_lowering_error(to, "real-to-complex target is not complex"));
            };
            let real = self.emit_real_operand_as_float_kind(real, *kind, locals, body)?;
            let imag = real.get_type().const_zero();
            self.build_complex_value(to, real, imag)
        }

        fn emit_real_from_complex(
            &self,
            complex: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let from_ty = self.operand_ty(complex, body)?;
            let Ty::Complex(kind) = self.tcx.get(from_ty) else {
                return Err(type_lowering_error(from_ty, "complex-to-real operand is not complex"));
            };
            let complex = self.emit_complex_operand(complex, locals, body)?;
            let real = self.extract_complex_part(complex, 0, "complex.real")?;
            self.emit_float_value_as_real_ty(real, self.real_ty_for_float_kind(*kind), to)
        }

        fn emit_cast(
            &self,
            operand: &Operand,
            to: TyId,
            kind: CastKind,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match kind {
                CastKind::IntToInt => self.emit_int_to_int_cast(operand, to, locals, body),
                CastKind::IntToFloat => self.emit_int_to_float_cast(operand, to, locals, body),
                CastKind::FloatToInt => self.emit_float_to_int_cast(operand, to, locals, body),
                CastKind::FloatToFloat => self.emit_float_to_float_cast(operand, to, locals, body),
                CastKind::PtrToPtr => self.emit_ptr_to_ptr_cast(operand, to, locals, body),
                CastKind::PtrToInt => self.emit_ptr_to_int_cast(operand, to, locals, body),
                CastKind::IntToPtr => self.emit_int_to_ptr_cast(operand, to, locals, body),
            }
        }

        fn emit_int_to_int_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let from_ty = self.operand_ty(operand, body)?;
            let value = self.emit_int_operand(operand, locals, body)?;
            let BasicTypeEnum::IntType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "integer cast target is not an integer"));
            };

            if self.is_bool_ty(to) {
                if value.get_type().get_bit_width() == 1 {
                    return Ok(value.as_basic_value_enum());
                }
                let bool_value = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        value,
                        value.get_type().const_zero(),
                        "tobool",
                    )
                    .map_err(builder_error)?;
                return Ok(bool_value.as_basic_value_enum());
            }

            let from_width = value.get_type().get_bit_width();
            let to_width = to_ty.get_bit_width();
            let cast = if from_width == to_width {
                value
            } else if from_width > to_width {
                self.builder.build_int_truncate(value, to_ty, "trunc").map_err(builder_error)?
            } else if self.is_signed_integer_ty(from_ty)? {
                self.builder.build_int_s_extend(value, to_ty, "sext").map_err(builder_error)?
            } else {
                self.builder.build_int_z_extend(value, to_ty, "zext").map_err(builder_error)?
            };
            Ok(cast.as_basic_value_enum())
        }

        fn emit_int_to_float_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let from_ty = self.operand_ty(operand, body)?;
            let value = self.emit_int_operand(operand, locals, body)?;
            let BasicTypeEnum::FloatType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "integer-to-float target is not floating"));
            };
            let cast = if self.is_signed_integer_ty(from_ty)? {
                self.builder
                    .build_signed_int_to_float(value, to_ty, "sitofp")
                    .map_err(builder_error)?
            } else {
                self.builder
                    .build_unsigned_int_to_float(value, to_ty, "uitofp")
                    .map_err(builder_error)?
            };
            Ok(cast.as_basic_value_enum())
        }

        fn emit_float_to_int_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let value = self.emit_float_operand(operand, locals, body)?;
            let BasicTypeEnum::IntType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "float-to-integer target is not an integer"));
            };

            if self.is_bool_ty(to) {
                let bool_value = self
                    .builder
                    .build_float_compare(
                        FloatPredicate::ONE,
                        value,
                        value.get_type().const_zero(),
                        "tobool",
                    )
                    .map_err(builder_error)?;
                return Ok(bool_value.as_basic_value_enum());
            }

            let cast = if self.is_signed_integer_ty(to)? {
                self.builder
                    .build_float_to_signed_int(value, to_ty, "fptosi")
                    .map_err(builder_error)?
            } else {
                self.builder
                    .build_float_to_unsigned_int(value, to_ty, "fptoui")
                    .map_err(builder_error)?
            };
            Ok(cast.as_basic_value_enum())
        }

        fn emit_float_to_float_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let from_ty = self.operand_ty(operand, body)?;
            let value = self.emit_float_operand(operand, locals, body)?;
            let BasicTypeEnum::FloatType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "float cast target is not floating"));
            };
            let from_width = self.float_ty_width(from_ty)?;
            let to_width = self.float_ty_width(to)?;
            let cast = if from_width == to_width {
                value
            } else if from_width < to_width {
                self.builder.build_float_ext(value, to_ty, "fpext").map_err(builder_error)?
            } else {
                self.builder.build_float_trunc(value, to_ty, "fptrunc").map_err(builder_error)?
            };
            Ok(cast.as_basic_value_enum())
        }

        fn emit_ptr_to_ptr_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            if !matches!(self.tcx.get(to), Ty::Ptr(_)) {
                return Err(type_lowering_error(to, "pointer cast target is not a pointer"));
            }
            Ok(self.emit_pointer_operand(operand, locals, body)?.as_basic_value_enum())
        }

        fn emit_ptr_to_int_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let value = self.emit_pointer_operand(operand, locals, body)?;
            let BasicTypeEnum::IntType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "pointer-to-integer target is not an integer"));
            };
            if self.is_bool_ty(to) {
                let bool_value =
                    self.builder.build_is_not_null(value, "tobool").map_err(builder_error)?;
                return Ok(bool_value.as_basic_value_enum());
            }
            self.builder
                .build_ptr_to_int(value, to_ty, "ptrtoint")
                .map(|value| value.as_basic_value_enum())
                .map_err(builder_error)
        }

        fn emit_int_to_ptr_cast(
            &self,
            operand: &Operand,
            to: TyId,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let value = self.emit_int_operand(operand, locals, body)?;
            let BasicTypeEnum::PointerType(to_ty) = self.type_cx().basic_type_of(to)? else {
                return Err(type_lowering_error(to, "integer-to-pointer target is not a pointer"));
            };
            self.builder
                .build_int_to_ptr(value, to_ty, "inttoptr")
                .map(|value| value.as_basic_value_enum())
                .map_err(builder_error)
        }

        fn emit_real_operand_as_float_kind(
            &self,
            operand: &Operand,
            kind: FloatKind,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<FloatValue<'ctx>, CodegenError> {
            let from_ty = self.operand_ty(operand, body)?;
            let BasicTypeEnum::FloatType(to_ty) =
                self.type_cx().basic_type_of(self.real_ty_for_float_kind(kind))?
            else {
                return Err(CodegenError::Internal(
                    "complex element type is not floating".to_owned(),
                ));
            };

            match self.emit_operand_value(operand, locals, body)? {
                BasicValueEnum::FloatValue(value) => {
                    let from_width = self.float_ty_width(from_ty)?;
                    let to_width = self.float_ty_width(self.real_ty_for_float_kind(kind))?;
                    if from_width == to_width {
                        Ok(value)
                    } else if from_width < to_width {
                        self.builder.build_float_ext(value, to_ty, "fpext").map_err(builder_error)
                    } else {
                        self.builder
                            .build_float_trunc(value, to_ty, "fptrunc")
                            .map_err(builder_error)
                    }
                }
                BasicValueEnum::IntValue(value) => {
                    if self.is_signed_integer_ty(from_ty)? {
                        self.builder
                            .build_signed_int_to_float(value, to_ty, "sitofp")
                            .map_err(builder_error)
                    } else {
                        self.builder
                            .build_unsigned_int_to_float(value, to_ty, "uitofp")
                            .map_err(builder_error)
                    }
                }
                other => Err(CodegenError::Internal(format!(
                    "expected real operand for complex conversion, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn emit_float_value_as_real_ty(
            &self,
            value: FloatValue<'ctx>,
            from_ty: TyId,
            to: TyId,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match self.tcx.get(to) {
                Ty::Float(_) => {
                    let BasicTypeEnum::FloatType(to_ty) = self.type_cx().basic_type_of(to)? else {
                        return Err(type_lowering_error(
                            to,
                            "complex-to-real target is not floating",
                        ));
                    };
                    let from_width = self.float_ty_width(from_ty)?;
                    let to_width = self.float_ty_width(to)?;
                    let cast = if from_width == to_width {
                        value
                    } else if from_width < to_width {
                        self.builder
                            .build_float_ext(value, to_ty, "fpext")
                            .map_err(builder_error)?
                    } else {
                        self.builder
                            .build_float_trunc(value, to_ty, "fptrunc")
                            .map_err(builder_error)?
                    };
                    Ok(cast.as_basic_value_enum())
                }
                Ty::Int { .. } | Ty::Enum(_) => {
                    let BasicTypeEnum::IntType(to_ty) = self.type_cx().basic_type_of(to)? else {
                        return Err(type_lowering_error(
                            to,
                            "complex-to-real target is not integer",
                        ));
                    };
                    if self.is_bool_ty(to) {
                        let bool_value = self
                            .builder
                            .build_float_compare(
                                FloatPredicate::ONE,
                                value,
                                value.get_type().const_zero(),
                                "tobool",
                            )
                            .map_err(builder_error)?;
                        return Ok(bool_value.as_basic_value_enum());
                    }
                    let cast = if self.is_signed_integer_ty(to)? {
                        self.builder
                            .build_float_to_signed_int(value, to_ty, "fptosi")
                            .map_err(builder_error)?
                    } else {
                        self.builder
                            .build_float_to_unsigned_int(value, to_ty, "fptoui")
                            .map_err(builder_error)?
                    };
                    Ok(cast.as_basic_value_enum())
                }
                _ => Err(type_lowering_error(to, "complex-to-real target is not a real type")),
            }
        }

        fn build_complex_value(
            &self,
            ty: TyId,
            real: FloatValue<'ctx>,
            imag: FloatValue<'ctx>,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let BasicTypeEnum::StructType(ty) = self.type_cx().basic_type_of(ty)? else {
                return Err(type_lowering_error(ty, "complex type did not lower to a struct"));
            };
            let aggregate = ty.get_undef();
            let aggregate = self
                .builder
                .build_insert_value(aggregate, real, 0, "complex.real")
                .map_err(builder_error)?
                .into_struct_value();
            let aggregate = self
                .builder
                .build_insert_value(aggregate, imag, 1, "complex.imag")
                .map_err(builder_error)?
                .into_struct_value();
            Ok(aggregate.as_basic_value_enum())
        }

        fn emit_complex_operand(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<StructValue<'ctx>, CodegenError> {
            match self.emit_operand_value(operand, locals, body)? {
                BasicValueEnum::StructValue(value) => Ok(value),
                other => Err(CodegenError::Internal(format!(
                    "expected complex operand, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn extract_complex_part(
            &self,
            complex: StructValue<'ctx>,
            index: u32,
            name: &str,
        ) -> Result<FloatValue<'ctx>, CodegenError> {
            match self.builder.build_extract_value(complex, index, name).map_err(builder_error)? {
                BasicValueEnum::FloatValue(value) => Ok(value),
                other => Err(CodegenError::Internal(format!(
                    "complex component is not floating, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn real_ty_for_float_kind(&self, kind: FloatKind) -> TyId {
            match kind {
                FloatKind::F32 => self.tcx.float,
                FloatKind::F64 => self.tcx.double,
                FloatKind::F80 => self.tcx.long_double,
            }
        }

        fn emit_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match op {
                BinOp::Add => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::Sub => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::Mul => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::SDiv => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::UDiv => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::SRem => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::URem => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::FDiv => self.emit_float_or_complex_binop(op, lhs, rhs, locals, body),
                BinOp::Shl => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::AShr => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::LShr => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::BitAnd => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::BitXor => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::BitOr => self.emit_int_binop(op, lhs, rhs, locals, body),
                BinOp::Eq => self.emit_eq_ne_binop(op, lhs, rhs, locals, body),
                BinOp::Ne => self.emit_eq_ne_binop(op, lhs, rhs, locals, body),
                BinOp::SLt => self.emit_int_compare(IntPredicate::SLT, lhs, rhs, locals, body),
                BinOp::SLe => self.emit_int_compare(IntPredicate::SLE, lhs, rhs, locals, body),
                BinOp::SGt => self.emit_int_compare(IntPredicate::SGT, lhs, rhs, locals, body),
                BinOp::SGe => self.emit_int_compare(IntPredicate::SGE, lhs, rhs, locals, body),
                BinOp::ULt => self.emit_int_compare(IntPredicate::ULT, lhs, rhs, locals, body),
                BinOp::ULe => self.emit_int_compare(IntPredicate::ULE, lhs, rhs, locals, body),
                BinOp::UGt => self.emit_int_compare(IntPredicate::UGT, lhs, rhs, locals, body),
                BinOp::UGe => self.emit_int_compare(IntPredicate::UGE, lhs, rhs, locals, body),
                BinOp::FLt => self.emit_float_compare(FloatPredicate::OLT, lhs, rhs, locals, body),
                BinOp::FLe => self.emit_float_compare(FloatPredicate::OLE, lhs, rhs, locals, body),
                BinOp::FGt => self.emit_float_compare(FloatPredicate::OGT, lhs, rhs, locals, body),
                BinOp::FGe => self.emit_float_compare(FloatPredicate::OGE, lhs, rhs, locals, body),
                BinOp::FAdd => self.emit_float_or_complex_binop(op, lhs, rhs, locals, body),
                BinOp::FSub => self.emit_float_or_complex_binop(op, lhs, rhs, locals, body),
                BinOp::FMul => self.emit_float_or_complex_binop(op, lhs, rhs, locals, body),
                BinOp::PtrAdd => self.emit_ptr_add(lhs, rhs, locals, body),
                BinOp::PtrSub => self.emit_ptr_sub(lhs, rhs, locals, body),
                BinOp::PtrDiff => self.emit_ptr_diff(lhs, rhs, locals, body),
            }
        }

        fn emit_unop(
            &self,
            op: UnOp,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match op {
                UnOp::Neg => {
                    let value = self.emit_int_operand(operand, locals, body)?;
                    self.builder
                        .build_int_neg(value, "neg")
                        .map(|value| value.as_basic_value_enum())
                        .map_err(builder_error)
                }
                UnOp::FNeg => {
                    if matches!(self.tcx.get(self.operand_ty(operand, body)?), Ty::Complex(_)) {
                        self.emit_complex_neg(operand, locals, body)
                    } else {
                        let value = self.emit_float_operand(operand, locals, body)?;
                        self.builder
                            .build_float_neg(value, "fneg")
                            .map(|value| value.as_basic_value_enum())
                            .map_err(builder_error)
                    }
                }
                UnOp::BitNot => {
                    let value = self.emit_int_operand(operand, locals, body)?;
                    self.builder
                        .build_not(value, "not")
                        .map(|value| value.as_basic_value_enum())
                        .map_err(builder_error)
                }
                UnOp::LogNot => {
                    let value = self.emit_operand_value(operand, locals, body)?;
                    let bool_value = match value {
                        BasicValueEnum::IntValue(value) => self
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                value,
                                value.get_type().const_zero(),
                                "lnot",
                            )
                            .map_err(builder_error)?,
                        BasicValueEnum::FloatValue(value) => self
                            .builder
                            .build_float_compare(
                                FloatPredicate::OEQ,
                                value,
                                value.get_type().const_zero(),
                                "lnot",
                            )
                            .map_err(builder_error)?,
                        BasicValueEnum::PointerValue(value) => {
                            self.builder.build_is_null(value, "lnot").map_err(builder_error)?
                        }
                        other => {
                            return Err(CodegenError::Internal(format!(
                                "logical not requires scalar operand, got {:?}",
                                other.get_type()
                            )));
                        }
                    };
                    self.bool_to_c_int(bool_value)
                }
            }
        }

        fn emit_int_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs = self.emit_int_operand(lhs, locals, body)?;
            let rhs = self.emit_int_operand(rhs, locals, body)?;
            let value = match op {
                BinOp::Add => self.builder.build_int_add(lhs, rhs, "add"),
                BinOp::Sub => self.builder.build_int_sub(lhs, rhs, "sub"),
                BinOp::Mul => self.builder.build_int_mul(lhs, rhs, "mul"),
                BinOp::SDiv => self.builder.build_int_signed_div(lhs, rhs, "sdiv"),
                BinOp::UDiv => self.builder.build_int_unsigned_div(lhs, rhs, "udiv"),
                BinOp::SRem => self.builder.build_int_signed_rem(lhs, rhs, "srem"),
                BinOp::URem => self.builder.build_int_unsigned_rem(lhs, rhs, "urem"),
                BinOp::Shl => self.builder.build_left_shift(lhs, rhs, "shl"),
                BinOp::AShr => self.builder.build_right_shift(lhs, rhs, true, "ashr"),
                BinOp::LShr => self.builder.build_right_shift(lhs, rhs, false, "lshr"),
                BinOp::BitAnd => self.builder.build_and(lhs, rhs, "and"),
                BinOp::BitXor => self.builder.build_xor(lhs, rhs, "xor"),
                BinOp::BitOr => self.builder.build_or(lhs, rhs, "or"),
                BinOp::FDiv
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::SLt
                | BinOp::SLe
                | BinOp::SGt
                | BinOp::SGe
                | BinOp::ULt
                | BinOp::ULe
                | BinOp::UGt
                | BinOp::UGe
                | BinOp::FLt
                | BinOp::FLe
                | BinOp::FGt
                | BinOp::FGe
                | BinOp::FAdd
                | BinOp::FSub
                | BinOp::FMul
                | BinOp::PtrAdd
                | BinOp::PtrSub
                | BinOp::PtrDiff => unreachable!("non-integer binop routed to emit_int_binop"),
            }
            .map_err(builder_error)?;
            Ok(value.as_basic_value_enum())
        }

        fn emit_float_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs = self.emit_float_operand(lhs, locals, body)?;
            let rhs = self.emit_float_operand(rhs, locals, body)?;
            let value = match op {
                BinOp::FAdd => self.builder.build_float_add(lhs, rhs, "fadd"),
                BinOp::FSub => self.builder.build_float_sub(lhs, rhs, "fsub"),
                BinOp::FMul => self.builder.build_float_mul(lhs, rhs, "fmul"),
                BinOp::FDiv => self.builder.build_float_div(lhs, rhs, "fdiv"),
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::SDiv
                | BinOp::UDiv
                | BinOp::SRem
                | BinOp::URem
                | BinOp::Shl
                | BinOp::AShr
                | BinOp::LShr
                | BinOp::BitAnd
                | BinOp::BitXor
                | BinOp::BitOr
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::SLt
                | BinOp::SLe
                | BinOp::SGt
                | BinOp::SGe
                | BinOp::ULt
                | BinOp::ULe
                | BinOp::UGt
                | BinOp::UGe
                | BinOp::FLt
                | BinOp::FLe
                | BinOp::FGt
                | BinOp::FGe
                | BinOp::PtrAdd
                | BinOp::PtrSub
                | BinOp::PtrDiff => unreachable!("non-floating binop routed to emit_float_binop"),
            }
            .map_err(builder_error)?;
            Ok(value.as_basic_value_enum())
        }

        fn emit_float_or_complex_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            if matches!(self.tcx.get(self.operand_ty(lhs, body)?), Ty::Complex(_))
                || matches!(self.tcx.get(self.operand_ty(rhs, body)?), Ty::Complex(_))
            {
                self.emit_complex_binop(op, lhs, rhs, locals, body)
            } else {
                self.emit_float_binop(op, lhs, rhs, locals, body)
            }
        }

        fn emit_complex_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs_ty = self.operand_ty(lhs, body)?;
            let rhs_ty = self.operand_ty(rhs, body)?;
            let Ty::Complex(_) = self.tcx.get(lhs_ty) else {
                return Err(type_lowering_error(lhs_ty, "complex lhs is not complex"));
            };
            let Ty::Complex(_) = self.tcx.get(rhs_ty) else {
                return Err(type_lowering_error(rhs_ty, "complex rhs is not complex"));
            };
            if lhs_ty != rhs_ty {
                return Err(CodegenError::Internal(format!(
                    "complex operands were not coerced to the same type: {:?} and {:?}",
                    lhs_ty, rhs_ty
                )));
            }

            let lhs = self.emit_complex_operand(lhs, locals, body)?;
            let rhs = self.emit_complex_operand(rhs, locals, body)?;
            let lhs_real = self.extract_complex_part(lhs, 0, "complex.l.real")?;
            let lhs_imag = self.extract_complex_part(lhs, 1, "complex.l.imag")?;
            let rhs_real = self.extract_complex_part(rhs, 0, "complex.r.real")?;
            let rhs_imag = self.extract_complex_part(rhs, 1, "complex.r.imag")?;

            let (real, imag) = match op {
                BinOp::FAdd => (
                    self.builder
                        .build_float_add(lhs_real, rhs_real, "complex.add.real")
                        .map_err(builder_error)?,
                    self.builder
                        .build_float_add(lhs_imag, rhs_imag, "complex.add.imag")
                        .map_err(builder_error)?,
                ),
                BinOp::FSub => (
                    self.builder
                        .build_float_sub(lhs_real, rhs_real, "complex.sub.real")
                        .map_err(builder_error)?,
                    self.builder
                        .build_float_sub(lhs_imag, rhs_imag, "complex.sub.imag")
                        .map_err(builder_error)?,
                ),
                BinOp::FMul => {
                    let real_l = self
                        .builder
                        .build_float_mul(lhs_real, rhs_real, "complex.mul.rr")
                        .map_err(builder_error)?;
                    let real_r = self
                        .builder
                        .build_float_mul(lhs_imag, rhs_imag, "complex.mul.ii")
                        .map_err(builder_error)?;
                    let imag_l = self
                        .builder
                        .build_float_mul(lhs_real, rhs_imag, "complex.mul.ri")
                        .map_err(builder_error)?;
                    let imag_r = self
                        .builder
                        .build_float_mul(lhs_imag, rhs_real, "complex.mul.ir")
                        .map_err(builder_error)?;
                    (
                        self.builder
                            .build_float_sub(real_l, real_r, "complex.mul.real")
                            .map_err(builder_error)?,
                        self.builder
                            .build_float_add(imag_l, imag_r, "complex.mul.imag")
                            .map_err(builder_error)?,
                    )
                }
                BinOp::FDiv => {
                    let denom_real = self
                        .builder
                        .build_float_mul(rhs_real, rhs_real, "complex.div.rr")
                        .map_err(builder_error)?;
                    let denom_imag = self
                        .builder
                        .build_float_mul(rhs_imag, rhs_imag, "complex.div.ii")
                        .map_err(builder_error)?;
                    let denom = self
                        .builder
                        .build_float_add(denom_real, denom_imag, "complex.div.denom")
                        .map_err(builder_error)?;
                    let real_l = self
                        .builder
                        .build_float_mul(lhs_real, rhs_real, "complex.div.rr.num")
                        .map_err(builder_error)?;
                    let real_r = self
                        .builder
                        .build_float_mul(lhs_imag, rhs_imag, "complex.div.ii.num")
                        .map_err(builder_error)?;
                    let imag_l = self
                        .builder
                        .build_float_mul(lhs_imag, rhs_real, "complex.div.ir.num")
                        .map_err(builder_error)?;
                    let imag_r = self
                        .builder
                        .build_float_mul(lhs_real, rhs_imag, "complex.div.ri.num")
                        .map_err(builder_error)?;
                    let real_num = self
                        .builder
                        .build_float_add(real_l, real_r, "complex.div.real.num")
                        .map_err(builder_error)?;
                    let imag_num = self
                        .builder
                        .build_float_sub(imag_l, imag_r, "complex.div.imag.num")
                        .map_err(builder_error)?;
                    (
                        self.builder
                            .build_float_div(real_num, denom, "complex.div.real")
                            .map_err(builder_error)?,
                        self.builder
                            .build_float_div(imag_num, denom, "complex.div.imag")
                            .map_err(builder_error)?,
                    )
                }
                _ => unreachable!("non-complex arithmetic routed to emit_complex_binop"),
            };

            self.build_complex_value(lhs_ty, real, imag)
        }

        fn emit_complex_neg(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let ty = self.operand_ty(operand, body)?;
            let Ty::Complex(_) = self.tcx.get(ty) else {
                return Err(type_lowering_error(ty, "complex negation operand is not complex"));
            };
            let value = self.emit_complex_operand(operand, locals, body)?;
            let real = self.extract_complex_part(value, 0, "complex.neg.real.in")?;
            let imag = self.extract_complex_part(value, 1, "complex.neg.imag.in")?;
            let real =
                self.builder.build_float_neg(real, "complex.neg.real").map_err(builder_error)?;
            let imag =
                self.builder.build_float_neg(imag, "complex.neg.imag").map_err(builder_error)?;
            self.build_complex_value(ty, real, imag)
        }

        fn emit_eq_ne_binop(
            &self,
            op: BinOp,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs = self.emit_operand_value(lhs, locals, body)?;
            let rhs = self.emit_operand_value(rhs, locals, body)?;
            match (lhs, rhs) {
                (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => {
                    let predicate = match op {
                        BinOp::Eq => IntPredicate::EQ,
                        BinOp::Ne => IntPredicate::NE,
                        _ => unreachable!("non-equality op routed to emit_eq_ne_binop"),
                    };
                    self.emit_int_compare_values(predicate, lhs, rhs)
                }
                (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) => {
                    let predicate = match op {
                        BinOp::Eq => FloatPredicate::OEQ,
                        BinOp::Ne => FloatPredicate::ONE,
                        _ => unreachable!("non-equality op routed to emit_eq_ne_binop"),
                    };
                    self.emit_float_compare_values(predicate, lhs, rhs)
                }
                (BasicValueEnum::PointerValue(lhs), BasicValueEnum::PointerValue(rhs)) => {
                    let int_ty = self.context.i64_type();
                    let lhs = self
                        .builder
                        .build_ptr_to_int(lhs, int_ty, "ptreq.l")
                        .map_err(builder_error)?;
                    let rhs = self
                        .builder
                        .build_ptr_to_int(rhs, int_ty, "ptreq.r")
                        .map_err(builder_error)?;
                    let predicate = match op {
                        BinOp::Eq => IntPredicate::EQ,
                        BinOp::Ne => IntPredicate::NE,
                        _ => unreachable!("non-equality op routed to emit_eq_ne_binop"),
                    };
                    self.emit_int_compare_values(predicate, lhs, rhs)
                }
                (lhs, rhs) => Err(CodegenError::Internal(format!(
                    "equality operands have incompatible LLVM types {:?} and {:?}",
                    lhs.get_type(),
                    rhs.get_type()
                ))),
            }
        }

        fn emit_int_compare(
            &self,
            predicate: IntPredicate,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs = self.emit_int_operand(lhs, locals, body)?;
            let rhs = self.emit_int_operand(rhs, locals, body)?;
            self.emit_int_compare_values(predicate, lhs, rhs)
        }

        fn emit_int_compare_values(
            &self,
            predicate: IntPredicate,
            lhs: IntValue<'ctx>,
            rhs: IntValue<'ctx>,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let value = self
                .builder
                .build_int_compare(predicate, lhs, rhs, "icmp")
                .map_err(builder_error)?;
            self.bool_to_c_int(value)
        }

        fn emit_float_compare(
            &self,
            predicate: FloatPredicate,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs = self.emit_float_operand(lhs, locals, body)?;
            let rhs = self.emit_float_operand(rhs, locals, body)?;
            self.emit_float_compare_values(predicate, lhs, rhs)
        }

        fn emit_float_compare_values(
            &self,
            predicate: FloatPredicate,
            lhs: FloatValue<'ctx>,
            rhs: FloatValue<'ctx>,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let value = self
                .builder
                .build_float_compare(predicate, lhs, rhs, "fcmp")
                .map_err(builder_error)?;
            self.bool_to_c_int(value)
        }

        fn emit_ptr_add(
            &self,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs_ty = self.operand_ty(lhs, body)?;
            let rhs_ty = self.operand_ty(rhs, body)?;
            match (self.tcx.get(lhs_ty), self.tcx.get(rhs_ty)) {
                (Ty::Ptr(pointee), Ty::Int { .. }) => {
                    let ptr = self.emit_pointer_operand(lhs, locals, body)?;
                    let index = self.emit_int_operand(rhs, locals, body)?;
                    let elem_ty = self.type_cx().basic_type_of(pointee.ty)?;
                    Ok(self.build_gep(elem_ty, ptr, &[index], "ptradd")?.as_basic_value_enum())
                }
                (Ty::Int { .. }, Ty::Ptr(pointee)) => {
                    let ptr = self.emit_pointer_operand(rhs, locals, body)?;
                    let index = self.emit_int_operand(lhs, locals, body)?;
                    let elem_ty = self.type_cx().basic_type_of(pointee.ty)?;
                    Ok(self.build_gep(elem_ty, ptr, &[index], "ptradd")?.as_basic_value_enum())
                }
                _ => Err(CodegenError::Internal(format!(
                    "PtrAdd requires pointer and integer operands, got {:?} and {:?}",
                    lhs_ty, rhs_ty
                ))),
            }
        }

        fn emit_ptr_sub(
            &self,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs_ty = self.operand_ty(lhs, body)?;
            let Ty::Ptr(pointee) = self.tcx.get(lhs_ty) else {
                return Err(CodegenError::Internal(format!(
                    "PtrSub left operand must be a pointer, got {:?}",
                    lhs_ty
                )));
            };
            let ptr = self.emit_pointer_operand(lhs, locals, body)?;
            let index = self.emit_int_operand(rhs, locals, body)?;
            let neg_index =
                self.builder.build_int_neg(index, "ptrsub.neg").map_err(builder_error)?;
            let elem_ty = self.type_cx().basic_type_of(pointee.ty)?;
            Ok(self.build_gep(elem_ty, ptr, &[neg_index], "ptrsub")?.as_basic_value_enum())
        }

        fn emit_ptr_diff(
            &self,
            lhs: &Operand,
            rhs: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let lhs_ty = self.operand_ty(lhs, body)?;
            let Ty::Ptr(pointee) = self.tcx.get(lhs_ty) else {
                return Err(CodegenError::Internal(format!(
                    "PtrDiff operands must be pointers, got {:?}",
                    lhs_ty
                )));
            };
            let ptr_ty = self.operand_ty(rhs, body)?;
            if !matches!(self.tcx.get(ptr_ty), Ty::Ptr(_)) {
                return Err(CodegenError::Internal(format!(
                    "PtrDiff right operand must be a pointer, got {:?}",
                    ptr_ty
                )));
            }
            let elem_layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                .layout_of(pointee.ty)
                .map_err(|err| type_lowering_error(pointee.ty, err.to_string()))?;
            if elem_layout.size == 0 {
                return Err(type_lowering_error(
                    pointee.ty,
                    "pointer difference element has zero size",
                ));
            }
            let int_ty = self.context.i64_type();
            let lhs_ptr = self.emit_pointer_operand(lhs, locals, body)?;
            let rhs_ptr = self.emit_pointer_operand(rhs, locals, body)?;
            let lhs_int = self
                .builder
                .build_ptr_to_int(lhs_ptr, int_ty, "ptrdiff.l")
                .map_err(builder_error)?;
            let rhs_int = self
                .builder
                .build_ptr_to_int(rhs_ptr, int_ty, "ptrdiff.r")
                .map_err(builder_error)?;
            let byte_delta = self
                .builder
                .build_int_sub(lhs_int, rhs_int, "ptrdiff.bytes")
                .map_err(builder_error)?;
            let elem_size = int_ty.const_int(elem_layout.size, false);
            let diff = self
                .builder
                .build_int_signed_div(byte_delta, elem_size, "ptrdiff")
                .map_err(builder_error)?;
            Ok(diff.as_basic_value_enum())
        }

        fn bool_to_c_int(
            &self,
            value: IntValue<'ctx>,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            self.builder
                .build_int_z_extend(value, self.context.i32_type(), "booltoint")
                .map(|value| value.as_basic_value_enum())
                .map_err(builder_error)
        }

        fn emit_int_operand(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<IntValue<'ctx>, CodegenError> {
            match self.emit_operand_value(operand, locals, body)? {
                BasicValueEnum::IntValue(value) => Ok(value),
                other => Err(CodegenError::Internal(format!(
                    "expected integer operand, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn emit_float_operand(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<FloatValue<'ctx>, CodegenError> {
            match self.emit_operand_value(operand, locals, body)? {
                BasicValueEnum::FloatValue(value) => Ok(value),
                other => Err(CodegenError::Internal(format!(
                    "expected floating operand, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn emit_pointer_operand(
            &self,
            operand: &Operand,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<PointerValue<'ctx>, CodegenError> {
            match self.emit_operand_value(operand, locals, body)? {
                BasicValueEnum::PointerValue(value) => Ok(value),
                other => Err(CodegenError::Internal(format!(
                    "expected pointer operand, got {:?}",
                    other.get_type()
                ))),
            }
        }

        fn try_emit_aggregate_assign(
            &self,
            place: &Place,
            rvalue: &Rvalue,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<bool, CodegenError> {
            let dest_ty = self.place_ty(place, body)?;
            if !self.is_memory_aggregate_ty(dest_ty) {
                return Ok(false);
            }

            let dest = self.emit_place_addr(place, locals, body)?;
            match rvalue {
                Rvalue::Use(Operand::Copy(src) | Operand::Move(src)) => {
                    let src_ty = self.place_ty(src, body)?;
                    let src_addr = self.emit_place_addr(src, locals, body)?;
                    let dest_volatile = self.place_is_volatile(place, body)?;
                    let src_volatile = self.place_is_volatile(src, body)?;
                    if dest_volatile || src_volatile {
                        let dest_layout = self.memory_layout(dest_ty)?;
                        let src_layout = self.memory_layout(src_ty)?;
                        if dest_layout.size != src_layout.size {
                            return Err(CodegenError::Internal(format!(
                                "aggregate copy size mismatch: dest {} bytes, source {} bytes",
                                dest_layout.size, src_layout.size
                            )));
                        }
                        self.emit_volatile_byte_range_copy(dest, src_addr, dest_layout.size)?;
                    } else {
                        self.emit_memcpy(dest, dest_ty, src_addr, src_ty)?;
                    }
                    Ok(true)
                }
                Rvalue::Use(Operand::Const(c)) if matches!(c.kind, ConstKind::ZeroInit) => {
                    if self.place_is_volatile(place, body)? {
                        self.emit_volatile_memset_zero(dest, dest_ty)?;
                    } else {
                        self.emit_memset_zero(dest, dest_ty)?;
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }

        /// Byte-wise volatile load/store copy (LLVM `memcpy` intrinsic has no volatile flag in inkwell 0.6).
        fn emit_volatile_byte_range_copy(
            &self,
            dest: PointerValue<'ctx>,
            src: PointerValue<'ctx>,
            size: u64,
        ) -> Result<(), CodegenError> {
            let i8_ty = self.context.i8_type();
            let i64_ty = self.context.i64_type();
            for off in 0..size {
                let off_val = i64_ty.const_int(off, false);
                let sp = self.build_gep(i8_ty, src, &[off_val], "vbc.src")?;
                let dp = self.build_gep(i8_ty, dest, &[off_val], "vbc.dst")?;
                let b = self.emit_memory_load(i8_ty.as_basic_type_enum(), sp, "vbc.l", true)?;
                self.emit_memory_store(dp, b, true)?;
            }
            Ok(())
        }

        fn emit_volatile_memset_zero(
            &self,
            dest: PointerValue<'ctx>,
            dest_ty: TyId,
        ) -> Result<(), CodegenError> {
            let layout = self.memory_layout(dest_ty)?;
            let i8_ty = self.context.i8_type();
            let i64_ty = self.context.i64_type();
            let zero = i8_ty.const_zero().as_basic_value_enum();
            for off in 0..layout.size {
                let off_val = i64_ty.const_int(off, false);
                let dp = self.build_gep(i8_ty, dest, &[off_val], "vmz")?;
                self.emit_memory_store(dp, zero, true)?;
            }
            Ok(())
        }

        fn emit_memcpy(
            &self,
            dest: PointerValue<'ctx>,
            dest_ty: TyId,
            src: PointerValue<'ctx>,
            src_ty: TyId,
        ) -> Result<(), CodegenError> {
            // Non-volatile aggregate copies only; volatile paths use `emit_volatile_byte_range_copy`.
            let dest_layout = self.memory_layout(dest_ty)?;
            let src_layout = self.memory_layout(src_ty)?;
            if dest_layout.size != src_layout.size {
                return Err(CodegenError::Internal(format!(
                    "aggregate copy size mismatch: dest {} bytes, source {} bytes",
                    dest_layout.size, src_layout.size
                )));
            }
            let size = self.context.i64_type().const_int(dest_layout.size, false);
            self.builder
                .build_memcpy(
                    dest,
                    layout_align(dest_ty, dest_layout)?,
                    src,
                    layout_align(src_ty, src_layout)?,
                    size,
                )
                .map(|_| ())
                .map_err(builder_error)
        }

        fn emit_memset_zero(
            &self,
            dest: PointerValue<'ctx>,
            dest_ty: TyId,
        ) -> Result<(), CodegenError> {
            let layout = self.memory_layout(dest_ty)?;
            let value = self.context.i8_type().const_zero();
            let size = self.context.i64_type().const_int(layout.size, false);
            self.builder
                .build_memset(dest, layout_align(dest_ty, layout)?, value, size)
                .map(|_| ())
                .map_err(builder_error)
        }

        /// Store an LLVM value into a CFG [`Place`].
        pub fn emit_store_place(
            &self,
            place: &Place,
            value: BasicValueEnum<'ctx>,
            locals: &IndexVec<Local, PointerValue<'ctx>>,
            body: &Body,
        ) -> Result<(), CodegenError> {
            if self.try_emit_bitfield_store(place, value, locals, body)? {
                return Ok(());
            }
            let addr = self.emit_place_addr(place, locals, body)?;
            let volatile = self.place_is_volatile(place, body)?;
            self.emit_memory_store(addr, value, volatile)
        }

        /// Resolve the HIR type of a place from the body's local declarations.
        fn place_ty(&self, place: &Place, body: &Body) -> Result<TyId, CodegenError> {
            let mut ty = body
                .locals
                .get(place.base)
                .ok_or_else(|| {
                    CodegenError::Internal(format!("place base local {:?} is missing", place.base))
                })?
                .ty;
            for proj in &place.projection {
                match proj {
                    Projection::Deref => {
                        let Ty::Ptr(pointee) = self.tcx.get(ty) else {
                            return Err(invalid_place_projection("dereference", ty));
                        };
                        ty = pointee.ty;
                    }
                    Projection::Field(idx) => {
                        let (_, field_ty) = self.record_field_ty(ty, *idx)?;
                        ty = field_ty;
                    }
                    Projection::Index(_) => {
                        ty = match self.tcx.get(ty) {
                            Ty::Array { elem, .. } => elem.ty,
                            Ty::Ptr(pointee) => pointee.ty,
                            _ => return Err(invalid_place_projection("index", ty)),
                        };
                    }
                }
            }
            Ok(ty)
        }

        fn operand_ty(&self, operand: &Operand, body: &Body) -> Result<TyId, CodegenError> {
            match operand {
                Operand::Copy(place) | Operand::Move(place) => self.place_ty(place, body),
                Operand::Const(c) => Ok(c.ty),
            }
        }

        fn is_bool_ty(&self, ty: TyId) -> bool {
            matches!(self.tcx.get(ty), Ty::Int { rank: IntRank::Bool, .. })
        }

        fn is_signed_integer_ty(&self, ty: TyId) -> Result<bool, CodegenError> {
            match self.tcx.get(ty) {
                Ty::Int { signed, .. } => Ok(*signed),
                Ty::Enum(def) => {
                    let Some(def_data) = self.hir.defs.get(*def) else {
                        return Ok(true);
                    };
                    let DefKind::Enum { repr, .. } = &def_data.kind else {
                        return Err(CodegenError::Internal(format!(
                            "definition {:?} is not an enum",
                            def
                        )));
                    };
                    self.is_signed_integer_ty(*repr)
                }
                other => Err(CodegenError::Internal(format!(
                    "expected integer type for signedness, got {:?}",
                    other
                ))),
            }
        }

        fn float_ty_width(&self, ty: TyId) -> Result<u32, CodegenError> {
            match self.tcx.get(ty) {
                Ty::Float(FloatKind::F32) => Ok(32),
                Ty::Float(FloatKind::F64) => Ok(64),
                Ty::Float(FloatKind::F80) => Ok(80),
                other => Err(CodegenError::Internal(format!(
                    "expected floating type for cast width, got {:?}",
                    other
                ))),
            }
        }

        fn is_memory_aggregate_ty(&self, ty: TyId) -> bool {
            matches!(self.tcx.get(ty), Ty::Array { .. } | Ty::Record(_))
        }

        fn memory_layout(&self, ty: TyId) -> Result<Layout, CodegenError> {
            LayoutCx::with_defs(self.tcx, &self.hir.defs)
                .layout_of(ty)
                .map_err(|err| type_error(ty, err.to_string()))
        }

        /// Materialise a CFG [`Const`] as an LLVM value.
        fn emit_const(&self, c: &rcc_cfg::Const) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match &c.kind {
                ConstKind::Int(n) => {
                    let BasicTypeEnum::IntType(ty) = self.type_cx().basic_type_of(c.ty)? else {
                        return Err(CodegenError::Internal(format!(
                            "integer constant has non-integer type {:?}",
                            c.ty
                        )));
                    };
                    Ok(ty.const_int(*n as u64, *n < 0).as_basic_value_enum())
                }
                ConstKind::Float(f) => {
                    let BasicTypeEnum::FloatType(ty) = self.type_cx().basic_type_of(c.ty)? else {
                        return Err(CodegenError::Internal(format!(
                            "floating constant has non-floating type {:?}",
                            c.ty
                        )));
                    };
                    Ok(ty.const_float(*f).as_basic_value_enum())
                }
                ConstKind::Global(def) => {
                    if let Some(gv) = self.globals.get(def) {
                        Ok(gv.as_pointer_value().as_basic_value_enum())
                    } else if let Some(fv) = self.functions.get(def) {
                        Ok(fv.as_global_value().as_pointer_value().as_basic_value_enum())
                    } else {
                        Err(CodegenError::Internal(format!(
                            "global constant references undeclared definition {:?}",
                            def
                        )))
                    }
                }
                ConstKind::ZeroInit => {
                    let ty = self.type_cx().basic_type_of(c.ty)?;
                    Ok(ty.const_zero())
                }
            }
        }

        fn emit_global_object_load(
            &self,
            def: DefId,
            ty: TyId,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let gv = self.globals.get(&def).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "global-object load references undeclared object {:?}",
                    def
                ))
            })?;
            let llvm_ty = self.type_cx().basic_type_of(ty)?;
            self.emit_memory_load(
                llvm_ty,
                gv.as_pointer_value(),
                "global.load",
                self.global_is_volatile(def),
            )
        }

        fn global_is_volatile(&self, def: DefId) -> bool {
            let Some(def_data) = self.hir.defs.get(def) else {
                return false;
            };
            match &def_data.kind {
                DefKind::Global { quals, .. } => quals.is_volatile,
                _ => false,
            }
        }

        fn record_field_info(
            &self,
            ty: TyId,
            index: u32,
        ) -> Result<(RecordKind, TyId, ObjectQuals), CodegenError> {
            let field = self.record_field_access(ty, index)?;
            Ok((field.kind, field.ty, field.quals))
        }

        fn record_field_access(
            &self,
            ty: TyId,
            index: u32,
        ) -> Result<RecordFieldAccess, CodegenError> {
            let Ty::Record(def) = self.tcx.get(ty) else {
                return Err(invalid_place_projection("field", ty));
            };
            let def_data = self.hir.defs.get(*def).ok_or_else(|| {
                CodegenError::Internal(format!("record definition {:?} is missing", def))
            })?;
            let DefKind::Record { kind, fields, .. } = &def_data.kind else {
                return Err(CodegenError::Internal(format!(
                    "definition {:?} is not a record",
                    def
                )));
            };
            let field = fields.get(index as usize).ok_or_else(|| {
                CodegenError::Internal(format!("record {:?} has no field at index {}", def, index))
            })?;
            let record_layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                .record_layout_of(ty)
                .map_err(|err| type_error(ty, err.to_string()))?;
            let layout = record_layout.fields.get(index as usize).copied().ok_or_else(|| {
                CodegenError::Internal(format!(
                    "record {:?} layout has no field at index {}",
                    def, index
                ))
            })?;
            Ok(RecordFieldAccess { kind: *kind, ty: field.ty, quals: field.quals, layout })
        }

        fn record_field_ty(
            &self,
            ty: TyId,
            index: u32,
        ) -> Result<(RecordKind, TyId), CodegenError> {
            let (kind, field_ty, _) = self.record_field_info(ty, index)?;
            Ok((kind, field_ty))
        }

        #[allow(unsafe_code)]
        fn build_gep<T: BasicType<'ctx>>(
            &self,
            pointee_ty: T,
            ptr: PointerValue<'ctx>,
            indices: &[inkwell::values::IntValue<'ctx>],
            name: &str,
        ) -> Result<PointerValue<'ctx>, CodegenError> {
            // SAFETY: `emit_place_addr` selects indices from validated CFG projection
            // types, so the pointee type and index arity match the address calculation.
            unsafe { self.builder.build_gep(pointee_ty, ptr, indices, name) }.map_err(builder_error)
        }
    }

    struct FnCodegen<'cg, 'a, 'ctx> {
        cx: &'cg CodegenCx<'a, 'ctx>,
        function: FunctionValue<'ctx>,
        body: &'cg Body,
        locals: LocalMap<'ctx>,
        vla_stacks: VlaStackMap<'ctx>,
        blocks: IndexVec<BasicBlockId, LlvmBasicBlock<'ctx>>,
    }

    impl<'cg, 'a, 'ctx> FnCodegen<'cg, 'a, 'ctx> {
        fn new(
            cx: &'cg CodegenCx<'a, 'ctx>,
            function: FunctionValue<'ctx>,
            body: &'cg Body,
        ) -> Result<Self, CodegenError> {
            if function.get_first_basic_block().is_some() {
                return Err(CodegenError::Internal(format!(
                    "function {} already has LLVM basic blocks",
                    function.get_name().to_string_lossy()
                )));
            }

            let mut blocks = IndexVec::with_capacity(body.blocks.len());
            for (bb, _) in body.blocks.iter_enumerated() {
                let llvm_block = cx.context.append_basic_block(function, &block_name(bb));
                let inserted = blocks.push(llvm_block);
                debug_assert_eq!(inserted, bb);
            }

            let entry = *blocks
                .get(BasicBlockId(0))
                .ok_or_else(|| CodegenError::Internal("CFG body has no entry block".to_owned()))?;
            cx.builder.position_at_end(entry);
            if let Some(def) = body.def {
                cx.debug_subprogram(def, function)?;
            }
            let locals = cx.materialize_locals(function, body)?;
            cx.emit_debug_declarations(function, body, &locals)?;
            let mut vla_stacks = VlaStackMap::with_capacity(body.locals.len());
            for _ in body.locals.iter() {
                vla_stacks.push(None);
            }

            Ok(Self { cx, function, body, locals, vla_stacks, blocks })
        }

        fn codegen_body(&mut self) -> Result<(), CodegenError> {
            for (bb, block) in self.body.blocks.iter_enumerated() {
                let llvm_block = self.llvm_block(bb)?;
                self.cx.builder.position_at_end(llvm_block);
                self.ensure_no_terminator(bb, llvm_block)?;

                for statement in &block.statements {
                    self.emit_statement(statement)?;
                    self.ensure_no_terminator(bb, llvm_block)?;
                }

                self.emit_terminator(bb, &block.terminator)?;
                if llvm_block.get_terminator().is_none() {
                    return Err(CodegenError::Internal(format!(
                        "CFG block {bb:?} did not emit an LLVM terminator"
                    )));
                }
            }

            Ok(())
        }

        fn emit_statement(&mut self, statement: &Statement) -> Result<(), CodegenError> {
            self.cx.set_debug_location(self.body.def, statement.span);
            match &statement.kind {
                StatementKind::Assign { place, rvalue } => {
                    if self.cx.try_emit_aggregate_assign(place, rvalue, &self.locals, self.body)? {
                        return Ok(());
                    }
                    let value = self.cx.emit_rvalue_value(rvalue, &self.locals, self.body)?;
                    self.cx.emit_store_place(place, value, &self.locals, self.body)
                }
                StatementKind::StorageLive(local) => self.cx.emit_storage_live(
                    *local,
                    &mut self.locals,
                    &mut self.vla_stacks,
                    self.body,
                ),
                StatementKind::StorageDead(local) => {
                    self.cx.emit_storage_dead(*local, &self.locals, &mut self.vla_stacks, self.body)
                }
                StatementKind::Nop => Ok(()),
            }
        }

        fn emit_terminator(
            &self,
            bb: BasicBlockId,
            terminator: &rcc_cfg::Terminator,
        ) -> Result<(), CodegenError> {
            self.cx.set_debug_location(self.body.def, terminator.span);
            let llvm_block = self.llvm_block(bb)?;
            self.ensure_no_terminator(bb, llvm_block)?;

            match &terminator.kind {
                TerminatorKind::Goto(target) => self.emit_goto(*target),
                TerminatorKind::SwitchInt { discr, targets } => {
                    self.emit_switch_int(discr, targets.as_slice())
                }
                TerminatorKind::Return => self.emit_return(),
                TerminatorKind::Call { callee, args, destination, target } => {
                    self.emit_call_terminator(callee, args, destination.as_ref(), *target)
                }
                TerminatorKind::Unreachable => {
                    self.cx.builder.build_unreachable().map(|_| ()).map_err(builder_error)
                }
                TerminatorKind::BuiltinVaStart { ap, last_param, target } => {
                    self.emit_va_start(ap, last_param)?;
                    self.emit_goto(*target)
                }
                TerminatorKind::BuiltinVaEnd { ap, target } => {
                    self.emit_va_end(ap)?;
                    self.emit_goto(*target)
                }
                TerminatorKind::BuiltinVaCopy { dst, src, target } => {
                    self.emit_va_copy(dst, src)?;
                    self.emit_goto(*target)
                }
            }
        }

        fn emit_goto(&self, target: BasicBlockId) -> Result<(), CodegenError> {
            let target = self.llvm_block(target)?;
            self.cx.builder.build_unconditional_branch(target).map(|_| ()).map_err(builder_error)
        }

        fn emit_switch_int(
            &self,
            discr: &Operand,
            targets: &[(Option<i128>, BasicBlockId)],
        ) -> Result<(), CodegenError> {
            let discr = match self.cx.emit_operand_value(discr, &self.locals, self.body)? {
                BasicValueEnum::IntValue(value) => value,
                other => {
                    return Err(CodegenError::Internal(format!(
                        "SwitchInt discriminator must be an integer, got {:?}",
                        other.get_type()
                    )));
                }
            };
            let Some(((default_value, default_target), cases)) = targets.split_last() else {
                return Err(CodegenError::Internal("SwitchInt has no targets".to_owned()));
            };
            if default_value.is_some() {
                return Err(CodegenError::Internal(
                    "SwitchInt default target must be last".to_owned(),
                ));
            }

            let default_block = self.llvm_block(*default_target)?;
            let mut llvm_cases = Vec::with_capacity(cases.len());
            for (case_value, target) in cases {
                let Some(case_value) = case_value else {
                    return Err(CodegenError::Internal(
                        "SwitchInt non-default target is missing a case value".to_owned(),
                    ));
                };
                let case_value = discr.get_type().const_int(*case_value as u64, *case_value < 0);
                llvm_cases.push((case_value, self.llvm_block(*target)?));
            }

            self.cx
                .builder
                .build_switch(discr, default_block, &llvm_cases)
                .map(|_| ())
                .map_err(builder_error)
        }

        fn emit_call_terminator(
            &self,
            callee: &Operand,
            args: &[Operand],
            destination: Option<&Place>,
            target: Option<BasicBlockId>,
        ) -> Result<(), CodegenError> {
            let fn_ty = self.callee_fn_ty(callee)?;
            let abi = sysv_fn_abi(self.cx.tcx, &self.cx.hir.defs, fn_ty)?;
            if abi.variadic {
                if args.len() < abi.params.len() {
                    return Err(CodegenError::Internal(format!(
                        "variadic call has {} args but ABI requires at least {} fixed params",
                        args.len(),
                        abi.params.len()
                    )));
                }
            } else if args.len() != abi.params.len() {
                return Err(CodegenError::Internal(format!(
                    "call argument count {} does not match ABI parameter count {}",
                    args.len(),
                    abi.params.len()
                )));
            }

            let mut llvm_args = Vec::with_capacity(abi.fixed_param_count);
            if matches!(abi.ret.kind, AbiReturnKind::Indirect { .. }) {
                let dest = destination.ok_or_else(|| {
                    CodegenError::Internal(
                        "indirect ABI return requires a call destination".to_owned(),
                    )
                })?;
                llvm_args.push(self.cx.emit_place_addr(dest, &self.locals, self.body)?.into());
            }

            for (arg, param_abi) in args.iter().zip(abi.params.iter()) {
                self.push_call_arg(arg, param_abi, &mut llvm_args)?;
            }

            for arg in args.iter().skip(abi.params.len()) {
                let value = self.cx.emit_operand_value(arg, &self.locals, self.body)?;
                llvm_args.push(value.into());
            }

            let fn_type = self.cx.type_cx().fn_type_of(fn_ty)?;
            let returns_value =
                !matches!(abi.ret.kind, AbiReturnKind::Void | AbiReturnKind::Indirect { .. });
            let call_name = if returns_value { "call" } else { "" };
            let call = if let Some(function) = self.direct_callee(callee)? {
                self.cx
                    .builder
                    .build_call(function, &llvm_args, call_name)
                    .map_err(builder_error)?
            } else {
                let callee = self.cx.emit_operand_value(callee, &self.locals, self.body)?;
                let BasicValueEnum::PointerValue(callee_ptr) = callee else {
                    return Err(CodegenError::Internal(format!(
                        "callee operand must lower to a pointer, got {:?}",
                        callee.get_type()
                    )));
                };
                self.cx
                    .builder
                    .build_indirect_call(fn_type, callee_ptr, &llvm_args, call_name)
                    .map_err(builder_error)?
            };

            self.cx.apply_call_abi_attrs(call, &abi)?;
            self.store_call_result(call, &abi.ret, destination)?;

            match target {
                Some(target) => self.emit_goto(target),
                None => self.cx.builder.build_unreachable().map(|_| ()).map_err(builder_error),
            }
        }

        fn callee_fn_ty(&self, callee: &Operand) -> Result<TyId, CodegenError> {
            let ty = self.operand_ty(callee)?;
            match self.cx.tcx.get(ty) {
                Ty::Func { .. } => Ok(ty),
                Ty::Ptr(q) => match self.cx.tcx.get(q.ty) {
                    Ty::Func { .. } => Ok(q.ty),
                    _ => Err(CodegenError::Internal(format!(
                        "callee pointer does not point to a function type: {:?}",
                        q.ty
                    ))),
                },
                _ => Err(CodegenError::Internal(format!(
                    "callee operand is not a function or function pointer: {:?}",
                    ty
                ))),
            }
        }

        fn direct_callee(
            &self,
            callee: &Operand,
        ) -> Result<Option<FunctionValue<'ctx>>, CodegenError> {
            let Operand::Const(c) = callee else {
                return Ok(None);
            };
            let ConstKind::Global(def) = &c.kind else {
                return Ok(None);
            };
            if !matches!(self.cx.tcx.get(c.ty), Ty::Func { .. }) {
                return Ok(None);
            }
            let function = self.cx.function_decl(*def).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "function constant references undeclared definition {:?}",
                    *def
                ))
            })?;
            Ok(Some(function))
        }

        fn push_call_arg(
            &self,
            arg: &Operand,
            param_abi: &AbiParam,
            llvm_args: &mut Vec<BasicMetadataValueEnum<'ctx>>,
        ) -> Result<(), CodegenError> {
            match &param_abi.kind {
                AbiParamKind::Direct(units) => {
                    if matches!(
                        units.as_slice(),
                        [AbiParamUnit { kind: AbiParamUnitKind::Source(source), .. }]
                            if *source == param_abi.source
                    ) {
                        let value = self.cx.emit_operand_value(arg, &self.locals, self.body)?;
                        llvm_args.push(value.into());
                        return Ok(());
                    }

                    let addr = self.operand_addr(arg)?;
                    for (unit_idx, unit) in units.iter().enumerate() {
                        let value = self.load_abi_unit(
                            addr,
                            abi_unit_offset(unit_idx, "call argument")?,
                            *unit,
                        )?;
                        llvm_args.push(value.into());
                    }
                    Ok(())
                }
                AbiParamKind::Indirect { .. } => {
                    let addr = self.operand_addr(arg)?;
                    llvm_args.push(addr.into());
                    Ok(())
                }
            }
        }

        fn load_abi_unit(
            &self,
            addr: PointerValue<'ctx>,
            offset: u64,
            unit: AbiParamUnit,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let offset = self.cx.context.i64_type().const_int(offset, false);
            let byte_ptr =
                self.cx.build_gep(self.cx.context.i8_type(), addr, &[offset], "abi.unit")?;
            let unit_ty = self.cx.type_cx().abi_unit_type(unit)?;
            self.cx.builder.build_load(unit_ty, byte_ptr, "abi.unit").map_err(builder_error)
        }

        fn store_call_result(
            &self,
            call: CallSiteValue<'ctx>,
            ret: &AbiReturn,
            destination: Option<&Place>,
        ) -> Result<(), CodegenError> {
            match &ret.kind {
                AbiReturnKind::Void => {
                    if destination.is_some() {
                        return Err(CodegenError::Internal(
                            "void call cannot write a destination".to_owned(),
                        ));
                    }
                    Ok(())
                }
                AbiReturnKind::Indirect { .. } => {
                    if destination.is_none() {
                        return Err(CodegenError::Internal(
                            "indirect ABI return requires a call destination".to_owned(),
                        ));
                    }
                    Ok(())
                }
                AbiReturnKind::Direct { units } => {
                    let dest = destination.ok_or_else(|| {
                        CodegenError::Internal(
                            "direct ABI return requires a call destination".to_owned(),
                        )
                    })?;
                    let value = call.try_as_basic_value().left().ok_or_else(|| {
                        CodegenError::Internal("direct ABI call did not produce a value".to_owned())
                    })?;
                    let dest_addr = self.cx.emit_place_addr(dest, &self.locals, self.body)?;

                    if units.len() == 1 {
                        return self.cx.store_abi_unit(dest_addr, 0, value);
                    }

                    let BasicValueEnum::StructValue(return_struct) = value else {
                        return Err(CodegenError::Internal(format!(
                            "multi-unit ABI return produced non-struct value {:?}",
                            value.get_type()
                        )));
                    };
                    for (unit_idx, _) in units.iter().enumerate() {
                        let index = u32::try_from(unit_idx).map_err(|_| {
                            CodegenError::Internal("ABI return unit index overflowed".to_owned())
                        })?;
                        let unit_value = self
                            .cx
                            .builder
                            .build_extract_value(return_struct, index, "ret.unit")
                            .map_err(builder_error)?;
                        self.cx.store_abi_unit(
                            dest_addr,
                            abi_unit_offset(unit_idx, "call return")?,
                            unit_value,
                        )?;
                    }
                    Ok(())
                }
            }
        }

        fn operand_ty(&self, operand: &Operand) -> Result<TyId, CodegenError> {
            match operand {
                Operand::Copy(place) | Operand::Move(place) => self.cx.place_ty(place, self.body),
                Operand::Const(c) => Ok(c.ty),
            }
        }

        fn operand_addr(&self, operand: &Operand) -> Result<PointerValue<'ctx>, CodegenError> {
            match operand {
                Operand::Copy(place) | Operand::Move(place) => {
                    self.cx.emit_place_addr(place, &self.locals, self.body)
                }
                Operand::Const(_) => Err(CodegenError::Internal(
                    "ABI aggregate call arguments must be addressable operands".to_owned(),
                )),
            }
        }

        // ---------------------------------------------------------------
        // Variadic builtin helpers (09-19)
        // ---------------------------------------------------------------

        fn va_list_ptr(&self, operand: &Operand) -> Result<PointerValue<'ctx>, CodegenError> {
            self.cx.emit_place_addr(
                match operand {
                    Operand::Copy(place) | Operand::Move(place) => place,
                    Operand::Const(_) => {
                        return Err(CodegenError::Internal(
                            "va_list operand must be an addressable place".to_owned(),
                        ));
                    }
                },
                &self.locals,
                self.body,
            )
        }

        fn va_start_intrinsic(&self) -> FunctionValue<'ctx> {
            let name = "llvm.va_start.p0";
            self.cx.module.get_function(name).unwrap_or_else(|| {
                let ptr_ty = self.cx.context.ptr_type(AddressSpace::default());
                let fn_ty = self.cx.context.void_type().fn_type(&[ptr_ty.into()], false);
                self.cx.module.add_function(name, fn_ty, None)
            })
        }

        fn va_end_intrinsic(&self) -> FunctionValue<'ctx> {
            let name = "llvm.va_end.p0";
            self.cx.module.get_function(name).unwrap_or_else(|| {
                let ptr_ty = self.cx.context.ptr_type(AddressSpace::default());
                let fn_ty = self.cx.context.void_type().fn_type(&[ptr_ty.into()], false);
                self.cx.module.add_function(name, fn_ty, None)
            })
        }

        fn va_copy_intrinsic(&self) -> FunctionValue<'ctx> {
            let name = "llvm.va_copy.p0";
            self.cx.module.get_function(name).unwrap_or_else(|| {
                let ptr_ty = self.cx.context.ptr_type(AddressSpace::default());
                let fn_ty =
                    self.cx.context.void_type().fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
                self.cx.module.add_function(name, fn_ty, None)
            })
        }

        fn emit_va_start(&self, ap: &Operand, _last_param: &Operand) -> Result<(), CodegenError> {
            let ap_ptr = self.va_list_ptr(ap)?;
            let intrinsic = self.va_start_intrinsic();
            self.cx.builder.build_call(intrinsic, &[ap_ptr.into()], "").map_err(builder_error)?;
            Ok(())
        }

        fn emit_va_end(&self, ap: &Operand) -> Result<(), CodegenError> {
            let ap_ptr = self.va_list_ptr(ap)?;
            let intrinsic = self.va_end_intrinsic();
            self.cx.builder.build_call(intrinsic, &[ap_ptr.into()], "").map_err(builder_error)?;
            Ok(())
        }

        fn emit_va_copy(&self, dst: &Operand, src: &Operand) -> Result<(), CodegenError> {
            let dst_ptr = self.va_list_ptr(dst)?;
            let src_ptr = self.va_list_ptr(src)?;
            let intrinsic = self.va_copy_intrinsic();
            self.cx
                .builder
                .build_call(intrinsic, &[dst_ptr.into(), src_ptr.into()], "")
                .map_err(builder_error)?;
            Ok(())
        }

        fn emit_return(&self) -> Result<(), CodegenError> {
            let ret_ty = self.body.ret_ty.ok_or_else(|| {
                CodegenError::Internal("CFG body is missing its return type".to_owned())
            })?;
            if matches!(self.cx.tcx.get(ret_ty), Ty::Void) {
                return self.cx.builder.build_return(None).map(|_| ()).map_err(builder_error);
            }

            match self.cx.body_abi(self.body)? {
                Some(abi) => match &abi.ret.kind {
                    AbiReturnKind::Void => {
                        self.cx.builder.build_return(None).map(|_| ()).map_err(builder_error)
                    }
                    AbiReturnKind::Direct { units }
                        if matches!(
                            units.as_slice(),
                            [AbiParamUnit { kind: AbiParamUnitKind::Source(source), .. }]
                                if *source == ret_ty
                        ) =>
                    {
                        self.emit_direct_return(ret_ty)
                    }
                    AbiReturnKind::Direct { units } => self.emit_coerced_direct_return(units),
                    AbiReturnKind::Indirect { .. } => self.emit_indirect_return(ret_ty),
                },
                None => self.emit_direct_return(ret_ty),
            }
        }

        fn emit_indirect_return(&self, ret_ty: TyId) -> Result<(), CodegenError> {
            let dest = self.function.get_nth_param(0).ok_or_else(|| {
                CodegenError::Internal("sret function is missing hidden return pointer".to_owned())
            })?;
            let BasicValueEnum::PointerValue(dest) = dest else {
                return Err(CodegenError::Internal(format!(
                    "sret hidden parameter must be a pointer, got {:?}",
                    dest.get_type()
                )));
            };
            let src = self.locals[Local(0)];
            self.cx.emit_memcpy(dest, ret_ty, src, ret_ty)?;
            self.cx.builder.build_return(None).map(|_| ()).map_err(builder_error)
        }

        fn emit_coerced_direct_return(&self, units: &[AbiParamUnit]) -> Result<(), CodegenError> {
            let ret_addr = self.locals[Local(0)];
            if units.len() == 1 {
                let value = self.load_abi_unit(ret_addr, 0, units[0])?;
                return self
                    .cx
                    .builder
                    .build_return(Some(&value))
                    .map(|_| ())
                    .map_err(builder_error);
            }

            let fields = units
                .iter()
                .map(|unit| self.cx.type_cx().abi_unit_type(*unit))
                .collect::<Result<Vec<_>, _>>()?;
            let ret_ty = self.cx.context.struct_type(&fields, false);
            let mut aggregate = ret_ty.get_undef();
            for (unit_idx, unit) in units.iter().enumerate() {
                let index = u32::try_from(unit_idx).map_err(|_| {
                    CodegenError::Internal("ABI return unit index overflowed".to_owned())
                })?;
                let value = self.load_abi_unit(
                    ret_addr,
                    abi_unit_offset(unit_idx, "function return")?,
                    *unit,
                )?;
                aggregate = self
                    .cx
                    .builder
                    .build_insert_value(aggregate, value, index, "ret.unit")
                    .map_err(builder_error)?
                    .into_struct_value();
            }
            self.cx.builder.build_return(Some(&aggregate)).map(|_| ()).map_err(builder_error)
        }

        fn emit_direct_return(&self, ret_ty: TyId) -> Result<(), CodegenError> {
            let value = self.cx.emit_operand_value(
                &Operand::Copy(Place { base: Local(0), projection: Vec::new() }),
                &self.locals,
                self.body,
            )?;
            let expected = self.cx.type_cx().basic_type_of(ret_ty)?;
            if value.get_type() != expected {
                return Err(CodegenError::Internal(format!(
                    "return slot lowered to {:?}, expected {:?}",
                    value.get_type(),
                    expected
                )));
            }
            self.cx.builder.build_return(Some(&value)).map(|_| ()).map_err(builder_error)
        }

        fn llvm_block(&self, bb: BasicBlockId) -> Result<LlvmBasicBlock<'ctx>, CodegenError> {
            self.blocks.get(bb).copied().ok_or_else(|| {
                CodegenError::Internal(format!("CFG branch target {bb:?} has no LLVM block"))
            })
        }

        fn ensure_no_terminator(
            &self,
            bb: BasicBlockId,
            llvm_block: LlvmBasicBlock<'ctx>,
        ) -> Result<(), CodegenError> {
            if llvm_block.get_terminator().is_some() {
                return Err(CodegenError::Internal(format!(
                    "CFG block {bb:?} already has an LLVM terminator"
                )));
            }
            Ok(())
        }
    }

    fn block_name(bb: BasicBlockId) -> String {
        if bb == BasicBlockId(0) {
            "entry".to_owned()
        } else {
            format!("bb{}", bb.0)
        }
    }

    fn abi_unit_offset(index: usize, context: &str) -> Result<u64, CodegenError> {
        u64::try_from(index)
            .map_err(|_| CodegenError::Internal(format!("{context} unit index overflowed")))?
            .checked_mul(8)
            .ok_or_else(|| CodegenError::Internal(format!("{context} unit offset overflowed")))
    }

    fn format_cfg_errors(errors: &[rcc_cfg::verify::CfgError]) -> String {
        errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    }

    fn invalid_place_projection(projection: &str, ty: TyId) -> CodegenError {
        CodegenError::Internal(format!("invalid {projection} projection for place type {ty:?}"))
    }

    fn storage_bits_for_bitfield(storage_size: u64) -> Result<u32, CodegenError> {
        let bits = storage_size.checked_mul(8).ok_or_else(|| {
            CodegenError::Internal("bit-field storage size overflowed".to_owned())
        })?;
        let bits = u32::try_from(bits).map_err(|_| {
            CodegenError::Internal("bit-field storage unit exceeds u32 bits".to_owned())
        })?;
        if bits == 0 || bits > 64 {
            return Err(CodegenError::Internal(format!(
                "unsupported {bits}-bit bit-field storage unit"
            )));
        }
        Ok(bits)
    }

    fn bit_mask(bits: u32) -> Result<u64, CodegenError> {
        if bits == 0 || bits > 64 {
            return Err(CodegenError::Internal(format!(
                "unsupported {bits}-bit bit-field mask width"
            )));
        }
        if bits == 64 {
            Ok(u64::MAX)
        } else {
            Ok((1_u64 << bits) - 1)
        }
    }

    fn builder_error(error: impl std::fmt::Display) -> CodegenError {
        CodegenError::Internal(format!("LLVM builder failed: {error}"))
    }

    fn layout_align(ty: TyId, layout: Layout) -> Result<u32, CodegenError> {
        if layout.align == 0 || !layout.align.is_power_of_two() {
            return Err(type_error(ty, format!("invalid memory alignment {}", layout.align)));
        }
        Ok(layout.align)
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
                Ty::BuiltinVaList => {
                    let i32_ty = self.context.i32_type();
                    let ptr_ty = self.context.ptr_type(AddressSpace::default());
                    self.context
                        .struct_type(
                            &[i32_ty.into(), i32_ty.into(), ptr_ty.into(), ptr_ty.into()],
                            false,
                        )
                        .into()
                }
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
            let abi = sysv_fn_abi(self.tcx, &self.hir.defs, ty)?;
            let params = self.abi_param_types(&abi)?;

            match self.abi_return_type(&abi.ret)? {
                None => Ok(self.context.void_type().fn_type(&params, abi.variadic)),
                Some(ret_ty) => Ok(ret_ty.fn_type(&params, abi.variadic)),
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

        fn abi_param_types(
            &mut self,
            abi: &FnAbi,
        ) -> Result<Vec<BasicMetadataTypeEnum<'ctx>>, CodegenError> {
            let mut params = Vec::with_capacity(abi.fixed_param_count);
            if matches!(abi.ret.kind, AbiReturnKind::Indirect { .. }) {
                params.push(self.ptr_type().into());
            }
            for param in &abi.params {
                match &param.kind {
                    AbiParamKind::Direct(units) => {
                        for unit in units {
                            params.push(self.abi_unit_type(*unit)?.into());
                        }
                    }
                    AbiParamKind::Indirect { .. } => params.push(self.ptr_type().into()),
                }
            }
            Ok(params)
        }

        fn abi_return_type(
            &mut self,
            ret: &AbiReturn,
        ) -> Result<Option<BasicTypeEnum<'ctx>>, CodegenError> {
            match &ret.kind {
                AbiReturnKind::Void | AbiReturnKind::Indirect { .. } => Ok(None),
                AbiReturnKind::Direct { units } if units.len() == 1 => {
                    self.abi_unit_type(units[0]).map(Some)
                }
                AbiReturnKind::Direct { units } => {
                    let fields = units
                        .iter()
                        .map(|unit| self.abi_unit_type(*unit))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Some(self.context.struct_type(&fields, false).into()))
                }
            }
        }

        fn abi_unit_type(
            &mut self,
            unit: AbiParamUnit,
        ) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
            match unit.kind {
                AbiParamUnitKind::Source(ty) => self.basic_type_of(ty),
                AbiParamUnitKind::Integer { bits } => {
                    Ok(self.context.custom_width_int_type(bits).into())
                }
                AbiParamUnitKind::Float(kind) => Ok(self.float_type(kind).into()),
                AbiParamUnitKind::Vector { elem, lanes } => {
                    Ok(self.float_type(elem).vec_type(lanes).into())
                }
            }
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

    // ---------------------------------------------------------------
    // 09-11: Global initializer materialization
    // ---------------------------------------------------------------

    /// Helper for lowering HIR `GlobalInit` values into LLVM constants and
    /// interning string literals into module-private globals.
    /// Build an LLVM array constant whose elements are arbitrary
    /// Build an LLVM array constant whose elements are arbitrary
    /// `BasicValueEnum`s.  `ArrayType::const_array` in inkwell 0.6 only
    /// accepts `&[ArrayValue]`; this helper uses the generic
    /// `ArrayValue::new_const_array` which accepts any `AsValueRef`.
    #[allow(unsafe_code)]
    fn const_array<'ctx>(
        ty: inkwell::types::ArrayType<'ctx>,
        values: &[BasicValueEnum<'ctx>],
    ) -> inkwell::values::ArrayValue<'ctx> {
        unsafe { inkwell::values::ArrayValue::new_const_array(&ty, values) }
    }

    /// Constant `inbounds GEP` on a pointer by an i8 offset.
    #[allow(unsafe_code)]
    fn const_ptr_offset<'ctx>(
        ptr: PointerValue<'ctx>,
        elem_ty: inkwell::types::IntType<'ctx>,
        offset: inkwell::values::IntValue<'ctx>,
    ) -> PointerValue<'ctx> {
        unsafe { ptr.const_in_bounds_gep(elem_ty, &[offset]) }
    }

    /// Shared state for lowering HIR global initializers into LLVM constants.
    pub struct GlobalCx<'a, 'ctx> {
        context: &'ctx Context,
        tcx: &'a TyCtxt,
        hir: &'a HirCrate,
        type_cx: TypeCx<'a, 'ctx>,
        globals: FxHashMap<DefId, GlobalValue<'ctx>>,
        functions: FxHashMap<DefId, FunctionValue<'ctx>>,
        string_literals: FxHashMap<DefId, GlobalValue<'ctx>>,
    }

    impl<'a, 'ctx> GlobalCx<'a, 'ctx> {
        /// Build a global-materialization helper sharing the codegen context.
        pub fn new(cx: &'a CodegenCx<'a, 'ctx>) -> Self {
            Self {
                context: cx.context(),
                tcx: cx.tcx(),
                hir: cx.hir(),
                type_cx: cx.type_cx(),
                globals: cx.global_decls().clone(),
                functions: cx.function_decls().clone(),
                string_literals: FxHashMap::default(),
            }
        }

        /// Lower every `GlobalInit` attached to file-scope objects into LLVM
        /// constants and attach them as initializers to the declared globals.
        pub fn materialize_all_globals(&mut self) -> Result<(), CodegenError> {
            for (def, def_data) in self.hir.defs.iter_enumerated() {
                if let DefKind::Global { init: Some(global_init), .. } = &def_data.kind {
                    self.materialize_global_init(def, global_init)?;
                }
            }
            Ok(())
        }

        /// Lower one HIR `GlobalInit` into an LLVM constant and attach it to
        /// the previously-declared global for `def`.
        pub fn materialize_global_init(
            &mut self,
            def: DefId,
            init: &GlobalInit,
        ) -> Result<(), CodegenError> {
            for entry in &init.entries {
                if matches!(entry.value, GlobalInitValue::Error) {
                    return Err(CodegenError::Internal(format!(
                        "global initializer for {:?} contains an error leaf",
                        def
                    )));
                }
            }

            let global = self.globals.get(&def).copied().ok_or_else(|| {
                CodegenError::Internal(format!("global {:?} was not declared", def))
            })?;

            let init_value = self.build_const_value(init.ty, &init.entries)?;
            global.set_initializer(&init_value);

            Ok(())
        }

        /// Recursively build an LLVM constant from a (possibly empty) list of
        /// flattened `GlobalInitEntry` leaves.
        ///
        /// * If `entries` is empty the result is a zero initializer.
        /// * If `entries` has a single leaf with an empty path the result is a
        ///   scalar constant.
        /// * Otherwise the type must be an array or record and the entries are
        ///   dispatched by their first designator component.
        fn build_const_value(
            &mut self,
            ty: TyId,
            entries: &[GlobalInitEntry],
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            if entries.is_empty() {
                return Ok(self.type_cx.basic_type_of(ty)?.const_zero());
            }

            if entries.len() == 1 && entries[0].path.is_empty() {
                return self.global_init_value_to_llvm(&entries[0].value, ty);
            }

            match self.tcx.get(ty) {
                Ty::Array { elem, len: Some(len), is_vla: false } => {
                    self.build_array_const(elem.ty, *len, entries)
                }
                Ty::Record(def_id) => self.build_record_const(ty, *def_id, entries),
                other => Err(CodegenError::Internal(format!(
                    "expected aggregate type for nested global initializer, got {:?}",
                    other
                ))),
            }
        }

        fn build_array_const(
            &mut self,
            elem_ty: TyId,
            len: u64,
            entries: &[GlobalInitEntry],
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let llvm_elem_ty = self.type_cx.basic_type_of(elem_ty)?;
            let u32_len = u32::try_from(len)
                .map_err(|_| CodegenError::Internal("array length exceeds u32".to_owned()))?;
            let mut elements = Vec::with_capacity(len as usize);

            for i in 0..len {
                let sub_entries: Vec<GlobalInitEntry> = entries
                    .iter()
                    .filter(|e| {
                        matches!(e.path.first(), Some(GlobalInitDesignator::Index(idx)) if *idx == i)
                    })
                    .cloned()
                    .map(|mut e| {
                        e.path = e.path.into_iter().skip(1).collect();
                        e
                    })
                    .collect();
                elements.push(self.build_const_value(elem_ty, &sub_entries)?);
            }

            let array_ty = llvm_elem_ty.array_type(u32_len);
            Ok(const_array(array_ty, &elements).into())
        }

        fn build_record_const(
            &mut self,
            ty: TyId,
            def_id: DefId,
            entries: &[GlobalInitEntry],
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            let def_data = self.hir.defs.get(def_id).ok_or_else(|| {
                CodegenError::Internal(format!("record definition {:?} not found", def_id))
            })?;
            let DefKind::Record { kind, fields, .. } = &def_data.kind else {
                return Err(CodegenError::Internal(format!("{:?} is not a record", def_id)));
            };

            match kind {
                RecordKind::Struct => {
                    let mut field_values = Vec::with_capacity(fields.len());
                    for (i, field) in fields.iter().enumerate() {
                        let sub_entries: Vec<GlobalInitEntry> = entries
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e.path.first(),
                                    Some(GlobalInitDesignator::Field(idx)) if *idx as usize == i
                                )
                            })
                            .cloned()
                            .map(|mut e| {
                                e.path = e.path.into_iter().skip(1).collect();
                                e
                            })
                            .collect();
                        field_values.push(self.build_const_value(field.ty, &sub_entries)?);
                    }
                    let struct_ty = self.type_cx.basic_type_of(ty)?.into_struct_type();
                    Ok(struct_ty.const_named_struct(&field_values).into())
                }
                RecordKind::Union => {
                    // C99 §6.7.8: only the first named member is initialized
                    // unless a designator specifies another. We approximate by
                    // finding the first entry that names a field.
                    let first_field_entry = entries
                        .iter()
                        .find(|e| matches!(e.path.first(), Some(GlobalInitDesignator::Field(_))));

                    let layout = LayoutCx::with_defs(self.tcx, &self.hir.defs)
                        .layout_of(ty)
                        .map_err(|err| type_error(ty, err.to_string()))?;
                    let size = u32::try_from(layout.size)
                        .map_err(|_| CodegenError::Internal("union size exceeds u32".to_owned()))?;

                    if let Some(entry) = first_field_entry {
                        let field_idx = match entry.path.first().unwrap() {
                            GlobalInitDesignator::Field(idx) => *idx as usize,
                            _ => unreachable!(),
                        };
                        let field = fields.get(field_idx).ok_or_else(|| {
                            CodegenError::Internal(format!(
                                "union field index {} out of bounds",
                                field_idx
                            ))
                        })?;

                        let sub_entries: Vec<GlobalInitEntry> = entries
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e.path.first(),
                                    Some(GlobalInitDesignator::Field(idx)) if *idx as usize == field_idx
                                )
                            })
                            .cloned()
                            .map(|mut e| {
                                e.path = e.path.into_iter().skip(1).collect();
                                e
                            })
                            .collect();

                        let bytes = self.build_const_bytes(field.ty, &sub_entries, size)?;
                        let byte_array_ty = self.context.i8_type().array_type(size);
                        let byte_array = const_array(byte_array_ty, &bytes);
                        let struct_ty = self.type_cx.basic_type_of(ty)?.into_struct_type();
                        return Ok(struct_ty.const_named_struct(&[byte_array.into()]).into());
                    }

                    let byte_array_ty = self.context.i8_type().array_type(size);
                    let struct_ty = self.type_cx.basic_type_of(ty)?.into_struct_type();
                    Ok(struct_ty.const_named_struct(&[byte_array_ty.const_zero().into()]).into())
                }
            }
        }

        fn global_init_value_to_llvm(
            &mut self,
            value: &GlobalInitValue,
            ty: TyId,
        ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
            match value {
                GlobalInitValue::Int(n) => {
                    let BasicTypeEnum::IntType(int_ty) = self.type_cx.basic_type_of(ty)? else {
                        return Err(CodegenError::Internal(
                            "Int value for non-int type".to_owned(),
                        ));
                    };
                    Ok(int_ty.const_int(*n as u64, *n < 0).into())
                }
                GlobalInitValue::Float(f) => match self.type_cx.basic_type_of(ty)? {
                    BasicTypeEnum::FloatType(float_ty) => Ok(float_ty.const_float(*f).into()),
                    other => Err(CodegenError::Internal(format!(
                        "Float value for non-float type {:?}",
                        other
                    ))),
                },
                GlobalInitValue::Address { def, offset } => {
                    let ptr_ty = self.context.ptr_type(AddressSpace::default());
                    let base = if let Some(def_id) = def {
                        if let Some(&global) = self.globals.get(def_id) {
                            global.as_pointer_value()
                        } else if let Some(&function) = self.functions.get(def_id) {
                            function.as_global_value().as_pointer_value()
                        } else {
                            return Err(CodegenError::Internal(format!(
                                "address-of references undeclared symbol {:?}",
                                def_id
                            )));
                        }
                    } else {
                        ptr_ty.const_null()
                    };

                    if *offset != 0 {
                        let i8_ty = self.context.i8_type();
                        let offset_val =
                            self.context.i64_type().const_int(*offset as u64, *offset < 0);
                        let result = const_ptr_offset(base, i8_ty, offset_val);
                        return Ok(result.into());
                    }

                    Ok(base.into())
                }
                GlobalInitValue::StringLiteral(def_id) => {
                    let global = self.get_or_create_string_literal(*def_id)?;
                    Ok(global.as_pointer_value().into())
                }
                GlobalInitValue::Zero => Ok(self.type_cx.basic_type_of(ty)?.const_zero()),
                GlobalInitValue::Error => Err(CodegenError::Internal(
                    "GlobalInitValue::Error should have been rejected earlier".to_owned(),
                )),
            }
        }

        fn zero_bytes(&self, size: u32) -> Vec<BasicValueEnum<'ctx>> {
            let zero = self.context.i8_type().const_zero().into();
            vec![zero; size as usize]
        }

        fn write_bytes_at(
            dst: &mut [BasicValueEnum<'ctx>],
            offset: u64,
            src: Vec<BasicValueEnum<'ctx>>,
        ) {
            let start = offset as usize;
            for (idx, byte) in src.into_iter().enumerate() {
                if let Some(slot) = dst.get_mut(start + idx) {
                    *slot = byte;
                }
            }
        }

        /// Build the object representation bytes for a constant initializer.
        ///
        /// This is used for union members, whose LLVM storage is `{ [N x i8] }`.
        /// For aggregate members, derive offsets from HIR layout metadata instead
        /// of trying to reverse-engineer bytes from an LLVM aggregate constant.
        fn build_const_bytes(
            &mut self,
            ty: TyId,
            entries: &[GlobalInitEntry],
            size: u32,
        ) -> Result<Vec<BasicValueEnum<'ctx>>, CodegenError> {
            let mut bytes = self.zero_bytes(size);
            if entries.is_empty() {
                return Ok(bytes);
            }

            if entries.len() == 1 && entries[0].path.is_empty() {
                let value = self.global_init_value_to_llvm(&entries[0].value, ty)?;
                return Ok(self.collect_scalar_bytes(value, size));
            }

            let layout_cx = LayoutCx::with_defs(self.tcx, &self.hir.defs);
            match self.tcx.get(ty) {
                Ty::Array { elem, len: Some(len), is_vla: false } => {
                    let elem_ty = elem.ty;
                    let elem_size = u32::try_from(
                        layout_cx
                            .array_layout_of(ty)
                            .map_err(|err| type_error(ty, err.to_string()))?
                            .elem
                            .size,
                    )
                    .map_err(|_| {
                        CodegenError::Internal("array element size exceeds u32".to_owned())
                    })?;

                    for i in 0..*len {
                        let sub_entries: Vec<GlobalInitEntry> = entries
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e.path.first(),
                                    Some(GlobalInitDesignator::Index(idx)) if *idx == i
                                )
                            })
                            .cloned()
                            .map(|mut e| {
                                e.path = e.path.into_iter().skip(1).collect();
                                e
                            })
                            .collect();
                        let elem_bytes =
                            self.build_const_bytes(elem_ty, &sub_entries, elem_size)?;
                        Self::write_bytes_at(&mut bytes, i * u64::from(elem_size), elem_bytes);
                    }
                    Ok(bytes)
                }
                Ty::Record(def_id) => {
                    let def_data = self.hir.defs.get(*def_id).ok_or_else(|| {
                        CodegenError::Internal(format!("record definition {:?} not found", def_id))
                    })?;
                    let DefKind::Record { kind, fields, .. } = &def_data.kind else {
                        return Err(CodegenError::Internal(format!(
                            "{:?} is not a record",
                            def_id
                        )));
                    };
                    let kind = *kind;
                    let field_tys = fields.iter().map(|field| field.ty).collect::<Vec<_>>();
                    let record_layout = layout_cx
                        .record_layout_of(ty)
                        .map_err(|err| type_error(ty, err.to_string()))?;

                    match kind {
                        RecordKind::Struct => {
                            for (i, field_ty) in field_tys.into_iter().enumerate() {
                                let field_layout = record_layout.fields[i];
                                let field_size =
                                    u32::try_from(field_layout.storage_size).map_err(|_| {
                                        CodegenError::Internal(
                                            "struct field size exceeds u32".to_owned(),
                                        )
                                    })?;
                                let sub_entries: Vec<GlobalInitEntry> = entries
                                    .iter()
                                    .filter(|e| {
                                        matches!(
                                            e.path.first(),
                                            Some(GlobalInitDesignator::Field(idx)) if *idx as usize == i
                                        )
                                    })
                                    .cloned()
                                    .map(|mut e| {
                                        e.path = e.path.into_iter().skip(1).collect();
                                        e
                                    })
                                    .collect();
                                let field_bytes =
                                    self.build_const_bytes(field_ty, &sub_entries, field_size)?;
                                Self::write_bytes_at(&mut bytes, field_layout.offset, field_bytes);
                            }
                            Ok(bytes)
                        }
                        RecordKind::Union => {
                            if let Some(entry) = entries.iter().find(|e| {
                                matches!(e.path.first(), Some(GlobalInitDesignator::Field(_)))
                            }) {
                                let field_idx = match entry.path.first().unwrap() {
                                    GlobalInitDesignator::Field(idx) => *idx as usize,
                                    _ => unreachable!(),
                                };
                                let field_ty =
                                    field_tys.get(field_idx).copied().ok_or_else(|| {
                                        CodegenError::Internal(format!(
                                            "union field index {} out of bounds",
                                            field_idx
                                        ))
                                    })?;
                                let field_size =
                                    u32::try_from(record_layout.fields[field_idx].storage_size)
                                        .map_err(|_| {
                                            CodegenError::Internal(
                                                "union field size exceeds u32".to_owned(),
                                            )
                                        })?;
                                let sub_entries: Vec<GlobalInitEntry> = entries
                                    .iter()
                                    .filter(|e| {
                                        matches!(
                                            e.path.first(),
                                            Some(GlobalInitDesignator::Field(idx)) if *idx as usize == field_idx
                                        )
                                    })
                                    .cloned()
                                    .map(|mut e| {
                                        e.path = e.path.into_iter().skip(1).collect();
                                        e
                                    })
                                    .collect();
                                let field_bytes =
                                    self.build_const_bytes(field_ty, &sub_entries, field_size)?;
                                Self::write_bytes_at(&mut bytes, 0, field_bytes);
                            }
                            Ok(bytes)
                        }
                    }
                }
                other => Err(CodegenError::Internal(format!(
                    "expected scalar leaf or aggregate type for byte initializer, got {:?}",
                    other
                ))),
            }
        }

        /// Extract the little-endian byte representation of an LLVM scalar
        /// constant value, zero-padding or truncating to `expected_size`.
        fn collect_scalar_bytes(
            &self,
            value: BasicValueEnum<'ctx>,
            expected_size: u32,
        ) -> Vec<BasicValueEnum<'ctx>> {
            let i8_ty = self.context.i8_type();
            let mut bytes = Vec::with_capacity(expected_size as usize);

            match value {
                BasicValueEnum::IntValue(int_val) => {
                    let val = int_val.get_zero_extended_constant().unwrap_or(0);
                    for i in 0..expected_size {
                        let byte = (val >> (i * 8)) & 0xFF;
                        bytes.push(i8_ty.const_int(byte, false).into());
                    }
                }
                BasicValueEnum::FloatValue(float_val) => {
                    let (val, _loses_info) = float_val.get_constant().unwrap_or((0.0, false));
                    let (bits, byte_count) = if float_val.get_type() == self.context.f32_type() {
                        ((val as f32).to_bits() as u64, 4u32)
                    } else {
                        (val.to_bits(), 8u32)
                    };
                    for i in 0..byte_count {
                        let byte = (bits >> (i * 8)) & 0xFF;
                        bytes.push(i8_ty.const_int(byte, false).into());
                    }
                }
                _ => {}
            }

            bytes.truncate(expected_size as usize);
            while bytes.len() < expected_size as usize {
                bytes.push(i8_ty.const_zero().into());
            }
            bytes
        }

        /// Look up (or cache) the LLVM global for a string literal `DefId`.
        ///
        /// The synthetic global's `GlobalInit` is populated by
        /// `rcc_hir_lower::intern_string_literal` with the already-decoded
        /// byte payload, so `materialize_all_globals` handles emitting the
        /// `[N x i8]` constant through the normal array path.  This
        /// function just returns the pointer.
        fn get_or_create_string_literal(
            &mut self,
            def_id: DefId,
        ) -> Result<GlobalValue<'ctx>, CodegenError> {
            if let Some(&global) = self.string_literals.get(&def_id) {
                return Ok(global);
            }

            let global = self.globals.get(&def_id).copied().ok_or_else(|| {
                CodegenError::Internal(format!(
                    "string literal global {:?} was not declared",
                    def_id
                ))
            })?;

            global.set_constant(true);
            self.string_literals.insert(def_id, global);
            Ok(global)
        }
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
        let needs_object =
            session.opts.emit.is_empty() || session.opts.emit.contains(&rcc_session::EmitKind::Obj);
        let needs_assembly = session.opts.emit.contains(&rcc_session::EmitKind::Asm);
        let context = Context::create();
        let mut cx = CodegenCx::new(&context, session, tcx, hir, bodies);
        cx.declare_all()?;
        GlobalCx::new(&cx).materialize_all_globals()?;
        cx.codegen_all_bodies()?;
        cx.finalize_debug_info();
        cx.verify_module()?;
        let ir_text = cx.ir_text();
        let assembly_text = needs_assembly.then(|| cx.assembly_text()).transpose()?;
        let object_bytes = needs_object.then(|| cx.object_bytes()).transpose()?;
        Ok(CodegenArtifact { ir_text, assembly_text, object_bytes })
    }

    fn llvm_opt_level(level: rcc_session::OptLevel) -> OptimizationLevel {
        match level {
            rcc_session::OptLevel::None => OptimizationLevel::None,
            rcc_session::OptLevel::Less => OptimizationLevel::Less,
            rcc_session::OptLevel::Default => OptimizationLevel::Default,
            rcc_session::OptLevel::Aggressive => OptimizationLevel::Aggressive,
        }
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

    fn debug_compile_unit_path(session: &Session) -> (String, String) {
        let Some(path) = session
            .source_map
            .read()
            .ok()
            .and_then(|source_map| source_map.files().next().map(|file| file.name.clone()))
        else {
            return ("<unknown>".to_owned(), ".".to_owned());
        };

        let filename = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.display().to_string());
        let directory = path
            .parent()
            .map(|parent| parent.display().to_string())
            .filter(|dir| !dir.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        (filename, directory)
    }

    fn debug_int_type(signed: bool, rank: IntRank) -> (&'static str, u64, u32) {
        match (signed, rank) {
            (_, IntRank::Bool) => ("_Bool", 8, 0x02),
            (true, IntRank::Char) => ("char", 8, 0x05),
            (false, IntRank::Char) => ("unsigned char", 8, 0x07),
            (true, IntRank::Short) => ("short", 16, 0x05),
            (false, IntRank::Short) => ("unsigned short", 16, 0x07),
            (true, IntRank::Int) => ("int", 32, 0x05),
            (false, IntRank::Int) => ("unsigned int", 32, 0x07),
            (true, IntRank::Long) => ("long", 64, 0x05),
            (false, IntRank::Long) => ("unsigned long", 64, 0x07),
            (true, IntRank::LongLong) => ("long long", 64, 0x05),
            (false, IntRank::LongLong) => ("unsigned long long", 64, 0x07),
        }
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
        Ty::BuiltinVaList => "builtin_va_list".into(),
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

    fn func_ty(tcx: &mut TyCtxt, ret: TyId, params: Vec<TyId>, variadic: bool) -> TyId {
        tcx.intern(Ty::Func { ret, params, variadic, proto: true })
    }

    fn abi_shapes(tcx: &TyCtxt, param: &AbiParam) -> Vec<String> {
        match &param.kind {
            AbiParamKind::Direct(units) => {
                units.iter().map(|unit| abi_unit_shape(tcx, unit.kind)).collect()
            }
            AbiParamKind::Indirect { .. } => vec!["ptr".to_owned()],
        }
    }

    fn return_shape(tcx: &TyCtxt, ret: &AbiReturn) -> String {
        match &ret.kind {
            AbiReturnKind::Void => "void".to_owned(),
            AbiReturnKind::Direct { units, .. } if units.len() == 1 => {
                abi_unit_shape(tcx, units[0].kind)
            }
            AbiReturnKind::Direct { units, .. } => {
                let fields =
                    units.iter().map(|unit| abi_unit_shape(tcx, unit.kind)).collect::<Vec<_>>();
                format!("{{ {} }}", fields.join(", "))
            }
            AbiReturnKind::Indirect { .. } => "void".to_owned(),
        }
    }

    fn return_uses_sret(ret: &AbiReturn) -> bool {
        matches!(ret.kind, AbiReturnKind::Indirect { sret: true, .. })
    }

    fn abi_unit_shape(tcx: &TyCtxt, unit: AbiParamUnitKind) -> String {
        match unit {
            AbiParamUnitKind::Source(ty) => llvm_source_shape(tcx, ty),
            AbiParamUnitKind::Integer { bits } => format!("i{bits}"),
            AbiParamUnitKind::Float(kind) => llvm_float_shape(kind).to_owned(),
            AbiParamUnitKind::Vector { elem, lanes } => {
                format!("<{lanes} x {}>", llvm_float_shape(elem))
            }
        }
    }

    fn llvm_source_shape(tcx: &TyCtxt, ty: TyId) -> String {
        match tcx.get(ty) {
            Ty::Int { rank, .. } => match rank {
                IntRank::Bool => "i1",
                IntRank::Char => "i8",
                IntRank::Short => "i16",
                IntRank::Int => "i32",
                IntRank::Long | IntRank::LongLong => "i64",
            }
            .to_owned(),
            Ty::Float(kind) => llvm_float_shape(*kind).to_owned(),
            Ty::Ptr(_) => "ptr".to_owned(),
            Ty::Enum(_) => "i32".to_owned(),
            other => panic!("unexpected source ABI unit: {other:?}"),
        }
    }

    fn llvm_float_shape(kind: FloatKind) -> &'static str {
        match kind {
            FloatKind::F32 => "float",
            FloatKind::F64 => "double",
            FloatKind::F80 => "x86_fp80",
        }
    }

    #[test]
    fn sysv_abi_classifies_direct_return_shapes_and_bool_zeroext() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let pair = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let pair_ty = tcx.intern(Ty::Record(pair));
        let mix =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.double)]);
        let mix_ty = tcx.intern(Ty::Record(mix));
        let ret_void = tcx.void;
        let ret_bool = tcx.bool_;
        let ret_int = tcx.int;
        let ret_double = tcx.double;

        let cases = [
            (func_ty(&mut tcx, ret_void, Vec::new(), false), "void", false),
            (func_ty(&mut tcx, ret_bool, Vec::new(), false), "i1", true),
            (func_ty(&mut tcx, ret_int, Vec::new(), false), "i32", false),
            (func_ty(&mut tcx, ret_double, Vec::new(), false), "double", false),
            (func_ty(&mut tcx, pair_ty, Vec::new(), false), "i64", false),
            (func_ty(&mut tcx, mix_ty, Vec::new(), false), "{ i32, double }", false),
        ];

        for (fn_ty, expected_shape, expected_zeroext) in cases {
            let abi = sysv_fn_abi(&tcx, &defs, fn_ty).unwrap();

            assert_eq!(return_shape(&tcx, &abi.ret), expected_shape);
            assert_eq!(abi.ret.zeroext, expected_zeroext);
            assert_eq!(abi.fixed_param_count, 0);
        }
    }

    #[test]
    fn sysv_abi_classifies_sret_return_before_user_params() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let big = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![field(tcx.long), field(tcx.long), field(tcx.long)],
        );
        let big_ty = tcx.intern(Ty::Record(big));
        let int = tcx.int;
        let double = tcx.double;
        let fn_ty = func_ty(&mut tcx, big_ty, vec![int, double], false);

        let abi = sysv_fn_abi(&tcx, &defs, fn_ty).unwrap();

        assert_eq!(return_shape(&tcx, &abi.ret), "void");
        assert!(return_uses_sret(&abi.ret));
        assert_eq!(abi.ret.classes, [AbiClass::Memory]);
        assert!(matches!(abi.ret.kind, AbiReturnKind::Indirect { sret: true, align: 8, size: 24 }));
        assert_eq!(abi_shapes(&tcx, &abi.params[0]), ["i32"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[1]), ["double"]);
        assert_eq!(abi.fixed_param_count, 3);
    }

    #[test]
    fn sysv_abi_classifies_scalar_params_and_variadic_boundary() {
        let mut tcx = TyCtxt::new();
        let ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let ret = tcx.void;
        let int = tcx.int;
        let double = tcx.double;
        let fn_ty = func_ty(&mut tcx, ret, vec![int, double, ptr], true);
        let defs = IndexVec::new();

        let abi = sysv_fn_abi(&tcx, &defs, fn_ty).unwrap();

        assert!(abi.variadic);
        assert_eq!(abi.fixed_param_count, 3);
        assert_eq!(abi_shapes(&tcx, &abi.params[0]), ["i32"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[1]), ["double"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[2]), ["ptr"]);
        assert_eq!(abi.params[0].classes, [AbiClass::Integer]);
        assert_eq!(abi.params[1].classes, [AbiClass::Sse]);
        assert_eq!(abi.params[2].classes, [AbiClass::Integer]);
    }

    #[test]
    fn sysv_abi_golden_shapes_match_clang_for_aggregate_params() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let pair = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let pair_ty = tcx.intern(Ty::Record(pair));
        let mix =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.double)]);
        let mix_ty = tcx.intern(Ty::Record(mix));
        let two_floats =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.float), field(tcx.float)]);
        let two_floats_ty = tcx.intern(Ty::Record(two_floats));
        let char_array =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(3), is_vla: false });
        let three_chars = record_def(&mut defs, RecordKind::Struct, vec![field(char_array)]);
        let three_chars_ty = tcx.intern(Ty::Record(three_chars));
        let big = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![field(tcx.long), field(tcx.long), field(tcx.long)],
        );
        let big_ty = tcx.intern(Ty::Record(big));
        let ret = tcx.void;
        let fn_ty = func_ty(
            &mut tcx,
            ret,
            vec![pair_ty, mix_ty, two_floats_ty, three_chars_ty, big_ty],
            false,
        );

        let abi = sysv_fn_abi(&tcx, &defs, fn_ty).unwrap();

        assert_eq!(abi.fixed_param_count, 6);
        assert_eq!(abi_shapes(&tcx, &abi.params[0]), ["i64"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[1]), ["i32", "double"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[2]), ["<2 x float>"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[3]), ["i24"]);
        assert_eq!(abi_shapes(&tcx, &abi.params[4]), ["ptr"]);
        assert_eq!(abi.params[1].classes, [AbiClass::Integer, AbiClass::Sse]);
        assert_eq!(abi.params[4].classes, [AbiClass::Memory]);
        assert!(matches!(
            abi.params[4].kind,
            AbiParamKind::Indirect { byval: true, align: 8, size: 24 }
        ));
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

    // -----------------------------------------------------------------------
    // 09-09: Place address, operand load, and store helpers
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    use rcc_cfg::{
        BasicBlock, BasicBlockId, BinOp, Body, CastKind, Const, ConstKind, LocalDecl, Operand,
        Place, Projection, Rvalue, Statement, StatementKind, Terminator, TerminatorKind, UnOp,
    };
    #[cfg(feature = "llvm")]
    use rcc_hir::Local;

    // -----------------------------------------------------------------------
    // 09-10: Entry-block alloca and local materialization
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn cfg_local_decl(name: Option<Symbol>, ty: TyId, is_param: bool) -> LocalDecl {
        LocalDecl {
            name,
            ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param,
            span: DUMMY_SP,
        }
    }

    #[cfg(feature = "llvm")]
    fn local_materialization_body(
        def: DefId,
        tcx: &TyCtxt,
        param_name: Symbol,
        local_name: Symbol,
    ) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(cfg_local_decl(Some(param_name), tcx.int, true));
        locals.push(cfg_local_decl(Some(local_name), tcx.int, false));
        let mut blocks = IndexVec::new();
        blocks.push(rcc_cfg::BasicBlock::default());
        Body { def: Some(def), locals, blocks, ret_ty: Some(tcx.void) }
    }

    #[cfg(feature = "llvm")]
    fn materialization_inputs(
        session: &mut Session,
        tcx: &mut TyCtxt,
    ) -> (HirCrate, FxHashMap<DefId, Body>, DefId, Body) {
        let fn_ty = tcx.intern(Ty::Func {
            ret: tcx.void,
            params: vec![tcx.int],
            variadic: false,
            proto: true,
        });
        let fn_name = session.interner.intern("materialize_one_int");
        let param_name = session.interner.intern("p");
        let local_name = session.interner.intern("x");
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let body = local_materialization_body(def, tcx, param_name, local_name);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        (hir, bodies, def, body)
    }

    /// Local materialization inserts all allocas before any non-alloca entry instruction.
    #[cfg(feature = "llvm")]
    #[test]
    fn materialize_locals_keeps_allocas_first() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let (hir, bodies, def, body) = materialization_inputs(&mut session, &mut tcx);
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        let function = cx.declare_function(def).unwrap();
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);

        let sink = cx.module().add_global(context.i32_type(), None, "__sink");
        sink.set_initializer(&context.i32_type().const_zero());
        cx.builder()
            .build_store(sink.as_pointer_value(), context.i32_type().const_int(1, false))
            .unwrap();

        let locals = cx.materialize_locals(function, &body).unwrap();
        assert_eq!(locals.len(), body.locals.len());
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();

        let entry = function.get_first_basic_block().unwrap();
        let mut seen_non_alloca = false;
        let mut alloca_count = 0;
        for instruction in entry.get_instructions() {
            if instruction.get_opcode() == inkwell::values::InstructionOpcode::Alloca {
                assert!(!seen_non_alloca, "alloca appeared after a non-alloca instruction");
                alloca_count += 1;
            } else {
                seen_non_alloca = true;
            }
        }
        assert_eq!(alloca_count, body.locals.len());
    }

    /// Scalar parameters are stored into their local slots during materialization.
    #[cfg(feature = "llvm")]
    #[test]
    fn materialize_locals_stores_scalar_params() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let (hir, bodies, def, body) = materialization_inputs(&mut session, &mut tcx);
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        let function = cx.declare_function(def).unwrap();
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);

        let _locals = cx.materialize_locals(function, &body).unwrap();
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("store i32 %0, ptr %param"));
    }

    /// StorageLive/StorageDead lower to LLVM lifetime intrinsics.
    #[cfg(feature = "llvm")]
    #[test]
    fn storage_markers_emit_lifetime_intrinsics() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let (hir, bodies, def, body) = materialization_inputs(&mut session, &mut tcx);
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        let function = cx.declare_function(def).unwrap();
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);
        let mut locals = cx.materialize_locals(function, &body).unwrap();
        let mut vla_stacks = backend::VlaStackMap::with_capacity(body.locals.len());
        for _ in body.locals.iter() {
            vla_stacks.push(None);
        }

        cx.emit_storage_live(Local(2), &mut locals, &mut vla_stacks, &body).unwrap();
        cx.emit_storage_dead(Local(2), &locals, &mut vla_stacks, &body).unwrap();
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("@llvm.lifetime.start.p0"));
        assert!(ir.contains("@llvm.lifetime.end.p0"));
        assert!(ir.contains("i64 4"));
    }

    #[cfg(feature = "llvm")]
    fn cfg_vla_local_decl(name: Option<Symbol>, ty: TyId, vla_len: Local) -> LocalDecl {
        LocalDecl {
            name,
            ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: Some(vla_len),
            is_param: false,
            span: DUMMY_SP,
        }
    }

    #[cfg(feature = "llvm")]
    fn vla_ty(tcx: &mut TyCtxt) -> TyId {
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: true })
    }

    // -----------------------------------------------------------------------
    // 09-17: VLA stack allocation and length values
    // -----------------------------------------------------------------------

    /// VLA locals do not get an entry-block alloca during materialization.
    #[cfg(feature = "llvm")]
    #[test]
    fn vla_materialization_skips_entry_alloca() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let vla_ty = vla_ty(&mut tcx);
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(cfg_local_decl(None, tcx.ulong, false));
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(1)));
        let body = cfg_body_with_locals(tcx.void, locals, vec![BasicBlock::default()]);

        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        let function = cx.module().add_function(
            "__vla_materialize",
            context.void_type().fn_type(&[], false),
            None,
        );
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);

        let llvm_locals = cx.materialize_locals(function, &body).unwrap();
        assert_eq!(llvm_locals.len(), body.locals.len());
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();

        let entry = function.get_first_basic_block().unwrap();
        let mut alloca_names = Vec::new();
        for instruction in entry.get_instructions() {
            if instruction.get_opcode() == inkwell::values::InstructionOpcode::Alloca {
                alloca_names.push(
                    instruction
                        .get_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            }
        }
        assert!(
            !alloca_names.iter().any(|n| n.contains("local2") || n.contains("tmp2")),
            "VLA local should not have an entry alloca, got: {:?}",
            alloca_names
        );
    }

    /// Dynamic VLA alloca is emitted at the `StorageLive` point (non-entry block).
    #[cfg(feature = "llvm")]
    #[test]
    fn vla_storage_live_emits_dynamic_alloca() {
        let mut tcx = TyCtxt::new();
        let vla_ty = vla_ty(&mut tcx);
        let ulong = tcx.ulong;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false)); // ret
        locals.push(cfg_local_decl(None, ulong, false)); // len
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(1))); // vla

        let entry_block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: Place { base: Local(1), projection: vec![] },
                    rvalue: Rvalue::Use(int_const(ulong, 5)),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Goto(BasicBlockId(1)),
        );
        let live_block = cfg_block(
            vec![Statement { kind: StatementKind::StorageLive(Local(2)), span: DUMMY_SP }],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![entry_block, live_block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__vla_live", ret_ty, body);

        assert!(ir.contains("bb1:"), "IR should contain bb1:\n{ir}");
        let after_bb1 = ir.split("bb1:").nth(1).expect("bb1 present");
        assert!(after_bb1.contains("alloca"), "dynamic alloca expected in bb1:\n{ir}");
    }

    /// `Rvalue::Len(place)` loads the saved runtime length local.
    #[cfg(feature = "llvm")]
    #[test]
    fn vla_len_rvalue_loads_saved_length() {
        let mut tcx = TyCtxt::new();
        let vla_ty = vla_ty(&mut tcx);
        let ulong = tcx.ulong;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ulong, false)); // ret
        locals.push(cfg_local_decl(None, ulong, false)); // len
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(1))); // vla

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::Use(int_const(ulong, 7)),
                    },
                    span: DUMMY_SP,
                },
                Statement { kind: StatementKind::StorageLive(Local(2)), span: DUMMY_SP },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(0), projection: vec![] },
                        rvalue: Rvalue::Len(local_place(Local(2))),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );

        let body = cfg_body_with_locals(ulong, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__vla_len", ulong, body);

        assert!(ir.contains("load i64"), "IR should load i64 length:\n{ir}");
    }

    /// VLA element access uses a single-index GEP over the element type.
    #[cfg(feature = "llvm")]
    #[test]
    fn vla_index_addressing_emits_gep() {
        let mut tcx = TyCtxt::new();
        let vla_ty = vla_ty(&mut tcx);
        let ulong = tcx.ulong;
        let int = tcx.int;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false)); // ret
        locals.push(cfg_local_decl(None, ulong, false)); // len
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(1))); // vla

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::Use(int_const(ulong, 5)),
                    },
                    span: DUMMY_SP,
                },
                Statement { kind: StatementKind::StorageLive(Local(2)), span: DUMMY_SP },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place {
                            base: Local(2),
                            projection: vec![Projection::Index(int_const(int, 2))],
                        },
                        rvalue: Rvalue::Use(int_const(int, 42)),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__vla_index", ret_ty, body);

        assert!(ir.contains("getelementptr i32"), "IR should contain GEP over i32:\n{ir}");
    }

    /// Nested VLA locals restore dynamic stack allocations on StorageDead.
    #[cfg(feature = "llvm")]
    #[test]
    fn nested_vla_storage_live_dead_does_not_error() {
        let mut tcx = TyCtxt::new();
        let vla_ty = vla_ty(&mut tcx);
        let ulong = tcx.ulong;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false)); // ret
        locals.push(cfg_local_decl(None, ulong, false)); // len_a
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(1))); // vla_a
        locals.push(cfg_local_decl(None, ulong, false)); // len_b
        locals.push(cfg_vla_local_decl(None, vla_ty, Local(3))); // vla_b

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::Use(int_const(ulong, 3)),
                    },
                    span: DUMMY_SP,
                },
                Statement { kind: StatementKind::StorageLive(Local(2)), span: DUMMY_SP },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(3), projection: vec![] },
                        rvalue: Rvalue::Use(int_const(ulong, 4)),
                    },
                    span: DUMMY_SP,
                },
                Statement { kind: StatementKind::StorageLive(Local(4)), span: DUMMY_SP },
                Statement { kind: StatementKind::StorageDead(Local(4)), span: DUMMY_SP },
                Statement { kind: StatementKind::StorageDead(Local(2)), span: DUMMY_SP },
            ],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__vla_nested", ret_ty, body);

        assert!(ir.contains("@llvm.stacksave.p0"), "IR should save VLA stack state:\n{ir}");
        assert!(ir.contains("@llvm.stackrestore.p0"), "IR should restore VLA stack state:\n{ir}");
    }

    // -----------------------------------------------------------------------
    // 09-18: Complex rvalue emission
    // -----------------------------------------------------------------------

    /// `ComplexFromReal` constructs `{ real, 0.0 }` with the target complex layout.
    #[cfg(feature = "llvm")]
    #[test]
    fn complex_from_real_constructs_zero_imaginary() {
        let mut tcx = TyCtxt::new();
        let double = tcx.double;
        let complex = tcx.complex_double;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(cfg_local_decl(None, complex, false));

        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: Place { base: Local(1), projection: vec![] },
                    rvalue: Rvalue::ComplexFromReal { real: float_const(double, 1.5), to: complex },
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__complex_from_real", ret_ty, body);

        assert!(
            ir.contains("{ double 1.500000e+00, double 0.000000e+00 }"),
            "complex real plus zero imaginary expected:\n{ir}"
        );
    }

    /// `RealFromComplex` extracts only the real component and ignores the imaginary field.
    #[cfg(feature = "llvm")]
    #[test]
    fn real_from_complex_extracts_only_real_component() {
        let mut tcx = TyCtxt::new();
        let double = tcx.double;
        let complex = tcx.complex_double;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, double, false));
        locals.push(cfg_local_decl(None, complex, false));

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::ComplexFromReal {
                            real: float_const(double, 3.25),
                            to: complex,
                        },
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(0), projection: vec![] },
                        rvalue: Rvalue::RealFromComplex {
                            complex: Operand::Copy(local_place(Local(1))),
                            to: double,
                        },
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );

        let body = cfg_body_with_locals(double, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__complex_to_real", double, body);
        let extracts = ir
            .lines()
            .filter(|line| line.contains("extractvalue { double, double }"))
            .collect::<Vec<_>>();

        assert!(extracts.iter().any(|line| line.contains(", 0")), "real extract expected:\n{ir}");
        assert!(
            !extracts.iter().any(|line| line.contains(", 1")),
            "imaginary extract must not be emitted:\n{ir}"
        );
    }

    /// Complex locals can be loaded, assigned, and stored without layout mismatch.
    #[cfg(feature = "llvm")]
    #[test]
    fn complex_assignment_passes_through_locals() {
        let mut tcx = TyCtxt::new();
        let double = tcx.double;
        let complex = tcx.complex_double;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(cfg_local_decl(None, complex, false));
        locals.push(cfg_local_decl(None, complex, false));

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::ComplexFromReal {
                            real: float_const(double, 2.0),
                            to: complex,
                        },
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(2), projection: vec![] },
                        rvalue: Rvalue::Use(Operand::Copy(local_place(Local(1)))),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__complex_assign", ret_ty, body);

        assert!(ir.contains("load { double, double }"), "complex load expected:\n{ir}");
        assert!(ir.contains("store { double, double }"), "complex store expected:\n{ir}");
    }

    /// Complex multiplication lowers to component-wise floating arithmetic.
    #[cfg(feature = "llvm")]
    #[test]
    fn complex_multiply_emits_component_arithmetic() {
        let mut tcx = TyCtxt::new();
        let double = tcx.double;
        let complex = tcx.complex_double;

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(cfg_local_decl(None, complex, false));
        locals.push(cfg_local_decl(None, complex, false));
        locals.push(cfg_local_decl(None, complex, false));

        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(1), projection: vec![] },
                        rvalue: Rvalue::ComplexFromReal {
                            real: float_const(double, 2.0),
                            to: complex,
                        },
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(2), projection: vec![] },
                        rvalue: Rvalue::ComplexFromReal {
                            real: float_const(double, 3.0),
                            to: complex,
                        },
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: Place { base: Local(3), projection: vec![] },
                        rvalue: Rvalue::BinaryOp(
                            BinOp::FMul,
                            Operand::Copy(local_place(Local(1))),
                            Operand::Copy(local_place(Local(2))),
                        ),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );

        let ret_ty = tcx.void;
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__complex_mul", ret_ty, body);

        assert!(ir.contains("fmul double"), "complex multiply should multiply components:\n{ir}");
        assert!(ir.contains("fsub double"), "complex multiply should compute real part:\n{ir}");
        assert!(
            ir.contains("fadd double"),
            "complex multiply should compute imaginary part:\n{ir}"
        );
    }

    // -----------------------------------------------------------------------
    // 09-12: Basic-block and terminator wiring
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn cfg_block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlock {
        BasicBlock { statements, terminator: Terminator { kind, span: DUMMY_SP } }
    }

    #[cfg(feature = "llvm")]
    fn cfg_body(ret_ty: TyId, blocks: Vec<BasicBlock>) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        cfg_body_with_locals(ret_ty, locals, blocks)
    }

    #[cfg(feature = "llvm")]
    fn cfg_body_with_locals(
        ret_ty: TyId,
        locals: IndexVec<Local, LocalDecl>,
        blocks: Vec<BasicBlock>,
    ) -> Body {
        let mut cfg_blocks = IndexVec::new();
        for block in blocks {
            cfg_blocks.push(block);
        }
        Body { def: None, locals, blocks: cfg_blocks, ret_ty: Some(ret_ty) }
    }

    #[cfg(feature = "llvm")]
    fn int_const(ty: TyId, value: i128) -> Operand {
        Operand::Const(Const { kind: ConstKind::Int(value), ty })
    }

    #[cfg(feature = "llvm")]
    fn float_const(ty: TyId, value: f64) -> Operand {
        Operand::Const(Const { kind: ConstKind::Float(value), ty })
    }

    #[cfg(feature = "llvm")]
    fn ret_slot() -> Place {
        Place { base: Local(0), projection: Vec::new() }
    }

    #[cfg(feature = "llvm")]
    fn assign_ret(ty: TyId, value: i128) -> Statement {
        Statement {
            kind: StatementKind::Assign {
                place: ret_slot(),
                rvalue: Rvalue::Use(int_const(ty, value)),
            },
            span: DUMMY_SP,
        }
    }

    #[cfg(feature = "llvm")]
    fn codegen_fixture_ir(
        session: &mut Session,
        tcx: &mut TyCtxt,
        name: &str,
        ret_ty: TyId,
        mut body: Body,
    ) -> Result<String, CodegenError> {
        let fn_ty = func_ty(tcx, ret_ty, Vec::new(), false);
        let fn_name = session.interner.intern(name);
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        body.def = Some(def);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);

        codegen(session, tcx, &hir, &bodies).map(|artifact| artifact.ir_text)
    }

    #[cfg(feature = "llvm")]
    fn assert_codegen_fixture_verifies(
        tcx: &mut TyCtxt,
        name: &str,
        ret_ty: TyId,
        body: Body,
    ) -> String {
        let (mut session, _cap) = Session::for_test();
        codegen_fixture_ir(&mut session, tcx, name, ret_ty, body).unwrap()
    }

    #[cfg(feature = "llvm")]
    fn assert_codegen_fixture_mem2reg(
        tcx: &mut TyCtxt,
        name: &str,
        ret_ty: TyId,
        mut body: Body,
    ) -> String {
        let (mut session, _cap) = Session::for_test();
        let fn_ty = func_ty(tcx, ret_ty, Vec::new(), false);
        let fn_name = session.interner.intern(name);
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        body.def = Some(def);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);

        let context = inkwell::context::Context::create();
        let mut cx = backend::CodegenCx::new(&context, &mut session, tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        cx.codegen_all_bodies().unwrap();
        cx.verify_module().unwrap();
        cx.run_mem2reg_for_tests().unwrap();
        cx.ir_text()
    }

    #[cfg(feature = "llvm")]
    fn matching_line_count(ir: &str, needle: &str) -> usize {
        ir.lines().filter(|line| line.contains(needle)).count()
    }

    /// A simple `return 42;` body emits a valid LLVM return from the CFG return slot.
    #[cfg(feature = "llvm")]
    #[test]
    fn simple_return_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.int;
        let body =
            cfg_body(ret_ty, vec![cfg_block(vec![assign_ret(ret_ty, 42)], TerminatorKind::Return)]);

        let ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_return", ret_ty, body);

        assert!(ir.contains("ret i32 42") || ir.contains("ret i32 %load"), "IR:\n{ir}");
    }

    /// `if`-shaped CFG lowers a `SwitchInt` diamond and verifies as an LLVM module.
    #[cfg(feature = "llvm")]
    #[test]
    fn if_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.int;
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(ret_ty, 1),
                targets: vec![(Some(0), BasicBlockId(2)), (None, BasicBlockId(1))],
            },
        );
        let then_bb = cfg_block(vec![assign_ret(ret_ty, 1)], TerminatorKind::Goto(BasicBlockId(3)));
        let else_bb = cfg_block(vec![assign_ret(ret_ty, 0)], TerminatorKind::Goto(BasicBlockId(3)));
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let body = cfg_body(ret_ty, vec![entry, then_bb, else_bb, join]);

        let ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_if", ret_ty, body);

        assert!(ir.contains("switch i32 1"), "IR:\n{ir}");
    }

    /// `while`-shaped CFG verifies with entry/header/body/exit branch wiring.
    #[cfg(feature = "llvm")]
    #[test]
    fn while_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let int_ty = tcx.int;
        let entry = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let header = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(int_ty, 1),
                targets: vec![(Some(0), BasicBlockId(3)), (None, BasicBlockId(2))],
            },
        );
        let body_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let exit = cfg_block(Vec::new(), TerminatorKind::Return);
        let body = cfg_body(ret_ty, vec![entry, header, body_bb, exit]);

        let _ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_while", ret_ty, body);
    }

    /// `for`-shaped CFG verifies with init, header, body, step, and exit blocks.
    #[cfg(feature = "llvm")]
    #[test]
    fn for_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let int_ty = tcx.int;
        let init = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let header = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(int_ty, 1),
                targets: vec![(Some(0), BasicBlockId(4)), (None, BasicBlockId(2))],
            },
        );
        let body_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(3)));
        let step = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let exit = cfg_block(Vec::new(), TerminatorKind::Return);
        let body = cfg_body(ret_ty, vec![init, header, body_bb, step, exit]);

        let _ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_for", ret_ty, body);
    }

    /// `break`-shaped CFG verifies when a loop body jumps directly to the exit block.
    #[cfg(feature = "llvm")]
    #[test]
    fn break_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let int_ty = tcx.int;
        let entry = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let header = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(int_ty, 1),
                targets: vec![(Some(0), BasicBlockId(3)), (None, BasicBlockId(2))],
            },
        );
        let body_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(3)));
        let exit = cfg_block(Vec::new(), TerminatorKind::Return);
        let body = cfg_body(ret_ty, vec![entry, header, body_bb, exit]);

        let _ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_break", ret_ty, body);
    }

    /// `continue`-shaped CFG verifies when a loop body jumps back to the header.
    #[cfg(feature = "llvm")]
    #[test]
    fn continue_fixture_verifies() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let int_ty = tcx.int;
        let entry = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let header = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(int_ty, 1),
                targets: vec![(Some(0), BasicBlockId(3)), (None, BasicBlockId(2))],
            },
        );
        let body_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(1)));
        let exit = cfg_block(Vec::new(), TerminatorKind::Return);
        let body = cfg_body(ret_ty, vec![entry, header, body_bb, exit]);

        let _ir = assert_codegen_fixture_verifies(&mut tcx, "__cfg_continue", ret_ty, body);
    }

    /// Bad branch targets are rejected before LLVM receives malformed IR.
    #[cfg(feature = "llvm")]
    #[test]
    fn invalid_branch_target_is_codegen_error() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let body =
            cfg_body(ret_ty, vec![cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(99)))]);

        let err = codegen_fixture_ir(&mut session, &mut tcx, "__cfg_bad_target", ret_ty, body)
            .unwrap_err();

        assert!(err.to_string().contains("InvalidBlockTarget"), "{err}");
    }

    /// A reachable default `Unreachable` sentinel is reported as a missing CFG terminator.
    #[cfg(feature = "llvm")]
    #[test]
    fn missing_cfg_terminator_is_codegen_error() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let body = cfg_body(ret_ty, vec![BasicBlock::default()]);

        let err = codegen_fixture_ir(&mut session, &mut tcx, "__cfg_missing_term", ret_ty, body)
            .unwrap_err();

        assert!(err.to_string().contains("ReachableUnreachableTerminator"), "{err}");
    }

    /// Every emitted LLVM block has exactly one terminator instruction.
    #[cfg(feature = "llvm")]
    #[test]
    fn branch_wiring_emits_one_terminator_per_block() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let int_ty = tcx.int;
        let fn_ty = func_ty(&mut tcx, ret_ty, Vec::new(), false);
        let fn_name = session.interner.intern("__cfg_terms");
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::SwitchInt {
                discr: int_const(int_ty, 1),
                targets: vec![(Some(0), BasicBlockId(2)), (None, BasicBlockId(1))],
            },
        );
        let then_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(3)));
        let else_bb = cfg_block(Vec::new(), TerminatorKind::Goto(BasicBlockId(3)));
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let mut body = cfg_body(ret_ty, vec![entry, then_bb, else_bb, join]);
        body.def = Some(def);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body.clone());
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        let function = cx.declare_function(def).unwrap();

        cx.codegen_body(function, &body).unwrap();
        cx.verify_module().unwrap();

        for block in function.get_basic_blocks() {
            let terminators = block
                .get_instructions()
                .filter(|inst| {
                    matches!(
                        inst.get_opcode(),
                        inkwell::values::InstructionOpcode::Br
                            | inkwell::values::InstructionOpcode::Return
                            | inkwell::values::InstructionOpcode::Switch
                            | inkwell::values::InstructionOpcode::Unreachable
                    )
                })
                .count();
            assert_eq!(terminators, 1);
        }
    }

    // -----------------------------------------------------------------------
    // 09-13: Call emission with ABI lowering
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn call_global(def: DefId, ty: TyId) -> Operand {
        Operand::Const(Const { kind: ConstKind::Global(def), ty })
    }

    #[cfg(feature = "llvm")]
    fn local_place(local: Local) -> Place {
        Place { base: local, projection: Vec::new() }
    }

    #[cfg(feature = "llvm")]
    fn local_copy(local: Local) -> Operand {
        Operand::Copy(local_place(local))
    }

    // -----------------------------------------------------------------------
    // 09-14: Binary and unary op emission
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn binop_return_body(ret_ty: TyId, op: BinOp, lhs: Operand, rhs: Operand) -> Body {
        cfg_body(
            ret_ty,
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::BinaryOp(op, lhs, rhs),
                    },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        )
    }

    #[cfg(feature = "llvm")]
    fn local_binop_return_body(ret_ty: TyId, lhs_ty: TyId, rhs_ty: TyId, op: BinOp) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(410)), lhs_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(411)), rhs_ty, false));
        cfg_body_with_locals(
            ret_ty,
            locals,
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::BinaryOp(op, local_copy(Local(1)), local_copy(Local(2))),
                    },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        )
    }

    #[cfg(feature = "llvm")]
    fn local_unop_return_body(ret_ty: TyId, operand_ty: TyId, op: UnOp) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(412)), operand_ty, false));
        cfg_body_with_locals(
            ret_ty,
            locals,
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::UnaryOp(op, local_copy(Local(1))),
                    },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        )
    }

    /// Integer, bitwise, and comparison binops emit the expected LLVM opcode
    /// while preserving the CFG result type.
    #[cfg(feature = "llvm")]
    #[test]
    fn binop_integer_opcode_table() {
        let int_cases: &[(BinOp, &str, &str)] = &[
            (BinOp::Add, " add ", "i32"),
            (BinOp::Sub, " sub ", "i32"),
            (BinOp::Mul, " mul ", "i32"),
            (BinOp::SDiv, " sdiv ", "i32"),
            (BinOp::UDiv, " udiv ", "i32"),
            (BinOp::SRem, " srem ", "i32"),
            (BinOp::URem, " urem ", "i32"),
            (BinOp::Shl, " shl ", "i32"),
            (BinOp::AShr, " ashr ", "i32"),
            (BinOp::LShr, " lshr ", "i32"),
            (BinOp::BitAnd, " and ", "i32"),
            (BinOp::BitXor, " xor ", "i32"),
            (BinOp::BitOr, " or ", "i32"),
        ];
        for (op, opcode, result_ty) in int_cases {
            let mut tcx = TyCtxt::new();
            let int_ty = tcx.int;
            let body = local_binop_return_body(int_ty, int_ty, int_ty, *op);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__binop_{op:?}").to_lowercase(),
                int_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing opcode {opcode:?} in IR:\n{ir}");
            assert!(ir.contains(&format!("{opcode}{result_ty}")), "bad result type in IR:\n{ir}");
        }

        let cmp_cases: &[(BinOp, &str)] = &[
            (BinOp::Eq, "icmp eq i32"),
            (BinOp::Ne, "icmp ne i32"),
            (BinOp::SLt, "icmp slt i32"),
            (BinOp::SLe, "icmp sle i32"),
            (BinOp::SGt, "icmp sgt i32"),
            (BinOp::SGe, "icmp sge i32"),
            (BinOp::ULt, "icmp ult i32"),
            (BinOp::ULe, "icmp ule i32"),
            (BinOp::UGt, "icmp ugt i32"),
            (BinOp::UGe, "icmp uge i32"),
        ];
        for (op, opcode) in cmp_cases {
            let mut tcx = TyCtxt::new();
            let int_ty = tcx.int;
            let body = local_binop_return_body(int_ty, int_ty, int_ty, *op);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__binop_{op:?}").to_lowercase(),
                int_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing opcode {opcode:?} in IR:\n{ir}");
            assert!(ir.contains("zext i1"), "comparison did not widen to int in IR:\n{ir}");
        }
    }

    /// Floating binops and unary `FNeg` use floating LLVM opcodes and result
    /// types.
    #[cfg(feature = "llvm")]
    #[test]
    fn binop_float_opcode_table() {
        let cases: &[(BinOp, &str)] = &[
            (BinOp::FAdd, " fadd double"),
            (BinOp::FSub, " fsub double"),
            (BinOp::FMul, " fmul double"),
            (BinOp::FDiv, " fdiv double"),
            (BinOp::FLt, "fcmp olt double"),
            (BinOp::FLe, "fcmp ole double"),
            (BinOp::FGt, "fcmp ogt double"),
            (BinOp::FGe, "fcmp oge double"),
            (BinOp::Eq, "fcmp oeq double"),
            (BinOp::Ne, "fcmp one double"),
        ];
        for (op, opcode) in cases {
            let mut tcx = TyCtxt::new();
            let result_ty = if matches!(op, BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv) {
                tcx.double
            } else {
                tcx.int
            };
            let body =
                binop_return_body(result_ty, *op, local_copy(Local(1)), local_copy(Local(2)));
            let mut body = body;
            let mut locals = IndexVec::new();
            locals.push(cfg_local_decl(None, result_ty, false));
            locals.push(cfg_local_decl(Some(Symbol(413)), tcx.double, false));
            locals.push(cfg_local_decl(Some(Symbol(414)), tcx.double, false));
            body.locals = locals;
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__float_binop_{op:?}").to_lowercase(),
                result_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing opcode {opcode:?} in IR:\n{ir}");
        }
    }

    /// Unary op emission covers integer negate, floating negate, bitwise not,
    /// and logical not.
    #[cfg(feature = "llvm")]
    #[test]
    fn unop_opcode_table() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_cases: &[(UnOp, &str)] = &[
            (UnOp::Neg, " sub i32 0,"),
            (UnOp::BitNot, " xor i32"),
            (UnOp::LogNot, "icmp eq i32"),
        ];
        for (op, opcode) in int_cases {
            let body = local_unop_return_body(int_ty, int_ty, *op);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__unop_{op:?}").to_lowercase(),
                int_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing opcode {opcode:?} in IR:\n{ir}");
        }

        let double_ty = tcx.double;
        let body = local_unop_return_body(double_ty, double_ty, UnOp::FNeg);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__unop_fneg", double_ty, body);
        assert!(ir.contains(" fneg double"), "missing fneg in IR:\n{ir}");
    }

    /// Pointer arithmetic is expressed as typed GEP over the pointed-to
    /// element, and pointer difference divides the byte delta by element size.
    #[cfg(feature = "llvm")]
    #[test]
    fn pointer_arithmetic_uses_element_layout() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(int_ty)));

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ptr_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(401)), ptr_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(402)), ptr_ty, false));

        let add_body = cfg_body_with_locals(
            ptr_ty,
            locals.clone(),
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::BinaryOp(
                            BinOp::PtrAdd,
                            local_copy(Local(1)),
                            int_const(int_ty, 3),
                        ),
                    },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        );
        let add_ir = assert_codegen_fixture_verifies(&mut tcx, "__ptr_add", ptr_ty, add_body);
        assert!(add_ir.contains("getelementptr i32"), "IR:\n{add_ir}");

        let long_ty = tcx.long;
        let mut diff_locals = IndexVec::new();
        diff_locals.push(cfg_local_decl(None, long_ty, false));
        diff_locals.push(cfg_local_decl(Some(Symbol(403)), ptr_ty, false));
        diff_locals.push(cfg_local_decl(Some(Symbol(404)), ptr_ty, false));
        let diff_body =
            binop_return_body(long_ty, BinOp::PtrDiff, local_copy(Local(1)), local_copy(Local(2)));
        let mut diff_body = diff_body;
        diff_body.locals = diff_locals;
        let diff_ir = assert_codegen_fixture_verifies(&mut tcx, "__ptr_diff", long_ty, diff_body);
        assert!(diff_ir.contains("ptrtoint ptr"), "IR:\n{diff_ir}");
        assert!(diff_ir.contains("sdiv i64") && diff_ir.contains(", 4"), "IR:\n{diff_ir}");
    }

    // -----------------------------------------------------------------------
    // 09-15: Cast emission
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn cast_return_body(ret_ty: TyId, operand_ty: TyId, kind: CastKind) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(420)), operand_ty, false));
        cfg_body_with_locals(
            ret_ty,
            locals,
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::Cast { op: local_copy(Local(1)), to: ret_ty, kind },
                    },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        )
    }

    #[cfg(feature = "llvm")]
    type PickTy = fn(&TyCtxt) -> TyId;

    #[cfg(feature = "llvm")]
    type IntCastCase = (PickTy, PickTy, &'static str, &'static str);

    #[cfg(feature = "llvm")]
    type IntToFloatCase = (PickTy, &'static str);

    #[cfg(feature = "llvm")]
    type FloatCastCase = (PickTy, PickTy, CastKind, &'static str, &'static str);

    /// Integer casts choose truncation, sign extension, or zero extension from
    /// HIR source/target types and materialize the destination LLVM type.
    #[cfg(feature = "llvm")]
    #[test]
    fn cast_int_to_int_opcode_table() {
        let cases: &[IntCastCase] = &[
            (|tcx| tcx.long, |tcx| tcx.int, "trunc i64", "store i32"),
            (|tcx| tcx.int, |tcx| tcx.long, "sext i32", "store i64"),
            (|tcx| tcx.uint, |tcx| tcx.ulong, "zext i32", "store i64"),
            (|tcx| tcx.int, |tcx| tcx.bool_, "icmp ne i32", "store i1"),
        ];

        for (src, dst, opcode, store_ty) in cases {
            let mut tcx = TyCtxt::new();
            let src_ty = src(&tcx);
            let dst_ty = dst(&tcx);
            let body = cast_return_body(dst_ty, src_ty, CastKind::IntToInt);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__cast_{opcode}").replace([' ', '\t'], "_"),
                dst_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing cast opcode {opcode:?} in IR:\n{ir}");
            assert!(ir.contains(store_ty), "missing destination type {store_ty:?} in IR:\n{ir}");
        }
    }

    /// Integer to float casts use source signedness from HIR, not the runtime
    /// integer value.
    #[cfg(feature = "llvm")]
    #[test]
    fn cast_int_to_float_uses_source_signedness() {
        let cases: &[IntToFloatCase] =
            &[(|tcx| tcx.int, "sitofp i32"), (|tcx| tcx.uint, "uitofp i32")];

        for (src, opcode) in cases {
            let mut tcx = TyCtxt::new();
            let src_ty = src(&tcx);
            let dst_ty = tcx.double;
            let body = cast_return_body(dst_ty, src_ty, CastKind::IntToFloat);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__cast_{opcode}").replace([' ', '\t'], "_"),
                dst_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing signedness opcode {opcode:?} in IR:\n{ir}");
            assert!(ir.contains("to double"), "missing double destination in IR:\n{ir}");
        }
    }

    /// Float casts cover float/int, float/float, and `_Bool` normalization.
    #[cfg(feature = "llvm")]
    #[test]
    fn cast_float_opcode_table() {
        let cases: &[FloatCastCase] = &[
            (|tcx| tcx.double, |tcx| tcx.int, CastKind::FloatToInt, "fptosi double", "store i32"),
            (|tcx| tcx.double, |tcx| tcx.uint, CastKind::FloatToInt, "fptoui double", "store i32"),
            (
                |tcx| tcx.float,
                |tcx| tcx.double,
                CastKind::FloatToFloat,
                "fpext float",
                "store double",
            ),
            (
                |tcx| tcx.double,
                |tcx| tcx.float,
                CastKind::FloatToFloat,
                "fptrunc double",
                "store float",
            ),
            (
                |tcx| tcx.double,
                |tcx| tcx.bool_,
                CastKind::FloatToInt,
                "fcmp one double",
                "store i1",
            ),
        ];

        for (src, dst, kind, opcode, store_ty) in cases {
            let mut tcx = TyCtxt::new();
            let src_ty = src(&tcx);
            let dst_ty = dst(&tcx);
            let body = cast_return_body(dst_ty, src_ty, *kind);
            let ir = assert_codegen_fixture_verifies(
                &mut tcx,
                &format!("__cast_{opcode}").replace([' ', '\t'], "_"),
                dst_ty,
                body,
            );
            assert!(ir.contains(opcode), "missing cast opcode {opcode:?} in IR:\n{ir}");
            assert!(ir.contains(store_ty), "missing destination type {store_ty:?} in IR:\n{ir}");
        }
    }

    /// Pointer casts cover pointer-pointer no-op shape, pointer-integer, and
    /// integer-pointer casts.
    #[cfg(feature = "llvm")]
    #[test]
    fn cast_pointer_opcode_table() {
        let mut tcx = TyCtxt::new();
        let int_ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let char_ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));

        let ptr_to_ptr = cast_return_body(char_ptr_ty, int_ptr_ty, CastKind::PtrToPtr);
        let ptr_ir =
            assert_codegen_fixture_verifies(&mut tcx, "__cast_ptr_to_ptr", char_ptr_ty, ptr_to_ptr);
        assert!(ptr_ir.contains("store ptr"), "IR:\n{ptr_ir}");

        let ulong_ty = tcx.ulong;
        let ptr_to_int = cast_return_body(ulong_ty, int_ptr_ty, CastKind::PtrToInt);
        let ptr_to_int_ir =
            assert_codegen_fixture_verifies(&mut tcx, "__cast_ptr_to_int", ulong_ty, ptr_to_int);
        assert!(ptr_to_int_ir.contains("ptrtoint ptr"), "IR:\n{ptr_to_int_ir}");
        assert!(ptr_to_int_ir.contains("to i64"), "IR:\n{ptr_to_int_ir}");

        let int_to_ptr = cast_return_body(int_ptr_ty, ulong_ty, CastKind::IntToPtr);
        let int_to_ptr_ir =
            assert_codegen_fixture_verifies(&mut tcx, "__cast_int_to_ptr", int_ptr_ty, int_to_ptr);
        assert!(int_to_ptr_ir.contains("inttoptr i64"), "IR:\n{int_to_ptr_ir}");
        assert!(int_to_ptr_ir.contains("to ptr"), "IR:\n{int_to_ptr_ir}");
    }

    // -----------------------------------------------------------------------
    // 09-16: Aggregate copy and memset intrinsics
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn aggregate_body_with_assign(void_ty: TyId, ty: TyId, rvalue: Rvalue) -> Body {
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, void_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(430)), ty, false));
        locals.push(cfg_local_decl(Some(Symbol(431)), ty, false));
        cfg_body_with_locals(
            void_ty,
            locals,
            vec![cfg_block(
                vec![Statement {
                    kind: StatementKind::Assign { place: local_place(Local(1)), rvalue },
                    span: DUMMY_SP,
                }],
                TerminatorKind::Return,
            )],
        )
    }

    #[cfg(feature = "llvm")]
    fn codegen_aggregate_fixture(
        session: &mut Session,
        tcx: &mut TyCtxt,
        defs: IndexVec<DefId, Def>,
        name: &str,
        body: Body,
    ) -> String {
        let fn_name = session.interner.intern(name);
        let mut defs = defs;
        let void_ty = tcx.void;
        let fn_ty = func_ty(tcx, void_ty, Vec::new(), false);
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let mut body = body;
        body.def = Some(def);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);
        codegen(session, tcx, &hir, &bodies).unwrap().ir_text
    }

    #[cfg(feature = "llvm")]
    fn codegen_fixture_ir_with_defs(
        session: &mut Session,
        tcx: &mut TyCtxt,
        defs: IndexVec<DefId, Def>,
        name: &str,
        ret_ty: TyId,
        body: Body,
    ) -> String {
        codegen_fixture_result_with_defs(session, tcx, defs, name, ret_ty, body).unwrap()
    }

    #[cfg(feature = "llvm")]
    fn codegen_fixture_result_with_defs(
        session: &mut Session,
        tcx: &mut TyCtxt,
        defs: IndexVec<DefId, Def>,
        name: &str,
        ret_ty: TyId,
        mut body: Body,
    ) -> Result<String, CodegenError> {
        let fn_name = session.interner.intern(name);
        let mut defs = defs;
        let fn_ty = func_ty(tcx, ret_ty, Vec::new(), false);
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        body.def = Some(def);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);
        codegen(session, tcx, &hir, &bodies).map(|artifact| artifact.ir_text)
    }

    /// Struct assignment lowers to an LLVM memcpy intrinsic instead of an
    /// aggregate load/store pair.
    #[cfg(feature = "llvm")]
    #[test]
    fn aggregate_struct_assignment_emits_memcpy() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let pair_def =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let pair_ty = tcx.intern(Ty::Record(pair_def));
        let body = aggregate_body_with_assign(tcx.void, pair_ty, Rvalue::Use(local_copy(Local(2))));

        let ir =
            codegen_aggregate_fixture(&mut session, &mut tcx, defs, "__aggregate_copy_pair", body);

        assert!(ir.contains("@llvm.memcpy.p0.p0.i64"), "IR:\n{ir}");
        assert!(ir.contains("i64 8"), "IR:\n{ir}");
    }

    /// Aggregate ZeroInit lowers to memset with the array byte size from
    /// LayoutCx.
    #[cfg(feature = "llvm")]
    #[test]
    fn aggregate_array_zero_init_emits_memset() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(5), is_vla: false });
        let body = aggregate_body_with_assign(
            tcx.void,
            arr_ty,
            Rvalue::Use(Operand::Const(Const { kind: ConstKind::ZeroInit, ty: arr_ty })),
        );

        let ir = codegen_aggregate_fixture(
            &mut session,
            &mut tcx,
            IndexVec::new(),
            "__aggregate_zero_array",
            body,
        );

        assert!(ir.contains("@llvm.memset.p0.i64"), "IR:\n{ir}");
        assert!(ir.contains("i8 0"), "IR:\n{ir}");
        assert!(ir.contains("i64 20"), "IR:\n{ir}");
    }

    /// Nested aggregate copies use the full outer object size, including
    /// padding, as reported by LayoutCx.
    #[cfg(feature = "llvm")]
    #[test]
    fn aggregate_nested_copy_uses_layout_size() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
        let rec_def =
            record_def(&mut defs, RecordKind::Struct, vec![field(arr_ty), field(tcx.long)]);
        let rec_ty = tcx.intern(Ty::Record(rec_def));
        let body = aggregate_body_with_assign(tcx.void, rec_ty, Rvalue::Use(local_copy(Local(2))));

        let ir = codegen_aggregate_fixture(
            &mut session,
            &mut tcx,
            defs,
            "__aggregate_copy_nested",
            body,
        );

        assert!(ir.contains("@llvm.memcpy.p0.p0.i64"), "IR:\n{ir}");
        assert!(ir.contains("i64 24"), "IR:\n{ir}");
    }

    /// Forward declarations are callable before any callee body is emitted.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_forward_decl_scalar_return_verifies() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let callee_ty = func_ty(&mut tcx, int_ty, vec![int_ty], false);
        let caller_ty = func_ty(&mut tcx, int_ty, Vec::new(), false);
        let mut defs = IndexVec::new();
        let callee = function_def(
            &mut defs,
            session.interner.intern("callee"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, int_ty, false));
        locals.push(cfg_local_decl(None, int_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: call_global(callee, callee_ty),
                args: vec![int_const(int_ty, 7)],
                destination: Some(local_place(Local(1))),
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(local_copy(Local(1))),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let mut body = cfg_body_with_locals(int_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(artifact.ir_text.contains("declare i32 @callee(i32)"), "IR:\n{}", artifact.ir_text);
        assert!(artifact.ir_text.contains("call i32 @callee(i32 7)"), "IR:\n{}", artifact.ir_text);
    }

    /// Function pointer operands lower to LLVM indirect calls with the declared ABI type.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_function_pointer_scalar_return_verifies() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let pointee_ty = func_ty(&mut tcx, int_ty, vec![int_ty], false);
        let fn_ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(pointee_ty)));
        let caller_ty = func_ty(&mut tcx, int_ty, vec![fn_ptr_ty], false);
        let mut defs = IndexVec::new();
        let caller = function_def(
            &mut defs,
            session.interner.intern("call_ptr"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, int_ty, false));
        locals.push(cfg_local_decl(Some(session.interner.intern("fp")), fn_ptr_ty, true));
        locals.push(cfg_local_decl(None, int_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: local_copy(Local(1)),
                args: vec![int_const(int_ty, 11)],
                destination: Some(local_place(Local(2))),
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(local_copy(Local(2))),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let mut body = cfg_body_with_locals(int_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(artifact.ir_text.contains("call i32 %"), "IR:\n{}", artifact.ir_text);
        assert!(artifact.ir_text.contains("(i32 11)"), "IR:\n{}", artifact.ir_text);
    }

    /// A call with no normal CFG target still leaves the LLVM block well-terminated.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_without_normal_target_emits_unreachable_path() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let void_ty = tcx.void;
        let callee_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let caller_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let mut defs = IndexVec::new();
        let callee = function_def(
            &mut defs,
            session.interner.intern("fatal"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller_no_edge"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let body = cfg_body(
            void_ty,
            vec![cfg_block(
                Vec::new(),
                TerminatorKind::Call {
                    callee: call_global(callee, callee_ty),
                    args: Vec::new(),
                    destination: None,
                    target: None,
                },
            )],
        );
        let mut body = body;
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(artifact.ir_text.contains("call void @fatal()"), "IR:\n{}", artifact.ir_text);
        assert!(artifact.ir_text.contains("unreachable"), "IR:\n{}", artifact.ir_text);
    }

    /// Large aggregate arguments are passed indirectly with byval call-site ABI attributes.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_large_aggregate_arg_uses_byval() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let long_ty = tcx.long;
        let void_ty = tcx.void;
        let mut defs = IndexVec::new();
        let big_def = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![field(long_ty), field(long_ty), field(long_ty), field(long_ty), field(long_ty)],
        );
        let big_ty = tcx.intern(Ty::Record(big_def));
        let callee_ty = func_ty(&mut tcx, void_ty, vec![big_ty], false);
        let caller_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let callee = function_def(
            &mut defs,
            session.interner.intern("take_big"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller_big_arg"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, void_ty, false));
        locals.push(cfg_local_decl(None, big_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: call_global(callee, callee_ty),
                args: vec![local_copy(Local(1))],
                destination: None,
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let mut body = cfg_body_with_locals(void_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(
            artifact.ir_text.contains("call void @take_big(ptr byval"),
            "IR:\n{}",
            artifact.ir_text
        );
    }

    /// Register-sized aggregate arguments are split into direct ABI units.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_small_aggregate_arg_splits_into_direct_units() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let long_ty = tcx.long;
        let void_ty = tcx.void;
        let mut defs = IndexVec::new();
        let duo_def =
            record_def(&mut defs, RecordKind::Struct, vec![field(long_ty), field(long_ty)]);
        let duo_ty = tcx.intern(Ty::Record(duo_def));
        let callee_ty = func_ty(&mut tcx, void_ty, vec![duo_ty], false);
        let caller_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let callee = function_def(
            &mut defs,
            session.interner.intern("take_duo"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller_duo_arg"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, void_ty, false));
        locals.push(cfg_local_decl(None, duo_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: call_global(callee, callee_ty),
                args: vec![local_copy(Local(1))],
                destination: None,
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let mut body = cfg_body_with_locals(void_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(
            artifact.ir_text.contains("declare void @take_duo(i64, i64)"),
            "IR:\n{}",
            artifact.ir_text
        );
        assert!(artifact.ir_text.contains("call void @take_duo(i64"), "IR:\n{}", artifact.ir_text);
    }

    /// Large aggregate returns use a hidden sret destination pointer.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_large_aggregate_return_uses_sret_destination() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let long_ty = tcx.long;
        let void_ty = tcx.void;
        let mut defs = IndexVec::new();
        let big_def = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![field(long_ty), field(long_ty), field(long_ty), field(long_ty), field(long_ty)],
        );
        let big_ty = tcx.intern(Ty::Record(big_def));
        let callee_ty = func_ty(&mut tcx, big_ty, Vec::new(), false);
        let caller_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let callee = function_def(
            &mut defs,
            session.interner.intern("make_big"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller_big_ret"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, void_ty, false));
        locals.push(cfg_local_decl(None, big_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: call_global(callee, callee_ty),
                args: Vec::new(),
                destination: Some(local_place(Local(1))),
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let mut body = cfg_body_with_locals(void_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(
            artifact.ir_text.contains("call void @make_big(ptr sret"),
            "IR:\n{}",
            artifact.ir_text
        );
    }

    /// Small aggregate returns are stored from their ABI register unit into the destination.
    #[cfg(feature = "llvm")]
    #[test]
    fn call_small_aggregate_return_stores_direct_unit() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let void_ty = tcx.void;
        let mut defs = IndexVec::new();
        let pair_def =
            record_def(&mut defs, RecordKind::Struct, vec![field(int_ty), field(int_ty)]);
        let pair_ty = tcx.intern(Ty::Record(pair_def));
        let callee_ty = func_ty(&mut tcx, pair_ty, Vec::new(), false);
        let caller_ty = func_ty(&mut tcx, void_ty, Vec::new(), false);
        let callee = function_def(
            &mut defs,
            session.interner.intern("make_pair"),
            callee_ty,
            FunctionDefOptions::default(),
        );
        let caller = function_def(
            &mut defs,
            session.interner.intern("caller_pair_ret"),
            caller_ty,
            FunctionDefOptions { has_body: true, ..FunctionDefOptions::default() },
        );
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, void_ty, false));
        locals.push(cfg_local_decl(None, pair_ty, false));
        let entry = cfg_block(
            Vec::new(),
            TerminatorKind::Call {
                callee: call_global(callee, callee_ty),
                args: Vec::new(),
                destination: Some(local_place(Local(1))),
                target: Some(BasicBlockId(1)),
            },
        );
        let join = cfg_block(Vec::new(), TerminatorKind::Return);
        let mut body = cfg_body_with_locals(void_ty, locals, vec![entry, join]);
        body.def = Some(caller);
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(caller, body);

        let artifact = codegen(&mut session, &tcx, &hir, &bodies).unwrap();

        assert!(artifact.ir_text.contains("call i64 @make_pair()"), "IR:\n{}", artifact.ir_text);
        assert!(artifact.ir_text.contains("store i64"), "IR:\n{}", artifact.ir_text);
    }

    // -----------------------------------------------------------------------
    // 09-09: Place address, operand load, and store helpers
    // -----------------------------------------------------------------------

    /// Create a minimal Body with int/ptr locals for place tests.
    #[cfg(feature = "llvm")]
    fn place_test_body(tcx: &mut TyCtxt) -> Body {
        use rcc_data_structures::IndexVec;
        use rcc_hir::ObjectQuals;

        let mut locals = IndexVec::new();
        // Local(0) = return slot (void)
        locals.push(LocalDecl {
            name: None,
            ty: tcx.void,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        // Local(1) = int param
        locals.push(LocalDecl {
            name: Some(Symbol(100)),
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: true,
            span: DUMMY_SP,
        });
        // Local(2) = int local
        locals.push(LocalDecl {
            name: Some(Symbol(101)),
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        // Local(3) = int* pointer
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        locals.push(LocalDecl {
            name: Some(Symbol(102)),
            ty: int_ptr,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });

        let mut blocks = IndexVec::new();
        blocks.push(rcc_cfg::BasicBlock::default());
        Body { def: None, locals, blocks, ret_ty: Some(tcx.void) }
    }

    /// Position a test function so helper methods can append instructions.
    #[cfg(feature = "llvm")]
    fn start_test_function<'ctx>(
        cx: &backend::CodegenCx<'_, 'ctx>,
        context: &'ctx inkwell::context::Context,
        name: &str,
    ) {
        let fn_ty = context.void_type().fn_type(&[], false);
        let function = cx.module().add_function(name, fn_ty, None);
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);
    }

    /// Create allocas matching `place_test_body`.
    #[cfg(feature = "llvm")]
    fn place_test_allocas<'ctx>(
        cx: &backend::CodegenCx<'_, 'ctx>,
        context: &'ctx inkwell::context::Context,
    ) -> IndexVec<Local, inkwell::values::PointerValue<'ctx>> {
        let mut allocas: IndexVec<Local, inkwell::values::PointerValue<'ctx>> = IndexVec::new();
        allocas.push(cx.builder().build_alloca(context.i8_type(), "ret").unwrap());
        allocas.push(cx.builder().build_alloca(context.i32_type(), "a").unwrap());
        allocas.push(cx.builder().build_alloca(context.i32_type(), "x").unwrap());
        let ptr_ty = context.ptr_type(inkwell::AddressSpace::default());
        allocas.push(cx.builder().build_alloca(ptr_ty, "p").unwrap());
        allocas
    }

    /// Add a void terminator to the current test function before verifier checks.
    #[cfg(feature = "llvm")]
    fn finish_void_test_function(cx: &backend::CodegenCx<'_, '_>) {
        cx.builder().build_return(None).unwrap();
    }

    /// emit_place_addr on a bare local returns its alloca.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_base_local() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_base");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(2), projection: vec![] };
        let addr = cx.emit_place_addr(&place, &allocas, &body).unwrap();
        assert_eq!(addr, allocas[Local(2)]);
    }

    /// emit_place_addr with Deref loads the pointer then returns that address.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_deref() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_deref");
        let allocas = place_test_allocas(&cx, &context);

        // Local(3) is int*. Deref means *p.
        let place = Place { base: Local(3), projection: vec![Projection::Deref] };
        let addr = cx.emit_place_addr(&place, &allocas, &body).unwrap();
        // Result differs from the alloca (it's the loaded pointer).
        assert_ne!(addr, allocas[Local(3)]);
        // Module should verify.
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    /// emit_place_addr with Field projection emits struct GEP.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_field() {
        use rcc_data_structures::IndexVec;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();

        // Build struct { i32, i32 }
        let mut defs = IndexVec::new();
        let rec = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let rec_ty = tcx.intern(Ty::Record(rec));
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);

        let fn_ty = context.void_type().fn_type(&[], false);
        let function = cx.module().add_function("__test_field", fn_ty, None);
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);

        let rec_llvm = cx.type_cx().basic_type_of(rec_ty).unwrap();
        let alloca = cx.builder().build_alloca(rec_llvm, "s").unwrap();

        let mut allocas: IndexVec<Local, inkwell::values::PointerValue<'_>> = IndexVec::new();
        allocas.push(cx.builder().build_alloca(context.i8_type(), "ret").unwrap());
        allocas.push(alloca);

        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: tcx.void,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: Some(Symbol(200)),
            ty: rec_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let mut blocks = IndexVec::new();
        blocks.push(rcc_cfg::BasicBlock::default());
        let body = Body { def: None, locals, blocks, ret_ty: Some(tcx.void) };

        let place = Place { base: Local(1), projection: vec![Projection::Field(1)] };
        let addr = cx.emit_place_addr(&place, &allocas, &body).unwrap();
        assert_ne!(addr, alloca);
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    /// emit_place_addr with Index projection emits GEP.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_index() {
        use rcc_data_structures::IndexVec;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();

        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(4), is_vla: false });
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);

        let fn_ty = context.void_type().fn_type(&[], false);
        let function = cx.module().add_function("__test_index", fn_ty, None);
        let entry = context.append_basic_block(function, "entry");
        cx.builder().position_at_end(entry);

        let arr_llvm = cx.type_cx().basic_type_of(arr_ty).unwrap();
        let alloca = cx.builder().build_alloca(arr_llvm, "a").unwrap();

        let mut allocas: IndexVec<Local, inkwell::values::PointerValue<'_>> = IndexVec::new();
        allocas.push(cx.builder().build_alloca(context.i8_type(), "ret").unwrap());
        allocas.push(alloca);

        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: tcx.void,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: Some(Symbol(300)),
            ty: arr_ty,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let mut blocks = IndexVec::new();
        blocks.push(rcc_cfg::BasicBlock::default());
        let body = Body { def: None, locals, blocks, ret_ty: Some(tcx.void) };

        let idx = Operand::Const(Const { kind: ConstKind::Int(2), ty: tcx.int });
        let place = Place { base: Local(1), projection: vec![Projection::Index(idx)] };
        let addr = cx.emit_place_addr(&place, &allocas, &body).unwrap();
        assert_ne!(addr, alloca);
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    /// emit_place_addr handles chained deref/field/index projections.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_nested_projection() {
        use rcc_data_structures::IndexVec;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();

        let mut defs = IndexVec::new();
        let inner = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let inner_ty = tcx.intern(Ty::Record(inner));
        let inner_array =
            tcx.intern(Ty::Array { elem: Qual::plain(inner_ty), len: Some(4), is_vla: false });
        let outer = record_def(&mut defs, RecordKind::Struct, vec![field(inner_array)]);
        let outer_ty = tcx.intern(Ty::Record(outer));
        let outer_ptr = tcx.intern(Ty::Ptr(Qual::plain(outer_ty)));

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_nested_projection");

        let mut allocas: IndexVec<Local, inkwell::values::PointerValue<'_>> = IndexVec::new();
        allocas.push(cx.builder().build_alloca(context.i8_type(), "ret").unwrap());
        allocas.push(
            cx.builder()
                .build_alloca(context.ptr_type(inkwell::AddressSpace::default()), "p")
                .unwrap(),
        );

        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: tcx.void,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: Some(Symbol(301)),
            ty: outer_ptr,
            quals: rcc_hir::ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let mut blocks = IndexVec::new();
        blocks.push(rcc_cfg::BasicBlock::default());
        let body = Body { def: None, locals, blocks, ret_ty: Some(tcx.void) };

        let idx = Operand::Const(Const { kind: ConstKind::Int(2), ty: tcx.int });
        let place = Place {
            base: Local(1),
            projection: vec![
                Projection::Deref,
                Projection::Field(0),
                Projection::Index(idx),
                Projection::Field(1),
            ],
        };
        let addr = cx.emit_place_addr(&place, &allocas, &body).unwrap();

        assert_ne!(addr, allocas[Local(1)]);
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    /// Rvalue::AddressOf emits the projected place address as a pointer value.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_rvalue_address_of() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_address_of");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(2), projection: vec![] };
        let value = cx.emit_rvalue_value(&Rvalue::AddressOf(place), &allocas, &body).unwrap();

        match value {
            inkwell::values::BasicValueEnum::PointerValue(ptr) => {
                assert_eq!(ptr, allocas[Local(2)]);
            }
            other => panic!("expected PointerValue, got {other:?}"),
        }
    }

    /// Invalid projections are reported as backend errors instead of panicking.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_place_addr_rejects_invalid_projection() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_invalid_projection");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(2), projection: vec![Projection::Deref] };

        assert!(matches!(
            cx.emit_place_addr(&place, &allocas, &body),
            Err(CodegenError::Internal(message)) if message.contains("invalid dereference projection")
        ));
    }

    /// emit_operand_value on IntConst returns the LLVM constant.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_operand_value_int_const() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_int_const");
        let allocas = place_test_allocas(&cx, &context);

        let operand = Operand::Const(Const { kind: ConstKind::Int(42), ty: tcx.int });
        let val = cx.emit_operand_value(&operand, &allocas, &body).unwrap();

        match val {
            inkwell::values::BasicValueEnum::IntValue(i) => {
                assert_eq!(i.get_type().get_bit_width(), 32);
            }
            other => panic!("expected IntValue, got {other:?}"),
        }
    }

    /// emit_operand_value on Copy(place) loads from the alloca.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_operand_value_copy() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_copy");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(1), projection: vec![] };
        let val = cx.emit_operand_value(&Operand::Copy(place), &allocas, &body).unwrap();

        match val {
            inkwell::values::BasicValueEnum::IntValue(i) => {
                assert_eq!(i.get_type().get_bit_width(), 32);
            }
            other => panic!("expected IntValue, got {other:?}"),
        }
    }

    /// emit_store_place writes a value and module verifies.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_store_place_basic() {
        use inkwell::values::BasicValue;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_store");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(2), projection: vec![] };
        let val = context.i32_type().const_int(99, false);
        cx.emit_store_place(&place, val.as_basic_value_enum(), &allocas, &body).unwrap();
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    /// store then load round-trips correctly.
    #[cfg(feature = "llvm")]
    #[test]
    fn store_then_load_roundtrip() {
        use inkwell::values::BasicValue;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let body = place_test_body(&mut tcx);
        let hir = HirCrate::default();
        let bodies = FxHashMap::default();
        let cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        start_test_function(&cx, &context, "__test_roundtrip");
        let allocas = place_test_allocas(&cx, &context);

        let place = Place { base: Local(2), projection: vec![] };
        let val = context.i32_type().const_int(77, false);
        cx.emit_store_place(&place, val.as_basic_value_enum(), &allocas, &body).unwrap();

        let loaded = cx.emit_operand_value(&Operand::Copy(place), &allocas, &body).unwrap();
        match loaded {
            inkwell::values::BasicValueEnum::IntValue(i) => {
                assert_eq!(i.get_type().get_bit_width(), 32);
            }
            other => panic!("expected IntValue, got {other:?}"),
        }
        finish_void_test_function(&cx);
        cx.verify_module().unwrap();
    }

    // -----------------------------------------------------------------------
    // 09-20: Volatile load / store
    // -----------------------------------------------------------------------

    /// Memory access to a `volatile` object uses `load volatile` in LLVM IR.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_object_load_emits_load_volatile() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: Some(Symbol(1)),
            ty: tcx.int,
            quals: ObjectQuals { is_volatile: true, ..ObjectQuals::none() },
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: Vec::new(),
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__volatile_obj_load", ret_ty, body);
        assert!(ir.contains("load volatile"), "expected load volatile in:\n{ir}");
    }

    /// Storing to a `volatile` object uses `store volatile` in LLVM IR.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_object_store_emits_store_volatile() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(LocalDecl {
            name: None,
            ty: tcx.int,
            quals: ObjectQuals { is_volatile: true, ..ObjectQuals::none() },
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(cfg_local_decl(None, tcx.int, true));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: Place { base: Local(1), projection: Vec::new() },
                    rvalue: Rvalue::Use(Operand::Copy(local_place(Local(2)))),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__volatile_obj_store", ret_ty, body);
        assert!(ir.contains("store volatile"), "expected store volatile in:\n{ir}");
    }

    /// Non-volatile loads are plain `load`, not `load volatile`.
    #[cfg(feature = "llvm")]
    #[test]
    fn non_volatile_object_load_is_not_load_volatile() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(cfg_local_decl(Some(Symbol(1)), tcx.int, false));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: Vec::new(),
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__non_volatile_load", ret_ty, body);
        assert!(!ir.contains("load volatile"), "unexpected load volatile in:\n{ir}");
    }

    /// Dereference of `volatile T *` issues a volatile load of the `T` value.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_pointee_deref_load_is_volatile() {
        let mut tcx = TyCtxt::new();
        let v_int_ptr = tcx.intern(Ty::Ptr(Qual {
            ty: tcx.int,
            is_const: false,
            is_volatile: true,
            is_restrict: false,
        }));
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: None,
            ty: v_int_ptr,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: vec![Projection::Deref],
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__volatile_deref", ret_ty, body);
        assert!(ir.contains("load volatile"), "expected load volatile in:\n{ir}");
    }

    /// A read of a `volatile` object in a value position is not dropped: IR shows `load volatile`.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_load_preserved_for_discarded_rvalue() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.void;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, tcx.void, false));
        locals.push(LocalDecl {
            name: None,
            ty: tcx.int,
            quals: ObjectQuals { is_volatile: true, ..ObjectQuals::none() },
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(cfg_local_decl(None, tcx.int, false));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: Place { base: Local(2), projection: Vec::new() },
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: Vec::new(),
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__volatile_discard", ret_ty, body);
        assert!(ir.contains("load volatile"), "expected load volatile in:\n{ir}");
    }

    /// `int * volatile p`: loading `p` uses `load volatile` for the pointer value.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_pointer_object_inner_load_is_load_volatile_ptr() {
        let mut tcx = TyCtxt::new();
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: None,
            ty: int_ptr,
            quals: ObjectQuals { is_volatile: true, ..ObjectQuals::none() },
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: vec![Projection::Deref],
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir =
            assert_codegen_fixture_verifies(&mut tcx, "__volatile_ptr_obj_deref", ret_ty, body);
        assert!(
            ir.contains("load volatile ptr") || ir.contains("load volatile i64"),
            "expected volatile pointer load:\n{ir}"
        );
        assert!(
            !ir.contains("load volatile i32"),
            "pointee load should not inherit pointer-slot volatility:\n{ir}"
        );
    }

    /// `volatile int *p`: loading `p` is non-volatile; only the `int` load is volatile.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_pointee_pointer_slot_load_is_plain_ptr() {
        let mut tcx = TyCtxt::new();
        let v_int_ptr = tcx.intern(Ty::Ptr(Qual {
            ty: tcx.int,
            is_const: false,
            is_volatile: true,
            is_restrict: false,
        }));
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: None,
            ty: v_int_ptr,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: vec![Projection::Deref],
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(
            &mut tcx,
            "__volatile_pointee_deref_split",
            ret_ty,
            body,
        );
        assert!(ir.contains("load volatile"), "expected final load volatile:\n{ir}");
        assert!(
            !ir.contains("load volatile ptr") && !ir.contains("load volatile i64"),
            "pointer slot load should not be marked volatile:\n{ir}"
        );
    }

    /// `int * volatile a[1]; *a[0]` — element qualifier makes the pointer-slot load volatile.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_pointer_array_element_load_is_volatile_ptr() {
        let mut tcx = TyCtxt::new();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let elem = Qual { ty: ptr_ty, is_const: false, is_volatile: true, is_restrict: false };
        let arr_ty = tcx.intern(Ty::Array { elem, len: Some(1), is_vla: false });
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            name: None,
            ty: ret_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        locals.push(LocalDecl {
            name: None,
            ty: arr_ty,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let idx_op = int_const(tcx.int, 0);
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: vec![Projection::Index(idx_op), Projection::Deref],
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);
        let ir = assert_codegen_fixture_verifies(&mut tcx, "__volatile_ptr_arr_elt", ret_ty, body);
        assert!(
            ir.contains("load volatile ptr") || ir.contains("load volatile i64"),
            "expected volatile pointer load from array element:\n{ir}"
        );
        assert!(
            !ir.contains("load volatile i32"),
            "pointee load should not inherit array element pointer-slot volatility:\n{ir}"
        );
    }

    // -----------------------------------------------------------------------
    // 09-21: Bitfield access codegen
    // -----------------------------------------------------------------------

    /// Reading a signed bitfield extracts the declared-width integer and
    /// sign-extends it to the field's declared integer type.
    #[cfg(feature = "llvm")]
    #[test]
    fn bitfield_signed_read_sign_extends_to_declared_type() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![bitfield(tcx.int, 3), bitfield(tcx.uint, 5)],
        );
        let rec_ty = tcx.intern(Ty::Record(rec));
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(520)), rec_ty, false));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(Place {
                        base: Local(1),
                        projection: vec![Projection::Field(0)],
                    })),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let ir = codegen_fixture_ir_with_defs(
            &mut session,
            &mut tcx,
            defs,
            "__bitfield_signed_read",
            ret_ty,
            body,
        );

        assert!(ir.contains("sext i3"), "signed 3-bit read should sign-extend:\n{ir}");
    }

    /// Writing one packed bitfield performs a read-modify-write on the
    /// containing storage unit so neighboring bitfields are preserved.
    #[cfg(feature = "llvm")]
    #[test]
    fn bitfield_write_masks_without_clobbering_neighbors() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![bitfield(tcx.uint, 3), bitfield(tcx.uint, 5)],
        );
        let rec_ty = tcx.intern(Ty::Record(rec));
        let ret_ty = tcx.void;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(521)), rec_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(522)), tcx.uint, true));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: Place { base: Local(1), projection: vec![Projection::Field(1)] },
                    rvalue: Rvalue::Use(Operand::Copy(local_place(Local(2)))),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let ir = codegen_fixture_ir_with_defs(
            &mut session,
            &mut tcx,
            defs,
            "__bitfield_masked_write",
            ret_ty,
            body,
        );

        assert!(ir.contains("load i32"), "bitfield write should read storage first:\n{ir}");
        assert!(ir.contains("and i32"), "bitfield write should mask storage/value:\n{ir}");
        assert!(ir.contains("shl i32"), "bitfield write should shift into place:\n{ir}");
        assert!(ir.contains("or i32"), "bitfield write should merge with neighbors:\n{ir}");
        assert!(ir.contains("store i32"), "bitfield write should store storage unit:\n{ir}");
    }

    /// Volatile bitfield reads and writes apply volatility to the containing
    /// storage-unit memory operation.
    #[cfg(feature = "llvm")]
    #[test]
    fn volatile_bitfield_accesses_use_volatile_storage_ops() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let mut vf = bitfield(tcx.uint, 3);
        vf.quals = ObjectQuals { is_volatile: true, ..ObjectQuals::none() };
        let rec = record_def(&mut defs, RecordKind::Struct, vec![vf]);
        let rec_ty = tcx.intern(Ty::Record(rec));
        let ret_ty = tcx.void;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(523)), rec_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(524)), tcx.uint, true));
        locals.push(cfg_local_decl(Some(Symbol(525)), tcx.uint, false));
        let field_place = Place { base: Local(1), projection: vec![Projection::Field(0)] };
        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: local_place(Local(3)),
                        rvalue: Rvalue::Use(Operand::Copy(field_place.clone())),
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: field_place,
                        rvalue: Rvalue::Use(Operand::Copy(local_place(Local(2)))),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let ir = codegen_fixture_ir_with_defs(
            &mut session,
            &mut tcx,
            defs,
            "__volatile_bitfield",
            ret_ty,
            body,
        );

        assert!(ir.contains("load volatile i32"), "volatile bitfield load expected:\n{ir}");
        assert!(ir.contains("store volatile i32"), "volatile bitfield store expected:\n{ir}");
    }

    /// C99 forbids taking the address of a bitfield; codegen rejects an
    /// address-producing rvalue before emitting an invalid pointer.
    #[cfg(feature = "llvm")]
    #[test]
    fn bitfield_address_of_is_rejected() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def(&mut defs, RecordKind::Struct, vec![bitfield(tcx.uint, 3)]);
        let rec_ty = tcx.intern(Ty::Record(rec));
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.uint)));
        let ret_ty = tcx.void;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(526)), rec_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(527)), ptr_ty, false));
        let block = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: local_place(Local(2)),
                    rvalue: Rvalue::AddressOf(Place {
                        base: Local(1),
                        projection: vec![Projection::Field(0)],
                    }),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let err = codegen_fixture_result_with_defs(
            &mut session,
            &mut tcx,
            defs,
            "__bitfield_addrof",
            ret_ty,
            body,
        )
        .unwrap_err();

        assert!(err.to_string().contains("address of a bit-field"), "unexpected error: {err}");
    }

    // -----------------------------------------------------------------------
    // 09-22: mem2reg and module verifier gate
    // -----------------------------------------------------------------------

    /// A simple scalar local should be promoted away by mem2reg after the
    /// non-SSA CFG has been emitted as allocas plus loads/stores.
    #[cfg(feature = "llvm")]
    #[test]
    fn mem2reg_promotes_clean_scalar_locals() {
        let mut tcx = TyCtxt::new();
        let ret_ty = tcx.int;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(540)), ret_ty, true));
        locals.push(cfg_local_decl(Some(Symbol(541)), ret_ty, false));
        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: local_place(Local(2)),
                        rvalue: Rvalue::BinaryOp(
                            BinOp::Add,
                            local_copy(Local(1)),
                            int_const(ret_ty, 1),
                        ),
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::Use(local_copy(Local(2))),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let ir = assert_codegen_fixture_mem2reg(&mut tcx, "__mem2reg_clean", ret_ty, body);

        assert_eq!(matching_line_count(&ir, "alloca i32"), 0, "IR:\n{ir}");
    }

    /// Taking a local's address prevents mem2reg from removing that local's
    /// storage, even though other scalar temporaries can still be promoted.
    #[cfg(feature = "llvm")]
    #[test]
    fn mem2reg_keeps_address_taken_alloca() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let int_ptr = tcx.intern(Ty::Ptr(Qual::plain(int_ty)));
        let ret_ty = int_ptr;
        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, ret_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(542)), int_ty, false));
        locals.push(cfg_local_decl(Some(Symbol(543)), int_ptr, false));
        let block = cfg_block(
            vec![
                Statement {
                    kind: StatementKind::Assign {
                        place: local_place(Local(1)),
                        rvalue: Rvalue::Use(int_const(int_ty, 7)),
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: local_place(Local(2)),
                        rvalue: Rvalue::AddressOf(local_place(Local(1))),
                    },
                    span: DUMMY_SP,
                },
                Statement {
                    kind: StatementKind::Assign {
                        place: ret_slot(),
                        rvalue: Rvalue::Use(local_copy(Local(2))),
                    },
                    span: DUMMY_SP,
                },
            ],
            TerminatorKind::Return,
        );
        let body = cfg_body_with_locals(ret_ty, locals, vec![block]);

        let ir = assert_codegen_fixture_mem2reg(&mut tcx, "__mem2reg_address_taken", ret_ty, body);

        assert_eq!(matching_line_count(&ir, "alloca i32"), 1, "IR:\n{ir}");
        assert!(ir.contains("%local542.addr = alloca i32"), "IR:\n{ir}");
    }

    /// emit_operand_value on ConstKind::Global returns a pointer.
    #[cfg(feature = "llvm")]
    #[test]
    fn emit_operand_value_global_const() {
        use rcc_data_structures::IndexVec;
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();

        // Intern the pointer type before borrowing tcx immutably.
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));

        // Declare a global so emit_const can find it.
        let mut defs = IndexVec::new();
        let global_name = session.interner.intern("global_for_const");
        let g = global_def(&mut defs, global_name, tcx.int, Linkage::External);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_global(g).unwrap();

        let operand = Operand::Const(Const { kind: ConstKind::Global(g), ty: ptr_ty });
        let val = cx.emit_operand_value(&operand, &IndexVec::new(), &Body::default()).unwrap();

        match val {
            inkwell::values::BasicValueEnum::PointerValue(_) => {}
            other => panic!("expected PointerValue, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 09-11: Global initializer materialization
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    fn global_def_with_init(
        defs: &mut IndexVec<DefId, Def>,
        name: Symbol,
        ty: TyId,
        linkage: Linkage,
        init: GlobalInit,
    ) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name,
            span: DUMMY_SP,
            kind: DefKind::Global { ty, quals: ObjectQuals::none(), linkage, init: Some(init) },
        });
        defs[id].id = id;
        id
    }

    /// `static int x = 5;` emits `@x = internal global i32 5`.
    #[cfg(feature = "llvm")]
    #[test]
    fn scalar_global_emits_initializer() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let name = session.interner.intern("x");
        let init = GlobalInit {
            ty: tcx.int,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: tcx.int,
                expr: None,
                value: GlobalInitValue::Int(5),
                span: DUMMY_SP,
            }],
        };
        let _g = global_def_with_init(&mut defs, name, tcx.int, Linkage::Internal, init);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("@x = internal global i32 5"), "IR:\n{ir}");
    }

    /// Missing initializer entries produce zero-fill.
    #[cfg(feature = "llvm")]
    #[test]
    fn zero_fill_for_missing_entries() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let name = session.interner.intern("arr");
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(3), is_vla: false });
        let init = GlobalInit {
            ty: arr_ty,
            entries: vec![GlobalInitEntry {
                path: vec![GlobalInitDesignator::Index(1)],
                ty: tcx.int,
                expr: None,
                value: GlobalInitValue::Int(42),
                span: DUMMY_SP,
            }],
        };
        let _g = global_def_with_init(&mut defs, name, arr_ty, Linkage::Internal, init);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        // [3 x i32] [i32 0, i32 42, i32 0]
        assert!(ir.contains("[i32 0, i32 42, i32 0]"), "IR:\n{ir}");
    }

    /// Struct fields are initialized from designator paths.
    #[cfg(feature = "llvm")]
    #[test]
    fn struct_global_emits_field_initializers() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def(&mut defs, RecordKind::Struct, vec![field(tcx.int), field(tcx.int)]);
        let rec_ty = tcx.intern(Ty::Record(rec));
        let name = session.interner.intern("s");
        let init = GlobalInit {
            ty: rec_ty,
            entries: vec![
                GlobalInitEntry {
                    path: vec![GlobalInitDesignator::Field(0)],
                    ty: tcx.int,
                    expr: None,
                    value: GlobalInitValue::Int(1),
                    span: DUMMY_SP,
                },
                GlobalInitEntry {
                    path: vec![GlobalInitDesignator::Field(1)],
                    ty: tcx.int,
                    expr: None,
                    value: GlobalInitValue::Int(2),
                    span: DUMMY_SP,
                },
            ],
        };
        let _g = global_def_with_init(&mut defs, name, rec_ty, Linkage::Internal, init);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        // { i32, i32 } { i32 1, i32 2 }
        assert!(ir.contains("{ i32 1, i32 2 }"), "IR:\n{ir}");
    }

    /// GlobalInitValue::Error is rejected before emitting invalid IR.
    #[cfg(feature = "llvm")]
    #[test]
    fn error_leaf_is_rejected() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let name = session.interner.intern("bad");
        let init = GlobalInit {
            ty: tcx.int,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: tcx.int,
                expr: None,
                value: GlobalInitValue::Error,
                span: DUMMY_SP,
            }],
        };
        let _g = global_def_with_init(&mut defs, name, tcx.int, Linkage::Internal, init);
        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        let result = backend::GlobalCx::new(&cx).materialize_all_globals();

        assert!(result.is_err(), "expected error for GlobalInitValue::Error");
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn address_offset_global_init() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let arr_name = session.interner.intern("arr");
        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(10), is_vla: false });
        let arr_def = global_def_with_init(
            &mut defs,
            arr_name,
            arr_ty,
            Linkage::Internal,
            GlobalInit { ty: arr_ty, entries: vec![] },
        );

        let ptr_name = session.interner.intern("p");
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let init = GlobalInit {
            ty: ptr_ty,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: ptr_ty,
                expr: None,
                value: GlobalInitValue::Address { def: Some(arr_def), offset: 8 },
                span: DUMMY_SP,
            }],
        };
        let _ptr_def = global_def_with_init(&mut defs, ptr_name, ptr_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(
            ir.contains("@p = internal global ptr getelementptr inbounds (i8, ptr @arr, i64 8)")
        );
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn union_scalar_initializer() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let union_def_id =
            record_def(&mut defs, RecordKind::Union, vec![field(tcx.int), field(char_arr_ty)]);
        let union_ty = tcx.intern(Ty::Record(union_def_id));

        let u_name = session.interner.intern("u");
        let init = GlobalInit {
            ty: union_ty,
            entries: vec![GlobalInitEntry {
                path: vec![rcc_hir::GlobalInitDesignator::Field(0)],
                ty: tcx.int,
                expr: None,
                value: GlobalInitValue::Int(5),
                span: DUMMY_SP,
            }],
        };
        let _u = global_def_with_init(&mut defs, u_name, union_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("c\"\\05\\00\\00\\00\"") || ir.contains("[i8 5, i8 0, i8 0, i8 0]"));
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn union_array_initializer_preserves_member_bytes() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let union_def_id =
            record_def(&mut defs, RecordKind::Union, vec![field(tcx.int), field(char_arr_ty)]);
        let union_ty = tcx.intern(Ty::Record(union_def_id));

        let u_name = session.interner.intern("u_arr");
        let init = GlobalInit {
            ty: union_ty,
            entries: vec![
                GlobalInitEntry {
                    path: vec![
                        rcc_hir::GlobalInitDesignator::Field(1),
                        rcc_hir::GlobalInitDesignator::Index(0),
                    ],
                    ty: tcx.char_,
                    expr: None,
                    value: GlobalInitValue::Int(i128::from(b'A')),
                    span: DUMMY_SP,
                },
                GlobalInitEntry {
                    path: vec![
                        rcc_hir::GlobalInitDesignator::Field(1),
                        rcc_hir::GlobalInitDesignator::Index(1),
                    ],
                    ty: tcx.char_,
                    expr: None,
                    value: GlobalInitValue::Int(i128::from(b'B')),
                    span: DUMMY_SP,
                },
                GlobalInitEntry {
                    path: vec![
                        rcc_hir::GlobalInitDesignator::Field(1),
                        rcc_hir::GlobalInitDesignator::Index(2),
                    ],
                    ty: tcx.char_,
                    expr: None,
                    value: GlobalInitValue::Int(i128::from(b'C')),
                    span: DUMMY_SP,
                },
            ],
        };
        let _u = global_def_with_init(&mut defs, u_name, union_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(
            ir.contains("c\"ABC\\00\"") || ir.contains("[i8 65, i8 66, i8 67, i8 0]"),
            "IR:\n{ir}"
        );
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn union_struct_initializer_preserves_member_bytes_with_padding() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let struct_def_id =
            record_def(&mut defs, RecordKind::Struct, vec![field(tcx.char_), field(tcx.int)]);
        let struct_ty = tcx.intern(Ty::Record(struct_def_id));
        let union_def_id =
            record_def(&mut defs, RecordKind::Union, vec![field(tcx.int), field(struct_ty)]);
        let union_ty = tcx.intern(Ty::Record(union_def_id));

        let u_name = session.interner.intern("u_struct");
        let init = GlobalInit {
            ty: union_ty,
            entries: vec![
                GlobalInitEntry {
                    path: vec![
                        rcc_hir::GlobalInitDesignator::Field(1),
                        rcc_hir::GlobalInitDesignator::Field(0),
                    ],
                    ty: tcx.char_,
                    expr: None,
                    value: GlobalInitValue::Int(1),
                    span: DUMMY_SP,
                },
                GlobalInitEntry {
                    path: vec![
                        rcc_hir::GlobalInitDesignator::Field(1),
                        rcc_hir::GlobalInitDesignator::Field(1),
                    ],
                    ty: tcx.int,
                    expr: None,
                    value: GlobalInitValue::Int(0x0203_0405),
                    span: DUMMY_SP,
                },
            ],
        };
        let _u = global_def_with_init(&mut defs, u_name, union_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(
            ir.contains("c\"\\01\\00\\00\\00\\05\\04\\03\\02\"")
                || ir.contains("[i8 1, i8 0, i8 0, i8 0, i8 5, i8 4, i8 3, i8 2]"),
            "IR:\n{ir}"
        );
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn nested_designator_initializer() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(2), is_vla: false });
        let struct_def_id = record_def(&mut defs, RecordKind::Struct, vec![field(arr_ty)]);
        let struct_ty = tcx.intern(Ty::Record(struct_def_id));

        let s_name = session.interner.intern("s");
        let init = GlobalInit {
            ty: struct_ty,
            entries: vec![GlobalInitEntry {
                path: vec![
                    rcc_hir::GlobalInitDesignator::Field(0),
                    rcc_hir::GlobalInitDesignator::Index(1),
                ],
                ty: tcx.int,
                expr: None,
                value: GlobalInitValue::Int(7),
                span: DUMMY_SP,
            }],
        };
        let _s = global_def_with_init(&mut defs, s_name, struct_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("i32 7"));
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn float_and_zero_leaf_initializer() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let s_name = session.interner.intern("s");
        let s_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.double), len: Some(2), is_vla: false });
        let init = GlobalInit {
            ty: s_ty,
            entries: vec![
                GlobalInitEntry {
                    path: vec![rcc_hir::GlobalInitDesignator::Index(0)],
                    ty: tcx.double,
                    expr: None,
                    value: GlobalInitValue::Float(1.25),
                    span: DUMMY_SP,
                },
                GlobalInitEntry {
                    path: vec![rcc_hir::GlobalInitDesignator::Index(1)],
                    ty: tcx.double,
                    expr: None,
                    value: GlobalInitValue::Zero,
                    span: DUMMY_SP,
                },
            ],
        };
        let _s = global_def_with_init(&mut defs, s_name, s_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("double"));
        assert!(ir.contains("1.25"));
    }
    /// Helper: build a `GlobalInit` for a string literal from raw bytes
    /// (mimicking what `rcc_hir_lower::intern_string_literal` now does).
    #[cfg(feature = "llvm")]
    fn string_literal_init(raw_bytes: &[u8], char_arr_ty: TyId, char_ty: TyId) -> GlobalInit {
        let mut entries = Vec::with_capacity(raw_bytes.len() + 1);
        for (i, &b) in raw_bytes.iter().enumerate() {
            entries.push(GlobalInitEntry {
                path: vec![rcc_hir::GlobalInitDesignator::Index(i as u64)],
                ty: char_ty,
                expr: None,
                value: GlobalInitValue::Int(i128::from(b)),
                span: DUMMY_SP,
            });
        }
        entries.push(GlobalInitEntry {
            path: vec![rcc_hir::GlobalInitDesignator::Index(raw_bytes.len() as u64)],
            ty: char_ty,
            expr: None,
            value: GlobalInitValue::Int(0),
            span: DUMMY_SP,
        });
        GlobalInit { ty: char_arr_ty, entries }
    }

    /// String literals referenced by GlobalInitValue::StringLiteral get a
    /// constant byte-array initializer.
    #[cfg(feature = "llvm")]
    #[test]
    fn string_literal_global_is_materialized() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let text_sym = session.interner.intern("\"hi\"");
        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(3), is_vla: false });
        let str_init = string_literal_init(b"hi", char_arr_ty, tcx.char_);
        let str_def = defs.push(Def {
            id: DefId(0),
            name: text_sym,
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: char_arr_ty,
                quals: ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: Some(str_init),
            },
        });
        defs[str_def].id = str_def;

        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let ptr_name = session.interner.intern("ptr_to_str");
        let init = GlobalInit {
            ty: ptr_ty,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: ptr_ty,
                expr: None,
                value: GlobalInitValue::StringLiteral(str_def),
                span: DUMMY_SP,
            }],
        };
        let _ptr_def = global_def_with_init(&mut defs, ptr_name, ptr_ty, Linkage::Internal, init);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains("c\"hi\\00\""), "IR:\n{ir}");
    }

    /// Identical string literal DefIds share the same LLVM global.
    #[cfg(feature = "llvm")]
    #[test]
    fn identical_string_literals_share_global() {
        let context = inkwell::context::Context::create();
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();

        let text_sym = session.interner.intern("\"shared\"");
        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(7), is_vla: false });
        let str_init = string_literal_init(b"shared", char_arr_ty, tcx.char_);
        let str_def = defs.push(Def {
            id: DefId(0),
            name: text_sym,
            span: DUMMY_SP,
            kind: DefKind::Global {
                ty: char_arr_ty,
                quals: ObjectQuals::none(),
                linkage: Linkage::Internal,
                init: Some(str_init),
            },
        });
        defs[str_def].id = str_def;

        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.char_)));
        let ptr1_name = session.interner.intern("p1");
        let ptr2_name = session.interner.intern("p2");

        let init1 = GlobalInit {
            ty: ptr_ty,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: ptr_ty,
                expr: None,
                value: GlobalInitValue::StringLiteral(str_def),
                span: DUMMY_SP,
            }],
        };
        let _p1 = global_def_with_init(&mut defs, ptr1_name, ptr_ty, Linkage::Internal, init1);

        let init2 = GlobalInit {
            ty: ptr_ty,
            entries: vec![GlobalInitEntry {
                path: vec![],
                ty: ptr_ty,
                expr: None,
                value: GlobalInitValue::StringLiteral(str_def),
                span: DUMMY_SP,
            }],
        };
        let _p2 = global_def_with_init(&mut defs, ptr2_name, ptr_ty, Linkage::Internal, init2);

        let hir = hir_with_defs(defs);
        let bodies = FxHashMap::default();
        let mut cx = backend::CodegenCx::new(&context, &mut session, &tcx, &hir, &bodies);
        cx.declare_all().unwrap();
        backend::GlobalCx::new(&cx).materialize_all_globals().unwrap();

        let ir = cx.ir_text();
        assert!(ir.contains(r#"@p1 = internal global ptr @"\22shared\22""#), "IR:\n{ir}");
        assert!(ir.contains(r#"@p2 = internal global ptr @"\22shared\22""#), "IR:\n{ir}");
    }

    // -----------------------------------------------------------------------
    // 09-19: Variadic function and va builtin tests
    // -----------------------------------------------------------------------

    #[cfg(feature = "llvm")]
    #[test]
    fn variadic_va_builtins_fixture_verifies() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        let va_list_ty = tcx.builtin_va_list;

        let fn_ty = func_ty(&mut tcx, int_ty, vec![int_ty], true);
        let fn_name = session.interner.intern("__test_va_sum");
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, variadic: true, ..FunctionDefOptions::default() },
        );

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, int_ty, false)); // 0: ret
        locals.push(cfg_local_decl(None, int_ty, true)); // 1: last param
        locals.push(cfg_local_decl(None, va_list_ty, false)); // 2: ap
        locals.push(cfg_local_decl(None, int_ty, false)); // 3: tmp

        let ap_place = local_place(Local(2));
        let last_place = local_place(Local(1));
        let tmp_place = local_place(Local(3));

        let block0 = cfg_block(
            Vec::new(),
            TerminatorKind::BuiltinVaStart {
                ap: Operand::Copy(ap_place.clone()),
                last_param: Operand::Copy(last_place),
                target: BasicBlockId(1),
            },
        );
        let block1 = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: tmp_place.clone(),
                    rvalue: Rvalue::BuiltinVaArg {
                        ap: Operand::Copy(ap_place.clone()),
                        ty: int_ty,
                    },
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Goto(BasicBlockId(2)),
        );
        let block2 = cfg_block(
            Vec::new(),
            TerminatorKind::BuiltinVaEnd { ap: Operand::Copy(ap_place), target: BasicBlockId(3) },
        );
        let block3 = cfg_block(
            vec![Statement {
                kind: StatementKind::Assign {
                    place: ret_slot(),
                    rvalue: Rvalue::Use(Operand::Copy(tmp_place)),
                },
                span: DUMMY_SP,
            }],
            TerminatorKind::Return,
        );

        let mut cfg_blocks = IndexVec::new();
        cfg_blocks.push(block0);
        cfg_blocks.push(block1);
        cfg_blocks.push(block2);
        cfg_blocks.push(block3);

        let body = Body { def: Some(def), locals, blocks: cfg_blocks, ret_ty: Some(int_ty) };
        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);

        let ir = codegen(&mut session, &tcx, &hir, &bodies).unwrap().ir_text;

        assert!(ir.contains("va_start"), "IR missing va_start:\n{ir}");
        assert!(ir.contains("va_arg"), "IR missing va_arg:\n{ir}");
        assert!(ir.contains("va_end"), "IR missing va_end:\n{ir}");
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn variadic_is_vararg_in_ir() {
        let (mut session, _cap) = Session::for_test();
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;

        let fn_ty = func_ty(&mut tcx, int_ty, vec![int_ty], true);
        let fn_name = session.interner.intern("__test_variadic_decl");
        let mut defs = IndexVec::new();
        let def = function_def(
            &mut defs,
            fn_name,
            fn_ty,
            FunctionDefOptions { has_body: true, variadic: true, ..FunctionDefOptions::default() },
        );

        let mut locals = IndexVec::new();
        locals.push(cfg_local_decl(None, int_ty, false));
        locals.push(cfg_local_decl(None, int_ty, true));

        let body = Body {
            def: Some(def),
            locals,
            blocks: {
                let mut b = IndexVec::new();
                b.push(cfg_block(vec![assign_ret(int_ty, 0)], TerminatorKind::Return));
                b
            },
            ret_ty: Some(int_ty),
        };

        let hir = hir_with_defs(defs);
        let mut bodies = FxHashMap::default();
        bodies.insert(def, body);

        let ir = codegen(&mut session, &tcx, &hir, &bodies).unwrap().ir_text;

        assert!(
            ir.contains("define i32 @__test_variadic_decl(i32 %0, ...)"),
            "IR missing variadic signature:\n{ir}"
        );
    }
}
