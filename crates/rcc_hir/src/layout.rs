//! Target-neutral type layout service for the baseline LP64 target.
//!
//! This module deliberately lives in `rcc_hir`, not `rcc_codegen_llvm`,
//! because CFG lowering needs `sizeof` answers before LLVM codegen runs.

use rcc_data_structures::IndexVec;

use crate::{Def, DefId, DefKind, FloatKind, IntRank, Layout, RecordKind, Ty, TyCtxt, TyId};

/// Result type used by layout queries.
pub type LayoutResult<T> = Result<T, LayoutError>;

/// Complete layout details for a record type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordLayout {
    /// Aggregate size and alignment.
    pub layout: Layout,
    /// Per-field layout metadata in source declaration order.
    pub fields: Vec<FieldLayout>,
}

/// Layout metadata for one record field.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    /// Byte offset from the start of the enclosing record.
    pub offset: u64,
    /// Bit offset within the storage unit for bit-fields.
    pub bit_offset: Option<u32>,
    /// Declared bit-field width, if this field is a bit-field.
    pub bit_width: Option<u32>,
    /// Storage occupied by this field in bytes.
    ///
    /// Flexible array members use zero here because they contribute no
    /// trailing element size to `sizeof(struct S)`.
    pub storage_size: u64,
    /// ABI alignment of the field storage in bytes.
    pub storage_align: u32,
}

/// Layout metadata for an array type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ArrayLayout {
    /// Element layout.
    pub elem: Layout,
    /// Array object alignment, inherited from the element type.
    pub align: u32,
    /// Constant element count for fixed arrays.
    pub len: Option<u64>,
    /// Static byte size for fixed arrays.
    ///
    /// This is `None` for VLA sentinels because their allocation size is
    /// computed at runtime, even though their element alignment is known.
    pub static_size: Option<u64>,
    /// Whether the array is a VLA.
    pub is_vla: bool,
}

/// Error returned when a type has no compile-time object layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The query needs the top-level definition table, but the caller
    /// constructed the context with [`LayoutCx::new`].
    MissingDefinitions { ty: TyId },
    /// A `DefId` referenced by a type is not present in the definition table.
    MissingDefinition { def: DefId },
    /// A `Ty::Record` did not point at a record definition.
    ExpectedRecord { def: DefId },
    /// The queried type is not a complete object type.
    Unsized { ty: TyId, reason: &'static str },
    /// Layout multiplication or padding overflowed `u64`.
    SizeOverflow { ty: TyId },
    /// The compiler knows the type kind, but this layout rule is deferred.
    Unsupported { ty: TyId, feature: &'static str },
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::MissingDefinitions { ty } => {
                write!(f, "layout of {ty:?} requires the HIR definition table")
            }
            LayoutError::MissingDefinition { def } => {
                write!(f, "layout references missing definition {def:?}")
            }
            LayoutError::ExpectedRecord { def } => {
                write!(f, "definition {def:?} is not a record")
            }
            LayoutError::Unsized { ty, reason } => {
                write!(f, "type {ty:?} has no compile-time layout: {reason}")
            }
            LayoutError::SizeOverflow { ty } => {
                write!(f, "layout computation for {ty:?} overflowed")
            }
            LayoutError::Unsupported { ty, feature } => {
                write!(f, "layout of {ty:?} needs unsupported feature: {feature}")
            }
        }
    }
}

impl std::error::Error for LayoutError {}

/// Layout context for the compiler's current baseline target.
///
/// The scalar ABI is LP64 / SysV x86-64 compatible for now. Aggregate
/// layout is independent of LLVM and can therefore be shared by CFG
/// lowering, constant evaluation, and the LLVM backend.
pub struct LayoutCx<'tcx> {
    /// Backing type context.
    pub tcx: &'tcx TyCtxt,
    defs: Option<&'tcx IndexVec<DefId, Def>>,
}

impl<'tcx> LayoutCx<'tcx> {
    /// Build a layout context without access to top-level definitions.
    ///
    /// This is sufficient for scalar, pointer, enum-as-int, and array
    /// layouts that do not contain records.
    #[must_use]
    pub fn new(tcx: &'tcx TyCtxt) -> Self {
        Self { tcx, defs: None }
    }

    /// Build a layout context that can resolve record and enum definitions.
    #[must_use]
    pub fn with_defs(tcx: &'tcx TyCtxt, defs: &'tcx IndexVec<DefId, Def>) -> Self {
        Self { tcx, defs: Some(defs) }
    }

    /// Compute the layout of `ty`.
    ///
    /// Returns an error for `void`, functions, incomplete arrays, VLA
    /// array objects, `Ty::Error`, and records when no definition table
    /// was supplied.
    pub fn layout_of(&self, ty: TyId) -> LayoutResult<Layout> {
        self.layout_of_inner(ty, &mut Vec::new())
    }

    /// Compute complete field layout details for a struct or union type.
    pub fn record_layout_of(&self, ty: TyId) -> LayoutResult<RecordLayout> {
        let Ty::Record(def) = self.tcx.get(ty) else {
            return Err(LayoutError::Unsized { ty, reason: "not a record type" });
        };
        self.record_layout_details(ty, *def, &mut Vec::new())
    }

    /// Compute array layout details, including the VLA alignment sentinel.
    pub fn array_layout_of(&self, ty: TyId) -> LayoutResult<ArrayLayout> {
        self.array_layout_details(ty, &mut Vec::new())
    }

    fn layout_of_inner(&self, ty: TyId, record_stack: &mut Vec<DefId>) -> LayoutResult<Layout> {
        match self.tcx.get(ty) {
            Ty::Void => Err(LayoutError::Unsized { ty, reason: "void is not an object type" }),
            Ty::Int { rank, .. } => Ok(int_layout(*rank)),
            Ty::Float(kind) => Ok(float_layout(*kind)),
            Ty::Complex(kind) => {
                let base = float_layout(*kind);
                Ok(Layout {
                    size: base.size.checked_mul(2).ok_or(LayoutError::SizeOverflow { ty })?,
                    align: base.align,
                })
            }
            Ty::Ptr(_) => Ok(Layout { size: 8, align: 8 }),
            Ty::Func { .. } => {
                Err(LayoutError::Unsized { ty, reason: "function types have no object size" })
            }
            Ty::Array { len: Some(_), is_vla: false, .. } => {
                let array = self.array_layout_details(ty, record_stack)?;
                Ok(Layout {
                    size: array.static_size.expect("fixed array has static size"),
                    align: array.align,
                })
            }
            Ty::Array { is_vla: true, .. } => {
                Err(LayoutError::Unsized { ty, reason: "VLA size is runtime-dependent" })
            }
            Ty::Array { len: None, .. } => {
                Err(LayoutError::Unsized { ty, reason: "incomplete array has no object size" })
            }
            Ty::Record(def) => self.record_layout_details(ty, *def, record_stack).map(|r| r.layout),
            Ty::Enum(def) => self.enum_layout(*def),
            Ty::BuiltinVaList => Ok(Layout { size: 24, align: 8 }),
            Ty::Error => Err(LayoutError::Unsized { ty, reason: "error type has no layout" }),
        }
    }

    fn array_layout_details(
        &self,
        ty: TyId,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<ArrayLayout> {
        let Ty::Array { elem, len, is_vla } = self.tcx.get(ty) else {
            return Err(LayoutError::Unsized { ty, reason: "not an array type" });
        };
        let elem_layout = self.layout_of_inner(elem.ty, record_stack)?;
        if *is_vla {
            return Ok(ArrayLayout {
                elem: elem_layout,
                align: elem_layout.align,
                len: *len,
                static_size: None,
                is_vla: true,
            });
        }
        let Some(len) = len else {
            return Err(LayoutError::Unsized { ty, reason: "incomplete array has no object size" });
        };
        let size = elem_layout.size.checked_mul(*len).ok_or(LayoutError::SizeOverflow { ty })?;
        Ok(ArrayLayout {
            elem: elem_layout,
            align: elem_layout.align,
            len: Some(*len),
            static_size: Some(size),
            is_vla: false,
        })
    }

    fn record_layout_details(
        &self,
        ty: TyId,
        def: DefId,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let defs = self.defs.ok_or(LayoutError::MissingDefinitions { ty })?;
        if record_stack.contains(&def) {
            return Err(LayoutError::Unsupported { ty, feature: "recursive record by value" });
        }
        let def_data = defs.get(def).ok_or(LayoutError::MissingDefinition { def })?;
        let DefKind::Record { kind, layout, fields } = &def_data.kind else {
            return Err(LayoutError::ExpectedRecord { def });
        };
        if let Some(layout) = layout {
            return Ok(RecordLayout {
                layout: *layout,
                fields: fields
                    .iter()
                    .map(|field| FieldLayout {
                        offset: field.offset.unwrap_or(0),
                        bit_offset: None,
                        bit_width: field.bit_width,
                        storage_size: 0,
                        storage_align: 1,
                    })
                    .collect(),
            });
        }
        if fields.is_empty() {
            return Err(LayoutError::Unsized {
                ty,
                reason: "record has no fields or completed layout",
            });
        }

        record_stack.push(def);
        let result = match kind {
            RecordKind::Struct => self.struct_layout_details(ty, fields, record_stack),
            RecordKind::Union => self.union_layout_details(ty, fields, record_stack),
        };
        record_stack.pop();
        result
    }

    fn struct_layout_details(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let mut offset = 0_u64;
        let mut max_align = 1_u32;
        let mut layouts = Vec::with_capacity(fields.len());
        let mut bit_unit: Option<BitUnit> = None;
        for (idx, field) in fields.iter().enumerate() {
            if let Some(width) = field.bit_width {
                let storage = self.layout_of_inner(field.ty, record_stack)?;
                let storage_bits = storage_size_bits(storage, ty)?;
                max_align = max_align.max(storage.align);

                if width == 0 {
                    offset = finish_bit_unit(offset, bit_unit.take(), ty)?;
                    offset =
                        align_to(offset, storage.align).ok_or(LayoutError::SizeOverflow { ty })?;
                    layouts.push(FieldLayout {
                        offset,
                        bit_offset: Some(0),
                        bit_width: Some(width),
                        storage_size: 0,
                        storage_align: storage.align,
                    });
                    continue;
                }

                let needs_new_unit = bit_unit
                    .map(|unit| {
                        unit.storage_size != storage.size
                            || unit.storage_align != storage.align
                            || u64::from(unit.used_bits) + u64::from(width)
                                > u64::from(unit.storage_bits)
                    })
                    .unwrap_or(true);
                if needs_new_unit {
                    offset = finish_bit_unit(offset, bit_unit.take(), ty)?;
                    offset =
                        align_to(offset, storage.align).ok_or(LayoutError::SizeOverflow { ty })?;
                    bit_unit = Some(BitUnit {
                        offset,
                        storage_size: storage.size,
                        storage_align: storage.align,
                        storage_bits,
                        used_bits: 0,
                    });
                }

                let mut unit = bit_unit.expect("bit-field unit exists after allocation");
                layouts.push(FieldLayout {
                    offset: unit.offset,
                    bit_offset: Some(unit.used_bits),
                    bit_width: Some(width),
                    storage_size: unit.storage_size,
                    storage_align: unit.storage_align,
                });
                unit.used_bits =
                    unit.used_bits.checked_add(width).ok_or(LayoutError::SizeOverflow { ty })?;
                if unit.used_bits == unit.storage_bits {
                    offset = finish_bit_unit(offset, Some(unit), ty)?;
                    bit_unit = None;
                } else {
                    bit_unit = Some(unit);
                }
                continue;
            }

            offset = finish_bit_unit(offset, bit_unit.take(), ty)?;
            let (field_layout, flexible) =
                self.field_storage_layout(field.ty, idx, fields.len(), record_stack)?;
            offset =
                align_to(offset, field_layout.align).ok_or(LayoutError::SizeOverflow { ty })?;
            layouts.push(FieldLayout {
                offset,
                bit_offset: None,
                bit_width: None,
                storage_size: field_layout.size,
                storage_align: field_layout.align,
            });
            max_align = max_align.max(field_layout.align);
            if !flexible {
                offset = offset
                    .checked_add(field_layout.size)
                    .ok_or(LayoutError::SizeOverflow { ty })?;
            }
        }
        offset = finish_bit_unit(offset, bit_unit, ty)?;
        let size = align_to(offset, max_align).ok_or(LayoutError::SizeOverflow { ty })?;
        Ok(RecordLayout { layout: Layout { size, align: max_align }, fields: layouts })
    }

    fn union_layout_details(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let mut size = 0_u64;
        let mut max_align = 1_u32;
        let mut layouts = Vec::with_capacity(fields.len());
        for (idx, field) in fields.iter().enumerate() {
            let (layout, flexible) =
                self.field_storage_layout(field.ty, idx, fields.len(), record_stack)?;
            let storage_size = if field.bit_width == Some(0) || flexible { 0 } else { layout.size };
            size = size.max(storage_size);
            max_align = max_align.max(layout.align);
            layouts.push(FieldLayout {
                offset: 0,
                bit_offset: field.bit_width.map(|_| 0),
                bit_width: field.bit_width,
                storage_size,
                storage_align: layout.align,
            });
        }
        let size = align_to(size, max_align).ok_or(LayoutError::SizeOverflow { ty })?;
        Ok(RecordLayout { layout: Layout { size, align: max_align }, fields: layouts })
    }

    fn field_storage_layout(
        &self,
        field_ty: TyId,
        idx: usize,
        field_count: usize,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<(Layout, bool)> {
        if matches!(self.tcx.get(field_ty), Ty::Array { len: None, is_vla: false, .. })
            && idx + 1 == field_count
        {
            let Ty::Array { elem, .. } = self.tcx.get(field_ty) else {
                unreachable!("flexible array match guarantees array type")
            };
            let elem_layout = self.layout_of_inner(elem.ty, record_stack)?;
            return Ok((Layout { size: 0, align: elem_layout.align }, true));
        }
        self.layout_of_inner(field_ty, record_stack).map(|layout| (layout, false))
    }

    fn enum_layout(&self, def: DefId) -> LayoutResult<Layout> {
        let Some(defs) = self.defs else {
            return Ok(int_layout(IntRank::Int));
        };
        let Some(def_data) = defs.get(def) else {
            return Ok(int_layout(IntRank::Int));
        };
        match &def_data.kind {
            DefKind::Enum { repr, .. } | DefKind::Enumerator { ty: repr, .. } => {
                self.layout_of_inner(*repr, &mut Vec::new())
            }
            _ => Ok(int_layout(IntRank::Int)),
        }
    }
}

fn int_layout(rank: IntRank) -> Layout {
    match rank {
        IntRank::Bool | IntRank::Char => Layout { size: 1, align: 1 },
        IntRank::Short => Layout { size: 2, align: 2 },
        IntRank::Int => Layout { size: 4, align: 4 },
        IntRank::Long | IntRank::LongLong => Layout { size: 8, align: 8 },
    }
}

fn float_layout(kind: FloatKind) -> Layout {
    match kind {
        FloatKind::F32 => Layout { size: 4, align: 4 },
        FloatKind::F64 => Layout { size: 8, align: 8 },
        FloatKind::F80 => Layout { size: 16, align: 16 },
    }
}

#[derive(Copy, Clone)]
struct BitUnit {
    offset: u64,
    storage_size: u64,
    storage_align: u32,
    storage_bits: u32,
    used_bits: u32,
}

fn storage_size_bits(layout: Layout, ty: TyId) -> LayoutResult<u32> {
    let bits = layout.size.checked_mul(8).ok_or(LayoutError::SizeOverflow { ty })?;
    u32::try_from(bits).map_err(|_| LayoutError::SizeOverflow { ty })
}

fn finish_bit_unit(offset: u64, bit_unit: Option<BitUnit>, ty: TyId) -> LayoutResult<u64> {
    match bit_unit {
        Some(unit) => {
            let unit_end = unit
                .offset
                .checked_add(unit.storage_size)
                .ok_or(LayoutError::SizeOverflow { ty })?;
            Ok(offset.max(unit_end))
        }
        None => Ok(offset),
    }
}

fn align_to(value: u64, align: u32) -> Option<u64> {
    let align = u64::from(align);
    if align <= 1 {
        return Some(value);
    }
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

#[cfg(test)]
mod tests {
    use rcc_span::{Symbol, DUMMY_SP};

    use super::*;
    use crate::{Field, RecordKind};

    fn record_def(defs: &mut IndexVec<DefId, Def>, kind: RecordKind, fields: Vec<Field>) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name: Symbol(1),
            span: DUMMY_SP,
            kind: DefKind::Record { kind, layout: None, fields },
        });
        defs[id].id = id;
        id
    }

    #[test]
    fn scalar_pointer_and_array_layout() {
        let mut tcx = TyCtxt::new();
        let ptr = tcx.intern(Ty::Ptr(crate::Qual::plain(tcx.int)));
        let arr = tcx.intern(Ty::Array {
            elem: crate::Qual::plain(tcx.int),
            len: Some(3),
            is_vla: false,
        });
        let nested =
            tcx.intern(Ty::Array { elem: crate::Qual::plain(arr), len: Some(2), is_vla: false });
        let layouts = LayoutCx::new(&tcx);

        assert_eq!(layouts.layout_of(tcx.int).unwrap(), Layout { size: 4, align: 4 });
        assert_eq!(layouts.layout_of(ptr).unwrap(), Layout { size: 8, align: 8 });
        assert_eq!(layouts.layout_of(arr).unwrap(), Layout { size: 12, align: 4 });
        assert_eq!(layouts.layout_of(nested).unwrap(), Layout { size: 24, align: 4 });
    }

    #[test]
    fn struct_and_union_layout() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let struct_id = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![
                Field {
                    name: None,
                    ty: tcx.char_,
                    quals: crate::ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                Field {
                    name: None,
                    ty: tcx.int,
                    quals: crate::ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
            ],
        );
        let union_id = record_def(
            &mut defs,
            RecordKind::Union,
            vec![
                Field {
                    name: None,
                    ty: tcx.char_,
                    quals: crate::ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                Field {
                    name: None,
                    ty: tcx.long,
                    quals: crate::ObjectQuals::none(),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
            ],
        );
        let struct_ty = tcx.intern(Ty::Record(struct_id));
        let union_ty = tcx.intern(Ty::Record(union_id));
        let layouts = LayoutCx::with_defs(&tcx, &defs);

        assert_eq!(layouts.layout_of(struct_ty).unwrap(), Layout { size: 8, align: 4 });
        assert_eq!(layouts.layout_of(union_ty).unwrap(), Layout { size: 8, align: 8 });
    }

    #[test]
    fn vla_and_missing_record_defs_are_explicit_errors() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let vla =
            tcx.intern(Ty::Array { elem: crate::Qual::plain(tcx.int), len: None, is_vla: true });
        let record = tcx.intern(Ty::Record(DefId(99)));
        let empty_record = {
            let def = record_def(&mut defs, RecordKind::Struct, Vec::new());
            tcx.intern(Ty::Record(def))
        };
        let layouts = LayoutCx::new(&tcx);

        assert!(matches!(
            layouts.layout_of(vla),
            Err(LayoutError::Unsized { reason: "VLA size is runtime-dependent", .. })
        ));
        assert!(matches!(
            layouts.layout_of(record),
            Err(LayoutError::MissingDefinitions { ty }) if ty == record
        ));
        assert!(matches!(
            LayoutCx::with_defs(&tcx, &defs).layout_of(empty_record),
            Err(LayoutError::Unsized { reason: "record has no fields or completed layout", .. })
        ));
    }
}
