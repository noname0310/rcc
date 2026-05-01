//! `rcc_hir`: the High-level IR for the rcc C compiler.
//!
//! Analogous to `rustc_hir`. Lives between AST (syntax-shaped) and the CFG
//! (control-flow-shaped). Every name is resolved to a `DefId` or `Local`,
//! every expression carries a resolved `Ty`, and the declarator chain is
//! already turned into a `Ty`.

#![forbid(unsafe_code)]
// Variants are documented at the enum level; docs on individual inline
// struct fields would be noise.
#![allow(missing_docs)]

use rcc_data_structures::{FxHashMap, IndexVec};
use rcc_span::{Span, Symbol};

pub mod layout;
pub mod ty;

pub use layout::{LayoutCx, LayoutError, LayoutResult};
pub use ty::{FloatKind, IntRank, Layout, Qual, Ty, TyCtxt, TyId};

rcc_data_structures::new_index! {
    /// HIR node id (per-body).
    pub struct HirId = u32;
}

rcc_data_structures::new_index! {
    /// Identifier for a top-level definition: function, static variable,
    /// typedef, struct/union/enum tag.
    pub struct DefId = u32;
}

rcc_data_structures::new_index! {
    /// Function-scoped local id (parameters + locals).
    pub struct Local = u32;
}

/// A fully lowered crate / translation unit.
#[derive(Debug, Default)]
pub struct HirCrate {
    /// Every top-level definition in declaration order.
    pub defs: IndexVec<DefId, Def>,
    /// Function bodies, keyed by the `DefId` of the enclosing function.
    pub bodies: FxHashMap<DefId, Body>,
    /// File-scope initializer expression bodies, keyed by the initialized
    /// global's `DefId`.
    pub global_init_bodies: FxHashMap<DefId, Body>,
}

/// One top-level definition.
#[derive(Debug, Clone)]
pub struct Def {
    /// Id.
    pub id: DefId,
    /// Declared name.
    pub name: Symbol,
    /// Definition span.
    pub span: Span,
    /// Kind.
    pub kind: DefKind,
}

/// Top-level qualifiers attached to a declared object rather than to one of
/// its component types.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectQuals {
    /// `const`
    pub is_const: bool,
    /// `volatile`
    pub is_volatile: bool,
    /// `restrict`
    pub is_restrict: bool,
}

impl ObjectQuals {
    /// No object-level qualifiers.
    pub fn none() -> Self {
        Self::default()
    }
}

/// Flavour of a top-level definition.
#[derive(Debug, Clone)]
pub enum DefKind {
    /// Function (definition or prototype).
    Function {
        /// Function type (signature).
        ty: TyId,
        /// Whether the function has a body.
        has_body: bool,
        /// `static`?
        is_static: bool,
        /// `inline`?
        is_inline: bool,
        /// `extern inline`? Distinguishes the C99 §6.7.4 case where an
        /// `inline` definition is also explicitly `extern`, which provides
        /// the external definition (vs. plain `inline`, which does not).
        is_extern_inline: bool,
        /// Variadic?
        variadic: bool,
    },
    /// Global variable (file-scope object).
    Global {
        /// Object type.
        ty: TyId,
        /// Qualifiers that apply to the global object itself.
        quals: ObjectQuals,
        /// Linkage kind.
        linkage: Linkage,
        /// Lowered static initializer, if this file-scope object has one.
        init: Option<GlobalInit>,
    },
    /// `typedef` alias.
    Typedef(TyId),
    /// `struct S { ... }`
    Record {
        /// Struct or union?
        kind: RecordKind,
        /// Resolved layout (filled after type checking).
        layout: Option<Layout>,
        /// Fields in declaration order.
        fields: Vec<Field>,
    },
    /// `enum E { ... }`
    Enum {
        /// Underlying integer type.
        repr: TyId,
        /// Enumerators in declaration order.
        variants: Vec<Enumerator>,
    },
    /// One enumerator entry (C99 §6.4.4.3). Enumerators live in the
    /// ordinary namespace so each constant gets its own `DefId` pointing
    /// at this variant; the parent `enum` definition is recorded
    /// separately as `DefKind::Enum`.
    Enumerator {
        /// Integer type assigned to the constant (M4: always `int`).
        ty: TyId,
        /// Folded constant value.
        value: i128,
    },
}

/// Struct vs union.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// `struct`
    Struct,
    /// `union`
    Union,
}

/// One field of a struct/union.
#[derive(Debug, Clone)]
pub struct Field {
    /// Field name (may be empty for anonymous bitfields).
    pub name: Option<Symbol>,
    /// Field type.
    pub ty: TyId,
    /// Qualifiers that apply to the field object itself.
    pub quals: ObjectQuals,
    /// Offset within the record, in bytes (filled by layout).
    pub offset: Option<u64>,
    /// Bitfield width, if applicable.
    pub bit_width: Option<u32>,
    /// Definition span.
    pub span: Span,
}

/// A single `enum` enumerator.
#[derive(Debug, Clone)]
pub struct Enumerator {
    /// Enumerator name.
    pub name: Symbol,
    /// Computed integer value.
    pub value: i128,
    /// Span.
    pub span: Span,
}

/// C linkage classification (C99 §6.2.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Linkage {
    /// `static` at file scope.
    Internal,
    /// Default for `extern` / un-qualified file-scope objects.
    External,
    /// Block-scope locals.
    None,
}

/// Lowered initializer payload for a file-scope object.
#[derive(Debug, Clone)]
pub struct GlobalInit {
    /// Object type after incomplete array completion.
    pub ty: TyId,
    /// Flattened initializer leaves in evaluation order.
    pub entries: Vec<GlobalInitEntry>,
}

/// One leaf in a flattened global initializer.
#[derive(Debug, Clone)]
pub struct GlobalInitEntry {
    /// Designator path from the root object to this leaf.
    pub path: Vec<GlobalInitDesignator>,
    /// Leaf type after aggregate designator descent.
    pub ty: TyId,
    /// HIR expression for this leaf, stored in `HirCrate::global_init_bodies`.
    ///
    /// Synthetic byte entries produced from `char[] = "..."` have no source
    /// expression and keep their lowered integer payload directly in `value`.
    pub expr: Option<HirExprId>,
    /// Leaf value.
    pub value: GlobalInitValue,
    /// Source span of the initializer leaf.
    pub span: Span,
}

/// A single component selector in a global initializer path.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GlobalInitDesignator {
    /// Array element index.
    Index(u64),
    /// Struct or union field index.
    Field(u32),
}

/// Constant-ish leaf value captured before codegen emits data.
#[derive(Clone, Debug)]
pub enum GlobalInitValue {
    /// Integer constant.
    Int(i128),
    /// Floating constant.
    Float(f64),
    /// Relocatable address constant.
    Address {
        /// Base definition, or `None` for a null-base pointer constant.
        def: Option<DefId>,
        /// Byte offset from the base.
        offset: i128,
    },
    /// String literal global.
    StringLiteral(DefId),
    /// Zero-fill marker for a scalar leaf.
    Zero,
    /// Malformed or not-yet-foldable initializer leaf.
    Error,
}

/// A function body: locals + statements that will be lowered to CFG.
#[derive(Debug, Default, Clone)]
pub struct Body {
    /// Every local in declaration order: `locals[0]` is the implicit return slot.
    pub locals: IndexVec<Local, LocalDecl>,
    /// Root statement (usually a compound statement).
    pub root: Option<HirStmtId>,
    /// Arena of statements.
    pub stmts: IndexVec<HirStmtId, HirStmt>,
    /// Arena of expressions.
    pub exprs: IndexVec<HirExprId, HirExpr>,
}

rcc_data_structures::new_index! {
    /// Id of an `HirStmt` inside a `Body`.
    pub struct HirStmtId = u32;
}

rcc_data_structures::new_index! {
    /// Id of an `HirExpr` inside a `Body`.
    pub struct HirExprId = u32;
}

/// One local variable (parameter or declared local).
#[derive(Debug, Clone)]
pub struct LocalDecl {
    /// Source name, or `None` for compiler-generated temporaries.
    pub name: Option<Symbol>,
    /// Resolved type.
    pub ty: TyId,
    /// Qualifiers that apply to the local object itself.
    pub quals: ObjectQuals,
    /// Runtime bound expression for a block-scope VLA local, when this
    /// declaration owns a dynamic array allocation.
    pub vla_len: Option<HirExprId>,
    /// Whether this local is a function parameter.
    pub is_param: bool,
    /// Declaration span.
    pub span: Span,
}

/// A typed HIR statement.
#[derive(Debug, Clone)]
pub struct HirStmt {
    /// Id in the body.
    pub id: HirStmtId,
    /// Source span.
    pub span: Span,
    /// Kind.
    pub kind: HirStmtKind,
}

/// HIR statement kind. Mirrors `rcc_ast::StmtKind` but with resolved ids.
#[derive(Debug, Clone)]
pub enum HirStmtKind {
    /// `{ ... }` — statement list (locals are declared in `Body::locals`).
    Block(Vec<HirStmtId>),
    /// Expression statement.
    Expr(HirExprId),
    /// `if (cond) then else?`
    If { cond: HirExprId, then_branch: HirStmtId, else_branch: Option<HirStmtId> },
    /// `while (cond) body`
    While { cond: HirExprId, body: HirStmtId },
    /// `do body while (cond);`
    DoWhile { body: HirStmtId, cond: HirExprId },
    /// `for (init?; cond?; step?) body`
    For {
        /// Optional initializer (expression or declaration-bound assignment).
        init: Option<HirStmtId>,
        /// Optional loop condition.
        cond: Option<HirExprId>,
        /// Optional step expression.
        step: Option<HirExprId>,
        /// Loop body.
        body: HirStmtId,
    },
    /// `switch (cond) body`
    Switch { cond: HirExprId, body: HirStmtId, cases: Vec<SwitchCase> },
    /// Unresolved label; target resolution in `rcc_cfg`.
    Label { name: Symbol, body: HirStmtId },
    /// `case expr: stmt` inside a switch. The case value is a folded
    /// integer constant; HIR lowering's switch-collection pass rewrites
    /// these into the enclosing `Switch::cases` table.
    Case { value: Option<i128>, body: HirStmtId },
    /// `default: stmt` inside a switch. Same rewrite as `Case`.
    Default { body: HirStmtId },
    /// `goto label;`
    Goto(Symbol),
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `return expr?;`
    Return(Option<HirExprId>),
    /// Local declaration with optional initializer.
    LocalDecl { local: Local, init: Option<HirExprId> },
    /// `;`
    Null,
}

/// One entry of a switch statement's case/default table.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// `Some(value)` for `case`, `None` for `default`.
    pub value: Option<i128>,
    /// Statement to jump to.
    pub target: HirStmtId,
}

/// A typed HIR expression.
#[derive(Debug, Clone)]
pub struct HirExpr {
    /// Id in the body.
    pub id: HirExprId,
    /// Resolved type.
    pub ty: TyId,
    /// lvalue / rvalue category.
    pub value_cat: ValueCat,
    /// Source span.
    pub span: Span,
    /// Kind.
    pub kind: HirExprKind,
}

/// Value category (C99 §6.3.2.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValueCat {
    /// lvalue (has an address).
    LValue,
    /// rvalue (value).
    RValue,
}

/// HIR expression kinds.
#[derive(Debug, Clone)]
pub enum HirExprKind {
    /// Integer constant.
    IntConst(i128),
    /// Float constant.
    FloatConst(f64),
    /// String-literal reference into a global table.
    StringRef(DefId),
    /// Reference to a local.
    LocalRef(Local),
    /// Reference to a top-level definition (function, global).
    DefRef(DefId),
    /// Binary op (type-resolved).
    Binary { op: rcc_hir_binop::BinOp, lhs: HirExprId, rhs: HirExprId },
    /// Unary op.
    Unary { op: rcc_hir_binop::UnOp, operand: HirExprId },
    /// Call.
    Call { callee: HirExprId, args: Vec<HirExprId> },
    /// Unresolved source member access (`s.f` / `p->f`) before typeck has
    /// resolved the requested name to a concrete record field index.
    ///
    /// HIR lowering emits this lossless form so the member `field` symbol is
    /// still available to `rcc_typeck`. The type checker rewrites successful
    /// lookups to [`HirExprKind::Field`], which is the resolved projection form
    /// consumed by CFG and LLVM codegen.
    UnresolvedField {
        /// Base record expression. For `p->f`, lowering first inserts a
        /// [`HirExprKind::Deref`] node and stores that id here.
        base: HirExprId,
        /// Requested member name.
        field: Symbol,
        /// Best available source span for the member token.
        field_span: Span,
    },
    /// Resolved field access (record + index).
    Field { base: HirExprId, field_index: u32 },
    /// Array/pointer index, lowered to `*(base + index)`.
    Index { base: HirExprId, index: HirExprId },
    /// Array-to-pointer / function-to-pointer / lvalue-to-rvalue conversion.
    Convert { operand: HirExprId, kind: ConvertKind },
    /// Explicit cast `(ty)expr`.
    Cast {
        /// Operand being cast.
        operand: HirExprId,
        /// Destination type.
        to: TyId,
    },
    /// `sizeof expr`.
    SizeofExpr(HirExprId),
    /// `sizeof(type-name)`.
    SizeofType(TyId),
    /// C99 compound literal `(type-name){ initializer-list }`.
    ///
    /// The type part is preserved here; storage materialisation is handled by
    /// the HIR-lower follow-up that lowers compound literal initializers.
    CompoundLiteral {
        /// Object type named by the compound literal.
        ty: TyId,
        /// Synthetic automatic-storage local backing the lvalue object.
        local: Local,
        /// Initializer statements to execute when the compound literal is
        /// evaluated.
        init_stmts: Vec<HirStmtId>,
    },
    /// `&expr`
    AddressOf(HirExprId),
    /// `*expr`
    Deref(HirExprId),
    /// `a ? b : c`
    Cond {
        /// Controlling expression.
        cond: HirExprId,
        /// Value when `cond` is non-zero.
        then_expr: HirExprId,
        /// Value when `cond` is zero.
        else_expr: HirExprId,
    },
    /// `,`
    Comma {
        /// Left operand (evaluated, discarded).
        lhs: HirExprId,
        /// Right operand (the comma expression's value).
        rhs: HirExprId,
    },
    /// `a = b` (simple assign; compound forms are desugared).
    Assign {
        /// Destination lvalue.
        lhs: HirExprId,
        /// Value.
        rhs: HirExprId,
    },
}

/// Kinds of implicit conversion inserted during type checking.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConvertKind {
    /// Integer promotion.
    IntegerPromotion,
    /// Usual arithmetic conversion to a common type.
    UsualArithmetic,
    /// Array-to-pointer decay.
    ArrayToPtr,
    /// Function-to-pointer decay.
    FuncToPtr,
    /// lvalue-to-rvalue conversion (C99 §6.3.2.1).
    LvalueToRvalue,
    /// Pointer conversion to `void*` or between compatible pointer types.
    Pointer,
    /// Real-to-complex conversion (C99 §6.3.1.6): the real value becomes
    /// the real part, the imaginary part is zero.
    RealToComplex,
    /// Complex-to-real conversion (C99 §6.3.1.6): the imaginary part is
    /// discarded. The type-checker emits W0012 at the conversion site.
    ComplexToReal,
}

/// Small nested module so `HirExprKind` names don't collide with `rcc_ast::BinOp`.
pub mod rcc_hir_binop {
    /// HIR-level binary operator. Same semantics as `rcc_ast::BinOp`; re-declared
    /// to keep `rcc_hir` free of a dependency on `rcc_ast`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub enum BinOp {
        /// `+`
        Add,
        /// `-`
        Sub,
        /// `*`
        Mul,
        /// `/`
        Div,
        /// `%`
        Rem,
        /// `<<`
        Shl,
        /// `>>`
        Shr,
        /// `<`
        Lt,
        /// `<=`
        Le,
        /// `>`
        Gt,
        /// `>=`
        Ge,
        /// `==`
        Eq,
        /// `!=`
        Ne,
        /// `&`
        BitAnd,
        /// `^`
        BitXor,
        /// `|`
        BitOr,
        /// `&&`
        LogAnd,
        /// `||`
        LogOr,
    }

    /// HIR-level unary operator.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub enum UnOp {
        /// `+`
        Plus,
        /// `-`
        Neg,
        /// `~`
        BitNot,
        /// `!`
        LogNot,
        /// `++x`
        PreInc,
        /// `--x`
        PreDec,
        /// `x++`
        PostInc,
        /// `x--`
        PostDec,
    }
}
