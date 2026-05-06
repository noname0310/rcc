//! Target-neutral type layout service for the baseline LP64 target.
//!
//! This module deliberately lives in `rcc_hir`, not `rcc_codegen_llvm`,
//! because CFG lowering needs `sizeof` answers before LLVM codegen runs.

use rcc_data_structures::IndexVec;
use rcc_target::{Endianness, FloatLayoutKind, IntRankLayout, TargetInfo, TypeLayout};

use crate::{
    Def, DefId, DefKind, FloatKind, IntRank, Layout, RecordKind, ScalarStorageOrder, Ty, TyCtxt,
    TyId,
};

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
    /// GNU scalar storage order inherited from the enclosing record.
    pub scalar_storage_order: Option<ScalarStorageOrder>,
}

#[derive(Copy, Clone)]
struct StructLayoutAttrs {
    packed: bool,
    ms_bitfields: bool,
    align_override: Option<u32>,
    scalar_storage_order: Option<ScalarStorageOrder>,
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

/// Layout context for one compilation target.
///
/// Scalar layouts are read from [`TargetInfo`]. Aggregate layout is independent
/// of LLVM and can therefore be shared by CFG lowering, constant evaluation,
/// and the LLVM backend.
pub struct LayoutCx<'tcx> {
    /// Backing type context.
    pub tcx: &'tcx TyCtxt,
    target: TargetInfo,
    defs: Option<&'tcx IndexVec<DefId, Def>>,
}

impl<'tcx> LayoutCx<'tcx> {
    /// Build a layout context without access to top-level definitions.
    ///
    /// This is sufficient for scalar, pointer, enum-as-int, and array
    /// layouts that do not contain records.
    #[must_use]
    pub fn new(tcx: &'tcx TyCtxt) -> Self {
        Self::for_target(tcx, TargetInfo::baseline())
    }

    /// Build a layout context for an explicit target without access to
    /// top-level definitions.
    #[must_use]
    pub fn for_target(tcx: &'tcx TyCtxt, target: TargetInfo) -> Self {
        Self { tcx, target, defs: None }
    }

    /// Build a layout context that can resolve record and enum definitions.
    #[must_use]
    pub fn with_defs(tcx: &'tcx TyCtxt, defs: &'tcx IndexVec<DefId, Def>) -> Self {
        Self::with_defs_for_target(tcx, defs, TargetInfo::baseline())
    }

    /// Build a layout context that can resolve record and enum definitions for
    /// an explicit target.
    #[must_use]
    pub fn with_defs_for_target(
        tcx: &'tcx TyCtxt,
        defs: &'tcx IndexVec<DefId, Def>,
        target: TargetInfo,
    ) -> Self {
        Self { tcx, target, defs: Some(defs) }
    }

    /// Target facts used by this layout context.
    #[must_use]
    pub fn target(&self) -> &TargetInfo {
        &self.target
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
            Ty::Int { rank, .. } => Ok(type_layout(self.target.int_layout(int_rank_layout(*rank)))),
            Ty::Float(kind) => Ok(type_layout(self.target.float_layout(float_layout_kind(*kind)))),
            Ty::Complex(kind) => {
                let base = type_layout(self.target.float_layout(float_layout_kind(*kind)));
                Ok(Layout {
                    size: base.size.checked_mul(2).ok_or(LayoutError::SizeOverflow { ty })?,
                    align: base.align,
                })
            }
            Ty::Vector { bytes, .. } => Ok(Layout { size: *bytes, align: vector_align(*bytes) }),
            Ty::Atomic(inner) => self.layout_of_inner(*inner, record_stack),
            Ty::Ptr(_) => Ok(type_layout(self.target.layouts.pointer)),
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
            Ty::BuiltinVaList => Ok(type_layout(self.target.layouts.builtin_va_list)),
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
        let DefKind::Record {
            kind,
            packed,
            ms_bitfields,
            align_override,
            scalar_storage_order,
            layout,
            fields,
        } = &def_data.kind
        else {
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
                        scalar_storage_order: *scalar_storage_order,
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
            RecordKind::Struct => self.struct_layout_details(
                ty,
                fields,
                StructLayoutAttrs {
                    packed: *packed,
                    ms_bitfields: *ms_bitfields,
                    align_override: *align_override,
                    scalar_storage_order: *scalar_storage_order,
                },
                record_stack,
            ),
            RecordKind::Union => self.union_layout_details(
                ty,
                fields,
                *packed,
                *align_override,
                *scalar_storage_order,
                record_stack,
            ),
        };
        record_stack.pop();
        result
    }

    fn struct_layout_details(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        attrs: StructLayoutAttrs,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let StructLayoutAttrs { packed, ms_bitfields, align_override, scalar_storage_order } =
            attrs;
        if ms_bitfields {
            return self.struct_layout_details_ms(ty, fields, attrs, record_stack);
        }
        let mut offset = 0_u64;
        let mut max_align = 1_u32;
        let mut layouts = Vec::with_capacity(fields.len());
        let mut bit_cursor = 0_u64;
        let msb_first = self.bitfields_are_msb_first(scalar_storage_order);
        for (idx, field) in fields.iter().enumerate() {
            if let Some(width) = field.bit_width {
                let natural_storage =
                    apply_field_align(self.layout_of_inner(field.ty, record_stack)?, field);
                let storage = apply_packed_layout(natural_storage, packed);
                let storage_bits = storage_size_bits(natural_storage, ty)?;
                max_align = max_align.max(storage.align);

                if width == 0 {
                    bit_cursor = next_allocation_unit(bit_cursor, storage_bits, ty)?;
                    offset = offset.max(bits_to_bytes(bit_cursor, ty)?);
                    layouts.push(FieldLayout {
                        offset: bits_to_bytes(bit_cursor, ty)?,
                        bit_offset: Some(0),
                        bit_width: Some(width),
                        storage_size: 0,
                        storage_align: storage.align,
                        scalar_storage_order,
                    });
                    continue;
                }

                if packed {
                    if field.align_override.is_some() {
                        bit_cursor = next_allocation_unit(bit_cursor, storage_bits, ty)?;
                    }
                    let field_start = bit_cursor;
                    let field_offset = field_start / 8;
                    let bit_offset = u32::try_from(field_start % 8)
                        .map_err(|_| LayoutError::SizeOverflow { ty })?;
                    let storage_size = bits_to_bytes(
                        u64::from(bit_offset)
                            .checked_add(u64::from(width))
                            .ok_or(LayoutError::SizeOverflow { ty })?,
                        ty,
                    )?;
                    let storage_bits =
                        storage_size_bits(Layout { size: storage_size, align: storage.align }, ty)?;
                    let bit_offset = if msb_first {
                        storage_bits
                            .checked_sub(bit_offset)
                            .and_then(|base| base.checked_sub(width))
                            .ok_or(LayoutError::SizeOverflow { ty })?
                    } else {
                        bit_offset
                    };
                    layouts.push(FieldLayout {
                        offset: field_offset,
                        bit_offset: Some(bit_offset),
                        bit_width: Some(width),
                        storage_size,
                        storage_align: storage.align,
                        scalar_storage_order,
                    });
                    bit_cursor = bit_cursor
                        .checked_add(u64::from(width))
                        .ok_or(LayoutError::SizeOverflow { ty })?;
                    offset = offset.max(
                        field_offset
                            .checked_add(storage_size)
                            .ok_or(LayoutError::SizeOverflow { ty })?,
                    );
                    continue;
                }

                if field.align_override.is_some() {
                    bit_cursor = align_bits_to(bit_cursor, storage.align)
                        .ok_or(LayoutError::SizeOverflow { ty })?;
                }
                let (unit_base_bits, start_bits) =
                    place_in_declared_unit(bit_cursor, storage_bits, width, ty)?;
                let within = start_bits
                    .checked_sub(unit_base_bits)
                    .ok_or(LayoutError::SizeOverflow { ty })?;
                let within = u32::try_from(within).map_err(|_| LayoutError::SizeOverflow { ty })?;
                let bit_offset = if msb_first {
                    storage_bits
                        .checked_sub(within)
                        .and_then(|base| base.checked_sub(width))
                        .ok_or(LayoutError::SizeOverflow { ty })?
                } else {
                    within
                };
                layouts.push(FieldLayout {
                    offset: unit_base_bits / 8,
                    bit_offset: Some(bit_offset),
                    bit_width: Some(width),
                    storage_size: natural_storage.size,
                    storage_align: storage.align,
                    scalar_storage_order,
                });
                bit_cursor = start_bits
                    .checked_add(u64::from(width))
                    .ok_or(LayoutError::SizeOverflow { ty })?;
                offset = offset.max(
                    (unit_base_bits / 8)
                        .checked_add(natural_storage.size)
                        .ok_or(LayoutError::SizeOverflow { ty })?,
                );
                continue;
            }

            let (field_layout, flexible) =
                self.field_storage_layout(field.ty, idx, fields.len(), record_stack)?;
            let field_layout = apply_packed_layout(apply_field_align(field_layout, field), packed);
            let field_offset = bits_to_bytes(bit_cursor, ty)?;
            let field_offset = align_to(field_offset, field_layout.align)
                .ok_or(LayoutError::SizeOverflow { ty })?;
            layouts.push(FieldLayout {
                offset: field_offset,
                bit_offset: None,
                bit_width: None,
                storage_size: field_layout.size,
                storage_align: field_layout.align,
                scalar_storage_order: None,
            });
            max_align = max_align.max(field_layout.align);
            if !flexible {
                let field_end = field_offset
                    .checked_add(field_layout.size)
                    .ok_or(LayoutError::SizeOverflow { ty })?;
                offset = offset.max(field_end);
                bit_cursor = field_end.checked_mul(8).ok_or(LayoutError::SizeOverflow { ty })?;
            } else {
                bit_cursor = field_offset.checked_mul(8).ok_or(LayoutError::SizeOverflow { ty })?;
            }
        }
        let has_record_align_override = align_override.is_some();
        if let Some(align) = align_override {
            max_align = max_align.max(align);
        }
        let size = if packed && !has_record_align_override {
            offset
        } else {
            align_to(offset, max_align).ok_or(LayoutError::SizeOverflow { ty })?
        };
        Ok(RecordLayout { layout: Layout { size, align: max_align }, fields: layouts })
    }

    fn struct_layout_details_ms(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        attrs: StructLayoutAttrs,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let StructLayoutAttrs { packed, align_override, scalar_storage_order, .. } = attrs;
        let mut offset = 0_u64;
        let mut max_align = 1_u32;
        let mut layouts = Vec::with_capacity(fields.len());
        let mut bit_unit: Option<MsBitUnit> = None;
        let msb_first = self.bitfields_are_msb_first(scalar_storage_order);
        for (idx, field) in fields.iter().enumerate() {
            if let Some(width) = field.bit_width {
                let natural_storage =
                    apply_field_align(self.layout_of_inner(field.ty, record_stack)?, field);
                let storage = if packed {
                    Layout {
                        size: natural_storage.size,
                        align: if field.align_override.is_some() {
                            natural_storage.align
                        } else {
                            1
                        },
                    }
                } else {
                    natural_storage
                };
                let storage_bits = storage_size_bits(natural_storage, ty)?;
                max_align = max_align.max(storage.align);

                if width == 0 {
                    if let Some(unit) = bit_unit.take() {
                        offset = offset.max(unit.end_offset(ty)?);
                    }
                    offset =
                        align_to(offset, storage.align).ok_or(LayoutError::SizeOverflow { ty })?;
                    layouts.push(FieldLayout {
                        offset,
                        bit_offset: Some(0),
                        bit_width: Some(width),
                        storage_size: 0,
                        storage_align: storage.align,
                        scalar_storage_order,
                    });
                    continue;
                }

                let compatible = |unit: &MsBitUnit| {
                    unit.storage_size == natural_storage.size
                        && unit.storage_bits == storage_bits
                        && unit.storage_align == storage.align
                };
                let can_share = bit_unit.as_ref().is_some_and(|unit| {
                    compatible(unit)
                        && u64::from(unit.used_bits) + u64::from(width)
                            <= u64::from(unit.storage_bits)
                });
                if !can_share {
                    if let Some(unit) = bit_unit.take() {
                        offset = offset.max(unit.end_offset(ty)?);
                    }
                    offset =
                        align_to(offset, storage.align).ok_or(LayoutError::SizeOverflow { ty })?;
                    bit_unit = Some(MsBitUnit {
                        offset,
                        storage_size: natural_storage.size,
                        storage_align: storage.align,
                        storage_bits,
                        used_bits: 0,
                    });
                }

                let unit = bit_unit.as_mut().expect("MS bit-field unit exists after allocation");
                let bit_offset = if msb_first {
                    unit.storage_bits
                        .checked_sub(unit.used_bits)
                        .and_then(|base| base.checked_sub(width))
                        .ok_or(LayoutError::SizeOverflow { ty })?
                } else {
                    unit.used_bits
                };
                layouts.push(FieldLayout {
                    offset: unit.offset,
                    bit_offset: Some(bit_offset),
                    bit_width: Some(width),
                    storage_size: unit.storage_size,
                    storage_align: unit.storage_align,
                    scalar_storage_order,
                });
                unit.used_bits =
                    unit.used_bits.checked_add(width).ok_or(LayoutError::SizeOverflow { ty })?;
                if unit.used_bits == unit.storage_bits {
                    let unit = bit_unit.take().expect("unit exists");
                    offset = offset.max(unit.end_offset(ty)?);
                }
                continue;
            }

            if let Some(unit) = bit_unit.take() {
                offset = offset.max(unit.end_offset(ty)?);
            }
            let (field_layout, flexible) =
                self.field_storage_layout(field.ty, idx, fields.len(), record_stack)?;
            let natural_field_layout = apply_field_align(field_layout, field);
            let field_layout = if packed {
                Layout {
                    size: natural_field_layout.size,
                    align: if field.align_override.is_some() {
                        natural_field_layout.align
                    } else {
                        1
                    },
                }
            } else {
                natural_field_layout
            };
            offset =
                align_to(offset, field_layout.align).ok_or(LayoutError::SizeOverflow { ty })?;
            layouts.push(FieldLayout {
                offset,
                bit_offset: None,
                bit_width: None,
                storage_size: field_layout.size,
                storage_align: field_layout.align,
                scalar_storage_order: None,
            });
            max_align = max_align.max(field_layout.align);
            if !flexible {
                offset = offset
                    .checked_add(field_layout.size)
                    .ok_or(LayoutError::SizeOverflow { ty })?;
            }
        }
        if let Some(unit) = bit_unit {
            offset = offset.max(unit.end_offset(ty)?);
        }
        let has_record_align_override = align_override.is_some();
        if let Some(align) = align_override {
            max_align = max_align.max(align);
        }
        let size = if packed && !has_record_align_override {
            offset
        } else {
            align_to(offset, max_align).ok_or(LayoutError::SizeOverflow { ty })?
        };
        Ok(RecordLayout { layout: Layout { size, align: max_align }, fields: layouts })
    }

    fn union_layout_details(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        packed: bool,
        align_override: Option<u32>,
        scalar_storage_order: Option<ScalarStorageOrder>,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<RecordLayout> {
        let mut size = 0_u64;
        let mut max_align = 1_u32;
        let mut layouts = Vec::with_capacity(fields.len());
        for (idx, field) in fields.iter().enumerate() {
            let (layout, flexible) =
                self.field_storage_layout(field.ty, idx, fields.len(), record_stack)?;
            let layout = apply_field_align(apply_packed_layout(layout, packed), field);
            let storage_size = if field.bit_width == Some(0) || flexible { 0 } else { layout.size };
            let bit_offset = match field.bit_width {
                Some(width) if width != 0 && self.bitfields_are_msb_first(scalar_storage_order) => {
                    Some(storage_size_bits(layout, ty)? - width)
                }
                Some(_) => Some(0),
                None => None,
            };
            size = size.max(storage_size);
            max_align = max_align.max(layout.align);
            layouts.push(FieldLayout {
                offset: 0,
                bit_offset,
                bit_width: field.bit_width,
                storage_size,
                storage_align: layout.align,
                scalar_storage_order,
            });
        }
        if let Some(align) = align_override {
            max_align = max_align.max(align);
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
            return Ok(type_layout(self.target.int_layout(IntRankLayout::Int)));
        };
        let Some(def_data) = defs.get(def) else {
            return Ok(type_layout(self.target.int_layout(IntRankLayout::Int)));
        };
        match &def_data.kind {
            DefKind::Enum { repr, .. } | DefKind::Enumerator { ty: repr, .. } => {
                self.layout_of_inner(*repr, &mut Vec::new())
            }
            _ => Ok(type_layout(self.target.int_layout(IntRankLayout::Int))),
        }
    }

    fn bitfields_are_msb_first(&self, scalar_storage_order: Option<ScalarStorageOrder>) -> bool {
        match scalar_storage_order {
            Some(ScalarStorageOrder::BigEndian) => true,
            Some(ScalarStorageOrder::LittleEndian) => false,
            None => self.target.endianness == Endianness::Big,
        }
    }
}

fn type_layout(layout: TypeLayout) -> Layout {
    Layout { size: layout.size, align: layout.align }
}

fn vector_align(bytes: u64) -> u32 {
    u32::try_from(bytes.min(16)).unwrap_or(16).max(1)
}

struct MsBitUnit {
    offset: u64,
    storage_size: u64,
    storage_align: u32,
    storage_bits: u32,
    used_bits: u32,
}

impl MsBitUnit {
    fn end_offset(&self, ty: TyId) -> LayoutResult<u64> {
        self.offset.checked_add(self.storage_size).ok_or(LayoutError::SizeOverflow { ty })
    }
}

fn apply_field_align(mut layout: Layout, field: &crate::Field) -> Layout {
    if let Some(align) = field.align_override {
        layout.align = layout.align.max(align);
    }
    layout
}

fn apply_packed_layout(mut layout: Layout, packed: bool) -> Layout {
    if packed {
        layout.align = 1;
    }
    layout
}

fn int_rank_layout(rank: IntRank) -> IntRankLayout {
    match rank {
        IntRank::Bool => IntRankLayout::Bool,
        IntRank::Char => IntRankLayout::Char,
        IntRank::Short => IntRankLayout::Short,
        IntRank::Int => IntRankLayout::Int,
        IntRank::Long => IntRankLayout::Long,
        IntRank::LongLong => IntRankLayout::LongLong,
    }
}

fn float_layout_kind(kind: FloatKind) -> FloatLayoutKind {
    match kind {
        FloatKind::F32 => FloatLayoutKind::Float,
        FloatKind::F64 => FloatLayoutKind::Double,
        FloatKind::F80 => FloatLayoutKind::LongDouble,
    }
}

fn storage_size_bits(layout: Layout, ty: TyId) -> LayoutResult<u32> {
    let bits = layout.size.checked_mul(8).ok_or(LayoutError::SizeOverflow { ty })?;
    u32::try_from(bits).map_err(|_| LayoutError::SizeOverflow { ty })
}

fn place_in_declared_unit(
    cursor: u64,
    storage_bits: u32,
    width: u32,
    ty: TyId,
) -> LayoutResult<(u64, u64)> {
    let storage_bits = u64::from(storage_bits);
    let width = u64::from(width);
    let unit_base = (cursor / storage_bits)
        .checked_mul(storage_bits)
        .ok_or(LayoutError::SizeOverflow { ty })?;
    if cursor.checked_add(width).ok_or(LayoutError::SizeOverflow { ty })?
        <= unit_base.checked_add(storage_bits).ok_or(LayoutError::SizeOverflow { ty })?
    {
        return Ok((unit_base, cursor));
    }
    let next = unit_base.checked_add(storage_bits).ok_or(LayoutError::SizeOverflow { ty })?;
    Ok((next, next))
}

fn next_allocation_unit(cursor: u64, storage_bits: u32, ty: TyId) -> LayoutResult<u64> {
    let storage_bits = u64::from(storage_bits);
    let unit_base = (cursor / storage_bits)
        .checked_mul(storage_bits)
        .ok_or(LayoutError::SizeOverflow { ty })?;
    if cursor == unit_base {
        Ok(cursor)
    } else {
        unit_base.checked_add(storage_bits).ok_or(LayoutError::SizeOverflow { ty })
    }
}

fn bits_to_bytes(bits: u64, ty: TyId) -> LayoutResult<u64> {
    bits.checked_add(7).map(|value| value / 8).ok_or(LayoutError::SizeOverflow { ty })
}

fn align_bits_to(bits: u64, byte_align: u32) -> Option<u64> {
    align_to(bits, byte_align.checked_mul(8)?)
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
        record_def_with_order(defs, kind, None, fields)
    }

    fn record_def_with_order(
        defs: &mut IndexVec<DefId, Def>,
        kind: RecordKind,
        scalar_storage_order: Option<ScalarStorageOrder>,
        fields: Vec<Field>,
    ) -> DefId {
        let id = defs.push(Def {
            id: DefId(0),
            name: Symbol(1),
            span: DUMMY_SP,
            kind: DefKind::Record {
                kind,
                packed: false,
                ms_bitfields: false,
                align_override: None,
                scalar_storage_order,
                layout: None,
                fields,
            },
        });
        defs[id].id = id;
        id
    }

    fn packed_record_def(
        defs: &mut IndexVec<DefId, Def>,
        kind: RecordKind,
        fields: Vec<Field>,
    ) -> DefId {
        let id = record_def_with_order(defs, kind, None, fields);
        let DefKind::Record { packed, .. } = &mut defs[id].kind else {
            unreachable!("record_def_with_order creates a record")
        };
        *packed = true;
        id
    }

    fn field(ty: TyId) -> Field {
        Field {
            name: None,
            ty,
            quals: crate::ObjectQuals::none(),
            align_override: None,
            offset: None,
            bit_width: None,
            span: DUMMY_SP,
        }
    }

    fn bitfield(ty: TyId, width: u32) -> Field {
        Field { bit_width: Some(width), ..field(ty) }
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
    fn target_info_drives_scalar_layout() {
        let tcx = TyCtxt::new();
        let windows =
            TargetInfo::from_triple(&rcc_target::TargetTriple::new("x86_64-pc-windows-msvc"))
                .unwrap();
        let layouts = LayoutCx::for_target(&tcx, windows);

        assert_eq!(layouts.layout_of(tcx.long).unwrap(), Layout { size: 4, align: 4 });
        assert_eq!(layouts.layout_of(tcx.long_long).unwrap(), Layout { size: 8, align: 8 });
        assert_eq!(layouts.layout_of(tcx.long_double).unwrap(), Layout { size: 8, align: 8 });
    }

    #[test]
    fn vector_layout_reports_total_size_and_abi_alignment() {
        let mut tcx = TyCtxt::new();
        let v4si = tcx.intern(Ty::Vector { elem: tcx.int, lanes: 4, bytes: 16 });
        let v2si = tcx.intern(Ty::Vector { elem: tcx.int, lanes: 2, bytes: 8 });
        let layouts = LayoutCx::new(&tcx);

        assert_eq!(layouts.layout_of(v4si).unwrap(), Layout { size: 16, align: 16 });
        assert_eq!(layouts.layout_of(v2si).unwrap(), Layout { size: 8, align: 8 });
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
                    align_override: None,
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                Field {
                    name: None,
                    ty: tcx.int,
                    quals: crate::ObjectQuals::none(),
                    align_override: None,
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
                    align_override: None,
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                Field {
                    name: None,
                    ty: tcx.long,
                    quals: crate::ObjectQuals::none(),
                    align_override: None,
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
    fn field_alignment_override_raises_offset_size_and_record_alignment() {
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
                    align_override: None,
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
                Field {
                    name: None,
                    ty: tcx.int,
                    quals: crate::ObjectQuals::none(),
                    align_override: Some(8),
                    offset: None,
                    bit_width: None,
                    span: DUMMY_SP,
                },
            ],
        );
        let struct_ty = tcx.intern(Ty::Record(struct_id));
        let record = LayoutCx::with_defs(&tcx, &defs).record_layout_of(struct_ty).unwrap();

        assert_eq!(record.layout, Layout { size: 16, align: 8 });
        assert_eq!(record.fields[0].offset, 0);
        assert_eq!(record.fields[0].storage_align, 1);
        assert_eq!(record.fields[1].offset, 8);
        assert_eq!(record.fields[1].storage_align, 8);
    }

    #[test]
    fn mixed_underlying_bitfields_start_at_declared_type_boundaries() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def(
            &mut defs,
            RecordKind::Struct,
            vec![
                bitfield(tcx.short, 12),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
            ],
        );
        let rec_ty = tcx.intern(Ty::Record(rec));
        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(rec_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 2, align: 2 });
        assert_eq!(layout.fields[0].bit_offset, Some(0));
        assert_eq!(layout.fields[1].offset, 1);
        assert_eq!(layout.fields[1].bit_offset, Some(4));
        assert!(layout.fields[1..].iter().all(|field| field.storage_size == 1));
    }

    #[test]
    fn scalar_storage_order_big_endian_uses_msb_first_bit_offsets() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = record_def_with_order(
            &mut defs,
            RecordKind::Struct,
            Some(ScalarStorageOrder::BigEndian),
            vec![
                bitfield(tcx.short, 12),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
                bitfield(tcx.char_, 1),
            ],
        );
        let rec_ty = tcx.intern(Ty::Record(rec));
        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(rec_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 2, align: 2 });
        assert_eq!(layout.fields[0].bit_offset, Some(4));
        assert_eq!(layout.fields[1].offset, 1);
        assert_eq!(layout.fields[1].bit_offset, Some(3));
        assert_eq!(layout.fields[2].bit_offset, Some(2));
        assert_eq!(layout.fields[3].bit_offset, Some(1));
        assert_eq!(layout.fields[4].bit_offset, Some(0));
        assert!(layout
            .fields
            .iter()
            .all(|field| field.scalar_storage_order == Some(ScalarStorageOrder::BigEndian)));
    }

    #[test]
    fn packed_bitfields_cross_declared_type_boundaries_bit_contiguously() {
        let mut tcx = TyCtxt::new();
        let mut defs = IndexVec::new();
        let rec = packed_record_def(
            &mut defs,
            RecordKind::Struct,
            vec![bitfield(tcx.uint, 12), bitfield(tcx.uchar, 7), bitfield(tcx.uint, 12)],
        );
        let rec_ty = tcx.intern(Ty::Record(rec));
        let layout = LayoutCx::with_defs(&tcx, &defs).record_layout_of(rec_ty).unwrap();

        assert_eq!(layout.layout, Layout { size: 4, align: 1 });
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[0].bit_offset, Some(0));
        assert_eq!(layout.fields[0].storage_size, 2);
        assert_eq!(layout.fields[1].offset, 1);
        assert_eq!(layout.fields[1].bit_offset, Some(4));
        assert_eq!(layout.fields[1].storage_size, 2);
        assert_eq!(layout.fields[2].offset, 2);
        assert_eq!(layout.fields[2].bit_offset, Some(3));
        assert_eq!(layout.fields[2].storage_size, 2);
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
