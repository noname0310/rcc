//! Target-neutral type layout service for the baseline LP64 target.
//!
//! This module deliberately lives in `rcc_hir`, not `rcc_codegen_llvm`,
//! because CFG lowering needs `sizeof` answers before LLVM codegen runs.

use rcc_data_structures::IndexVec;

use crate::{Def, DefId, DefKind, FloatKind, IntRank, Layout, RecordKind, Ty, TyCtxt, TyId};

/// Result type used by layout queries.
pub type LayoutResult<T> = Result<T, LayoutError>;

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
    /// array objects, `Ty::Error`, unsupported bit-fields, and records
    /// when no definition table was supplied.
    pub fn layout_of(&self, ty: TyId) -> LayoutResult<Layout> {
        self.layout_of_inner(ty, &mut Vec::new())
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
            Ty::Array { elem, len: Some(len), is_vla: false } => {
                let elem_layout = self.layout_of_inner(elem.ty, record_stack)?;
                let size =
                    elem_layout.size.checked_mul(*len).ok_or(LayoutError::SizeOverflow { ty })?;
                Ok(Layout { size, align: elem_layout.align })
            }
            Ty::Array { is_vla: true, .. } => {
                Err(LayoutError::Unsized { ty, reason: "VLA size is runtime-dependent" })
            }
            Ty::Array { len: None, .. } => {
                Err(LayoutError::Unsized { ty, reason: "incomplete array has no object size" })
            }
            Ty::Record(def) => self.record_layout(ty, *def, record_stack),
            Ty::Enum(def) => self.enum_layout(*def),
            Ty::Error => Err(LayoutError::Unsized { ty, reason: "error type has no layout" }),
        }
    }

    fn record_layout(
        &self,
        ty: TyId,
        def: DefId,
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<Layout> {
        let defs = self.defs.ok_or(LayoutError::MissingDefinitions { ty })?;
        if record_stack.contains(&def) {
            return Err(LayoutError::Unsupported { ty, feature: "recursive record by value" });
        }
        let def_data = defs.get(def).ok_or(LayoutError::MissingDefinition { def })?;
        let DefKind::Record { kind, layout, fields } = &def_data.kind else {
            return Err(LayoutError::ExpectedRecord { def });
        };
        if let Some(layout) = layout {
            return Ok(*layout);
        }
        if fields.is_empty() {
            return Err(LayoutError::Unsized {
                ty,
                reason: "record has no fields or completed layout",
            });
        }

        record_stack.push(def);
        let result = match kind {
            RecordKind::Struct => self.struct_layout(ty, fields, record_stack),
            RecordKind::Union => self.union_layout(ty, fields, record_stack),
        };
        record_stack.pop();
        result
    }

    fn struct_layout(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<Layout> {
        let mut offset = 0_u64;
        let mut max_align = 1_u32;
        for field in fields {
            if field.bit_width.is_some() {
                return Err(LayoutError::Unsupported { ty, feature: "bit-field layout" });
            }
            let layout = self.layout_of_inner(field.ty, record_stack)?;
            offset = align_to(offset, layout.align).ok_or(LayoutError::SizeOverflow { ty })?;
            offset = offset.checked_add(layout.size).ok_or(LayoutError::SizeOverflow { ty })?;
            max_align = max_align.max(layout.align);
        }
        let size = align_to(offset, max_align).ok_or(LayoutError::SizeOverflow { ty })?;
        Ok(Layout { size, align: max_align })
    }

    fn union_layout(
        &self,
        ty: TyId,
        fields: &[crate::Field],
        record_stack: &mut Vec<DefId>,
    ) -> LayoutResult<Layout> {
        let mut size = 0_u64;
        let mut max_align = 1_u32;
        for field in fields {
            if field.bit_width.is_some() {
                return Err(LayoutError::Unsupported { ty, feature: "bit-field layout" });
            }
            let layout = self.layout_of_inner(field.ty, record_stack)?;
            size = size.max(layout.size);
            max_align = max_align.max(layout.align);
        }
        let size = align_to(size, max_align).ok_or(LayoutError::SizeOverflow { ty })?;
        Ok(Layout { size, align: max_align })
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
