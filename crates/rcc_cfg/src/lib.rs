//! `rcc_cfg`: MIR-style control-flow graph for the rcc C compiler.
//!
//! Roughly analogous to `rustc_middle::mir`. The CFG is intentionally
//! non-SSA; SSA construction is delegated to LLVM's `mem2reg` pass, which
//! promotes the `alloca + load/store` pattern we emit.

#![forbid(unsafe_code)]
// Variants carry docs at the enum level; per-field docs would be noise.
#![allow(missing_docs)]

use rcc_data_structures::IndexVec;
use rcc_hir::{DefId, Local, ObjectQuals, TyId};
use rcc_span::Span;

pub mod build;
pub mod lower;
pub mod pretty;
pub mod verify;

pub use build::{build_bodies, BodyBuilder, BreakCtx, LoopCtx};
pub use lower::{lower_as_place, lower_as_rvalue, lower_stmt, LocalMap, LowerCx};

rcc_data_structures::new_index! {
    /// Basic-block id within a `Body`.
    pub struct BasicBlockId = u32;
}

/// Per-function CFG.
#[derive(Debug, Clone, Default)]
pub struct Body {
    /// Function this body belongs to.
    pub def: Option<DefId>,
    /// Locals (parameters first, then declared locals, then temporaries).
    pub locals: IndexVec<Local, LocalDecl>,
    /// Basic blocks. `blocks[0]` is always the entry block.
    pub blocks: IndexVec<BasicBlockId, BasicBlock>,
    /// Return type.
    pub ret_ty: Option<TyId>,
}

/// Metadata for one local slot.
#[derive(Debug, Clone)]
pub struct LocalDecl {
    /// Optional source name (for debug info / pretty print).
    pub name: Option<rcc_span::Symbol>,
    /// Type of the slot.
    pub ty: TyId,
    /// Object qualifiers preserved from HIR for codegen access policy.
    pub quals: ObjectQuals,
    /// Runtime element-count local for a VLA allocation.
    pub vla_len: Option<Local>,
    /// Whether this is a function parameter.
    pub is_param: bool,
    /// Declaration span.
    pub span: Span,
}

/// A single basic block.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Straight-line statements.
    pub statements: Vec<Statement>,
    /// Terminator (always present in a well-formed body).
    pub terminator: Terminator,
}

impl Default for BasicBlock {
    fn default() -> Self {
        Self {
            statements: Vec::new(),
            terminator: Terminator { kind: TerminatorKind::Unreachable, span: rcc_span::DUMMY_SP },
        }
    }
}

/// One straight-line statement.
#[derive(Debug, Clone)]
pub struct Statement {
    /// Kind.
    pub kind: StatementKind,
    /// Source span.
    pub span: Span,
}

/// Statement discriminant.
#[derive(Debug, Clone)]
pub enum StatementKind {
    /// `place = rvalue`.
    Assign { place: Place, rvalue: Rvalue },
    /// Mark a local as live. Must dominate every use.
    StorageLive(Local),
    /// Mark a local as dead. Reads after this are UB.
    StorageDead(Local),
    /// No-op (preserved for debug info / comments in IR dumps).
    Nop,
}

/// Terminator for a basic block.
#[derive(Debug, Clone)]
pub struct Terminator {
    /// Kind.
    pub kind: TerminatorKind,
    /// Source span.
    pub span: Span,
}

/// Terminator discriminant.
#[derive(Debug, Clone)]
pub enum TerminatorKind {
    /// Jump to `target`.
    Goto(BasicBlockId),
    /// Switch over an integer scrutinee.
    SwitchInt {
        /// Value being matched.
        discr: Operand,
        /// `(value, target)` pairs; last entry is `default`.
        targets: Vec<(Option<i128>, BasicBlockId)>,
    },
    /// Return.
    Return,
    /// `callee(args...)`, writing to `destination`, continuing at `target`.
    Call {
        /// Function operand (pointer).
        callee: Operand,
        /// Call arguments.
        args: Vec<Operand>,
        /// Destination place for the return value (`None` for `void`).
        destination: Option<Place>,
        /// Control transfers here on normal return.
        target: Option<BasicBlockId>,
    },
    /// Unreachable (missing `return`, `__builtin_unreachable`).
    Unreachable,
    /// `__builtin_va_start(ap, last_param)`.
    BuiltinVaStart {
        /// va_list operand.
        ap: Operand,
        /// Last named parameter.
        last_param: Operand,
        /// Control transfers here after the intrinsic call.
        target: BasicBlockId,
    },
    /// `__builtin_va_end(ap)`.
    BuiltinVaEnd {
        /// va_list operand.
        ap: Operand,
        /// Control transfers here after the intrinsic call.
        target: BasicBlockId,
    },
    /// `__builtin_va_copy(dst, src)`.
    BuiltinVaCopy {
        /// Destination va_list.
        dst: Operand,
        /// Source va_list.
        src: Operand,
        /// Control transfers here after the intrinsic call.
        target: BasicBlockId,
    },
}

/// A memory location addressable by the IR.
#[derive(Debug, Clone)]
pub struct Place {
    /// Base local.
    pub base: Local,
    /// Projections applied in order.
    pub projection: Vec<Projection>,
}

/// One step of a place projection.
#[derive(Debug, Clone)]
pub enum Projection {
    /// `*base` — pointer dereference.
    Deref,
    /// `base.field` — record field index.
    Field(u32),
    /// `base[index]` — array indexing.
    Index(Operand),
}

/// Operand: value used in an rvalue or terminator.
#[derive(Debug, Clone)]
pub enum Operand {
    /// Copy from a place (safe-ish alias).
    Copy(Place),
    /// Move from a place (the source is dead after this).
    Move(Place),
    /// Constant value.
    Const(Const),
}

/// Constant operand.
#[derive(Debug, Clone)]
pub struct Const {
    /// Value.
    pub kind: ConstKind,
    /// Type.
    pub ty: TyId,
}

/// Constant kinds.
#[derive(Debug, Clone)]
pub enum ConstKind {
    /// Integer.
    Int(i128),
    /// Float.
    Float(f64),
    /// Address of a global / string literal.
    Global(DefId),
    /// Zero-initialised aggregate sentinel.
    ZeroInit,
}

/// Right-hand side of an assignment.
#[derive(Debug, Clone)]
pub enum Rvalue {
    /// Pass-through of a single operand.
    Use(Operand),
    /// Binary op.
    BinaryOp(BinOp, Operand, Operand),
    /// Unary op.
    UnaryOp(UnOp, Operand),
    /// Cast.
    Cast {
        /// Operand being cast.
        op: Operand,
        /// Target type.
        to: TyId,
        /// Cast kind (integer, pointer, ...).
        kind: CastKind,
    },
    /// C99 real -> complex conversion: construct `to` from `real + 0i`.
    ///
    /// Backend contract: codegen must emit a complex value whose real
    /// component is `real` converted to the corresponding real element type,
    /// and whose imaginary component is zero.
    ComplexFromReal {
        /// Real operand to place into the complex real component.
        real: Operand,
        /// Target complex type.
        to: TyId,
    },
    /// C99 complex -> real conversion: extract the real component.
    ///
    /// Backend contract: codegen must read only the real component, discarding
    /// the imaginary component. Typeck is responsible for W0012.
    RealFromComplex {
        /// Complex operand to read.
        complex: Operand,
        /// Target real type.
        to: TyId,
    },
    /// Take the address of a place.
    AddressOf(Place),
    /// Array/struct length (used for VLA).
    Len(Place),
    /// `__builtin_va_arg(ap, type)` — extract one variadic argument.
    BuiltinVaArg {
        /// va_list operand.
        ap: Operand,
        /// Type of the value to extract.
        ty: TyId,
    },
}

/// Cast kinds recognised by the backend.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CastKind {
    /// Integer <-> integer (trunc / zext / sext depending on signedness).
    IntToInt,
    /// Integer <-> float.
    IntToFloat,
    /// Float <-> integer.
    FloatToInt,
    /// Float <-> float.
    FloatToFloat,
    /// Pointer <-> pointer (bitcast / addrspacecast).
    PtrToPtr,
    /// Pointer to integer (inttoptr inverse).
    PtrToInt,
    /// Integer to pointer.
    IntToPtr,
}

/// Binary op for the CFG (post type-checking; concrete semantics known).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// signed `/`
    SDiv,
    /// unsigned `/`
    UDiv,
    /// signed `%`
    SRem,
    /// unsigned `%`
    URem,
    /// `/` on float
    FDiv,
    /// `<<`
    Shl,
    /// arithmetic `>>`
    AShr,
    /// logical `>>`
    LShr,
    /// `&`
    BitAnd,
    /// `^`
    BitXor,
    /// `|`
    BitOr,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// signed `<`
    SLt,
    /// signed `<=`
    SLe,
    /// signed `>`
    SGt,
    /// signed `>=`
    SGe,
    /// unsigned `<`
    ULt,
    /// unsigned `<=`
    ULe,
    /// unsigned `>`
    UGt,
    /// unsigned `>=`
    UGe,
    /// float `<`
    FLt,
    /// float `<=`
    FLe,
    /// float `>`
    FGt,
    /// float `>=`
    FGe,
    /// float `+`
    FAdd,
    /// float `-`
    FSub,
    /// float `*`
    FMul,
    /// Pointer + integer.
    PtrAdd,
    /// Pointer - integer.
    PtrSub,
    /// Pointer - pointer (yields `ptrdiff_t`).
    PtrDiff,
}

/// Unary op.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// Integer `-` (two's complement negate).
    Neg,
    /// Float `-`.
    FNeg,
    /// Bitwise `~`.
    BitNot,
    /// Logical `!`.
    LogNot,
}
