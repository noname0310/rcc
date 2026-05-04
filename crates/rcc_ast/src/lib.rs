//! `rcc_ast`: concrete-ish Abstract Syntax Tree for C99.
//!
//! Analogous to `rustc_ast`. Kept *close to the surface syntax* — typedef
//! names are still just identifiers, declarators are nested as written, and
//! no name resolution has happened. `rcc_hir_lower` turns this into HIR.

#![forbid(unsafe_code)]
// Enum-level doc comments cover every variant; individual inline struct
// fields inside variants are self-explanatory and docs would be noise.
#![allow(missing_docs)]

use rcc_span::{Span, Symbol};

pub mod pretty;
pub mod visit;

rcc_data_structures::new_index! {
    /// AST node id, unique per translation unit.
    pub struct NodeId = u32;
}

/// One parsed translation unit.
#[derive(Debug, Clone)]
pub struct TranslationUnit {
    /// Top-level external declarations.
    pub decls: Vec<ExternalDecl>,
    /// Span of the whole unit (usually the whole file).
    pub span: Span,
}

/// `external-declaration` (C99 §6.9).
#[derive(Debug, Clone)]
pub enum ExternalDecl {
    /// Function definition.
    Function(FunctionDef),
    /// A declaration list (typedef / extern / static / variable / tag).
    Decl(Decl),
}

/// A `declaration` (one `declaration-specifiers init-declarator-list ;`).
#[derive(Debug, Clone)]
pub struct Decl {
    /// Node id.
    pub id: NodeId,
    /// Span.
    pub span: Span,
    /// Declaration specifiers (storage class + type specifiers + qualifiers).
    pub specs: DeclSpecs,
    /// Init declarator list.
    pub inits: Vec<InitDeclarator>,
}

/// An `init-declarator`.
#[derive(Debug, Clone)]
pub struct InitDeclarator {
    /// Declarator part.
    pub declarator: Declarator,
    /// Optional initializer.
    pub init: Option<Initializer>,
}

/// A function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef {
    /// Node id.
    pub id: NodeId,
    /// Span.
    pub span: Span,
    /// Declaration specifiers.
    pub specs: DeclSpecs,
    /// Declarator naming the function.
    pub declarator: Declarator,
    /// K&R-style declaration list (C99 §6.9.1 old-style decls).
    pub kr_decls: Vec<Decl>,
    /// Function body (compound statement).
    pub body: Block,
}

/// Declaration specifiers.
#[derive(Debug, Clone)]
pub struct DeclSpecs {
    /// Span of the specifier list.
    pub span: Span,
    /// Storage class specifier (at most one).
    pub storage: Option<StorageClass>,
    /// Type specifiers in declaration order (combined later by `rcc_hir_lower`).
    pub type_specs: Vec<TypeSpec>,
    /// Type qualifiers (may repeat; deduped in lowering).
    pub quals: TypeQuals,
    /// Function specifier(s), e.g. `inline`.
    pub func_specs: FuncSpecs,
    /// GNU attributes written in declaration-specifier position.
    pub attrs: Vec<Attribute>,
}

/// Storage class keywords.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageClass {
    /// `typedef`
    Typedef,
    /// `extern`
    Extern,
    /// `static`
    Static,
    /// `auto`
    Auto,
    /// `register`
    Register,
}

/// Bitset of type qualifiers.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TypeQuals {
    /// `const`
    pub const_: bool,
    /// `volatile`
    pub volatile: bool,
    /// `restrict` (C99).
    pub restrict: bool,
}

/// Bitset of function specifiers (C99).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FuncSpecs {
    /// `inline`
    pub inline: bool,
}

/// A single type specifier keyword / tag reference.
#[derive(Debug, Clone)]
pub enum TypeSpec {
    /// `void`
    Void,
    /// `char`
    Char,
    /// `short`
    Short,
    /// `int`
    Int,
    /// `long`
    Long,
    /// `float`
    Float,
    /// `double`
    Double,
    /// `signed`
    Signed,
    /// `unsigned`
    Unsigned,
    /// `_Bool` (C99).
    Bool,
    /// `_Complex` (C99).
    Complex,
    /// `_Imaginary` (C99).
    Imaginary,
    /// Reference to a previously seen `typedef-name`.
    TypedefName(Symbol),
    /// Struct or union specifier, possibly defining fields.
    Record(RecordSpec),
    /// Enum specifier.
    Enum(EnumSpec),
    /// `__builtin_va_list`
    BuiltinVaList,
    /// GNU `typeof (expression)`.
    TypeofExpr(Box<Expr>),
    /// GNU `typeof (type-name)`.
    TypeofType(Box<TypeName>),
}

/// `struct`/`union` specifier.
#[derive(Debug, Clone)]
pub struct RecordSpec {
    /// Node id.
    pub id: NodeId,
    /// Kind: struct or union.
    pub kind: RecordKind,
    /// Optional tag name.
    pub tag: Option<Symbol>,
    /// `Some` when the specifier defines fields; `None` for a bare tag reference.
    pub fields: Option<Vec<FieldDecl>>,
    /// Span.
    pub span: Span,
    /// GNU attributes attached to the record specifier.
    pub attrs: Vec<Attribute>,
}

/// `struct` vs `union`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// `struct`
    Struct,
    /// `union`
    Union,
}

/// A single field in a struct/union.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// Shared specifiers for this group of declarators.
    pub specs: DeclSpecs,
    /// Field declarators (may include bitfield width).
    pub declarators: Vec<FieldDeclarator>,
    /// Full span.
    pub span: Span,
}

/// A field declarator with optional bitfield width.
#[derive(Debug, Clone)]
pub struct FieldDeclarator {
    /// Declarator; `None` for anonymous bitfields.
    pub declarator: Option<Declarator>,
    /// Bitfield width expression, if any.
    pub bit_width: Option<Expr>,
}

/// `enum` specifier.
#[derive(Debug, Clone)]
pub struct EnumSpec {
    /// Node id.
    pub id: NodeId,
    /// Tag name.
    pub tag: Option<Symbol>,
    /// `Some` when defining enumerators.
    pub enumerators: Option<Vec<Enumerator>>,
    /// Span.
    pub span: Span,
    /// GNU attributes attached to the enum specifier.
    pub attrs: Vec<Attribute>,
}

/// A single `NAME [= expr]` enumerator.
#[derive(Debug, Clone)]
pub struct Enumerator {
    /// Enumerator name.
    pub name: Symbol,
    /// Optional explicit value.
    pub value: Option<Expr>,
    /// Full span.
    pub span: Span,
    /// GNU attributes attached to this enumerator.
    pub attrs: Vec<Attribute>,
}

/// A declarator: name + derived-declarator chain.
#[derive(Debug, Clone)]
pub struct Declarator {
    /// Identifier being declared, or `None` for abstract declarators
    /// (used in type names and function parameters).
    pub name: Option<(Symbol, Span)>,
    /// Outermost-to-innermost chain of derivations: pointer / array / function.
    pub derived: Vec<DerivedDeclarator>,
    /// Full span.
    pub span: Span,
    /// GNU attributes attached to this declarator.
    pub attrs: Vec<Attribute>,
}

/// GNU-style attribute payload.
#[derive(Debug, Clone)]
pub struct Attribute {
    /// Attribute name, e.g. `packed`, `aligned`, `section`.
    pub name: Symbol,
    /// Comma-separated argument payloads inside `name(...)`.
    pub args: Vec<AttributeArg>,
    /// Full span covering the attribute item.
    pub span: Span,
}

/// One comma-delimited attribute argument.
#[derive(Debug, Clone)]
pub struct AttributeArg {
    /// Raw parser-level tokens preserved for phase-14 semantic checks.
    pub tokens: Vec<AttributeToken>,
    /// Span of this argument.
    pub span: Span,
}

/// Parser-level token preserved inside an attribute argument.
#[derive(Debug, Clone)]
pub struct AttributeToken {
    /// Token payload.
    pub kind: AttributeTokenKind,
    /// Source span.
    pub span: Span,
}

/// Token categories preserved for attribute arguments.
#[derive(Debug, Clone)]
pub enum AttributeTokenKind {
    /// Identifier or keyword spelling.
    Symbol(Symbol),
    /// Integer literal value.
    Int(u128),
    /// Floating literal value.
    Float(f64),
    /// Character literal value.
    Char(u32),
    /// String-literal bytes after phase-7 decoding.
    String(Vec<u8>),
    /// Punctuator spelling, interned in the session.
    Punct(Symbol),
}

/// GNU inline assembly statement payload.
#[derive(Debug, Clone)]
pub struct InlineAsm {
    /// Qualifiers written between `asm` and the template.
    pub quals: InlineAsmQuals,
    /// Assembly template string.
    pub template: StringLiteral,
    /// Output operands in extended asm.
    pub outputs: Vec<InlineAsmOperand>,
    /// Input operands in extended asm.
    pub inputs: Vec<InlineAsmOperand>,
    /// Clobber strings in extended asm.
    pub clobbers: Vec<StringLiteral>,
    /// Full statement span.
    pub span: Span,
}

/// GNU inline assembly qualifiers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineAsmQuals {
    /// `volatile` / `__volatile__`.
    pub volatile: bool,
    /// `inline` / `__inline__`.
    pub inline: bool,
    /// `goto` / `__goto__`.
    pub goto: bool,
}

/// One GNU inline assembly operand.
#[derive(Debug, Clone)]
pub struct InlineAsmOperand {
    /// Optional symbolic operand name from `[name]`.
    pub name: Option<(Symbol, Span)>,
    /// Constraint string.
    pub constraint: StringLiteral,
    /// Parenthesised operand expression.
    pub expr: Expr,
    /// Full operand span.
    pub span: Span,
}

/// One step in a declarator's derivation chain.
#[derive(Debug, Clone)]
pub enum DerivedDeclarator {
    /// `* qualifiers`
    Pointer(TypeQuals),
    /// `[size]` with optional qualifiers / `static` / `*`.
    Array(ArrayDeclarator),
    /// `(params)` function declarator.
    Function(FunctionDeclarator),
}

/// Array declarator details. Supports C99 `[static 10]`, `[*]`, VLA.
#[derive(Debug, Clone)]
pub struct ArrayDeclarator {
    /// Qualifiers inside `[...]`.
    pub quals: TypeQuals,
    /// `static` inside `[...]` (C99).
    pub has_static: bool,
    /// `[*]` (VLA of unspecified size, C99).
    pub star: bool,
    /// Explicit size expression.
    pub size: Option<Expr>,
}

/// Function declarator details.
#[derive(Debug, Clone)]
pub struct FunctionDeclarator {
    /// Parameters. Empty vec == `(void)` only when `is_void` is `true`;
    /// a truly empty `()` is represented by `params.is_empty() && !is_void`.
    pub params: Vec<ParamDecl>,
    /// Whether `(void)` was written.
    pub is_void: bool,
    /// Whether the parameter list ends with `...`.
    pub variadic: bool,
    /// K&R identifier list (old-style). Mutually exclusive with `params`.
    pub kr_names: Vec<(Symbol, Span)>,
}

/// A function parameter.
#[derive(Debug, Clone)]
pub struct ParamDecl {
    /// Declaration specifiers.
    pub specs: DeclSpecs,
    /// Declarator (name may be absent in prototypes).
    pub declarator: Declarator,
    /// Full span.
    pub span: Span,
}

/// `type-name` (used in `sizeof`, casts, compound literals).
#[derive(Debug, Clone)]
pub struct TypeName {
    /// Specifiers + qualifiers.
    pub specs: DeclSpecs,
    /// Abstract declarator.
    pub declarator: Declarator,
    /// Span.
    pub span: Span,
}

/// An initializer clause.
#[derive(Debug, Clone)]
pub enum Initializer {
    /// Single expression.
    Expr(Expr),
    /// Brace-enclosed initializer list. Each element may carry designators.
    List(Vec<(Vec<Designator>, Initializer)>),
}

/// A designator (C99).
#[derive(Debug, Clone)]
pub enum Designator {
    /// `.name`
    Field(Symbol),
    /// `[expr]`
    Index(Expr),
    /// GNU `[lo ... hi]` initializer range designator.
    Range { lo: Box<Expr>, hi: Box<Expr> },
}

/// Member-designator component accepted by `__builtin_offsetof`.
#[derive(Debug, Clone)]
pub enum OffsetofDesignator {
    /// `.field` or the first unprefixed `field`.
    Field(Symbol),
    /// `[expr]`.
    Index(Box<Expr>),
}

/// Compound statement / block.
#[derive(Debug, Clone)]
pub struct Block {
    /// Node id.
    pub id: NodeId,
    /// Items in order (declarations are legal mid-block in C99).
    pub items: Vec<BlockItem>,
    /// Span of the `{ ... }`.
    pub span: Span,
}

/// Block item (C99 allows declarations interleaved with statements).
///
/// `Stmt` is boxed because `Stmt` is substantially larger than `Decl`
/// (statement kinds recursively embed expressions and sub-statements),
/// so keeping it inline would bloat every `BlockItem` and every
/// `Vec<BlockItem>` element. Boxing balances the enum and silences
/// `clippy::large_enum_variant` without changing AST semantics.
#[derive(Debug, Clone)]
pub enum BlockItem {
    /// A declaration.
    Decl(Decl),
    /// A statement.
    Stmt(Box<Stmt>),
}

/// A statement.
#[derive(Debug, Clone)]
pub struct Stmt {
    /// Node id.
    pub id: NodeId,
    /// Kind.
    pub kind: StmtKind,
    /// Span.
    pub span: Span,
}

/// Statement discriminant.
#[derive(Debug, Clone)]
pub enum StmtKind {
    /// Expression statement.
    Expr(Option<Expr>),
    /// `{ ... }`
    Compound(Block),
    /// `if (cond) then else?`
    If { cond: Expr, then_branch: Box<Stmt>, else_branch: Option<Box<Stmt>> },
    /// `while (cond) body`
    While { cond: Expr, body: Box<Stmt> },
    /// `do body while (cond);`
    DoWhile { body: Box<Stmt>, cond: Expr },
    /// `for (init?; cond?; step?) body`.
    For {
        /// `init` may be an expression statement OR a declaration (C99).
        init: Option<Box<BlockItem>>,
        /// Loop condition.
        cond: Option<Box<Expr>>,
        /// Loop step.
        step: Option<Box<Expr>>,
        /// Body.
        body: Box<Stmt>,
    },
    /// `switch (cond) body`
    Switch { cond: Expr, body: Box<Stmt> },
    /// `case expr: stmt`, or GNU `case lo ... hi: stmt`.
    Case { value: Expr, range_end: Option<Expr>, body: Box<Stmt> },
    /// `default: stmt`
    Default { body: Box<Stmt> },
    /// GNU attributes attached to a following statement.
    Attributed { attrs: Vec<Attribute>, stmt: Box<Stmt> },
    /// GNU inline assembly statement.
    InlineAsm(InlineAsm),
    /// `label: stmt`
    Label { name: Symbol, body: Box<Stmt> },
    /// `goto label;`
    Goto(Symbol),
    /// GNU computed goto: `goto *expr;`
    GotoComputed(Expr),
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `return expr?;`
    Return(Option<Expr>),
    /// `;`
    Null,
}

/// An expression.
#[derive(Debug, Clone)]
pub struct Expr {
    /// Node id.
    pub id: NodeId,
    /// Kind.
    pub kind: ExprKind,
    /// Span.
    pub span: Span,
}

/// Decoded integer literal payload carried by the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntLiteral {
    /// Original source spelling, retained for diagnostics and display.
    pub text: Symbol,
    /// Numeric value decoded during parser phase 7.
    pub value: u128,
    /// Literal base spelling.
    pub base: IntBase,
    /// Literal suffix.
    pub suffix: IntSuffix,
}

/// Integer-literal base spelling.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntBase {
    /// Decimal constant.
    Decimal,
    /// Octal constant.
    Octal,
    /// Hexadecimal constant.
    Hex,
    /// GNU binary constant (`0b...` / `0B...`).
    Binary,
}

/// Integer-literal suffix.
///
/// Variant spellings mirror the C source suffix set.
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntSuffix {
    /// No suffix.
    None,
    /// `u`/`U`.
    U,
    /// `l`/`L`.
    L,
    /// `ul`/`uL`/...
    UL,
    /// `ll`/`LL`.
    LL,
    /// `ull`/`uLL`.
    ULL,
}

/// Decoded floating literal payload carried by the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct FloatLiteral {
    /// Original source spelling, retained for diagnostics and display.
    pub text: Symbol,
    /// Parsed value.
    pub value: f64,
    /// Literal suffix.
    pub suffix: FloatSuffix,
}

/// Floating-literal suffix.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FloatSuffix {
    /// No suffix, i.e. `double`.
    None,
    /// `f`/`F`, i.e. `float`.
    F,
    /// `l`/`L`, i.e. `long double`.
    L,
}

/// Literal source encoding.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LiteralEncoding {
    /// No prefix.
    None,
    /// `u8`.
    Utf8,
    /// `u`.
    Utf16,
    /// `U`.
    Utf32,
    /// `L`.
    Wide,
}

/// Decoded character literal payload carried by the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharLiteral {
    /// Original source spelling, retained for diagnostics and display.
    pub text: Symbol,
    /// Decoded code-point value.
    pub value: u32,
    /// Literal encoding prefix.
    pub encoding: LiteralEncoding,
}

/// Decoded string literal payload carried by the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral {
    /// Original source spelling, retained for diagnostics and display.
    pub text: Symbol,
    /// Decoded bytes, without the trailing C NUL.
    pub bytes: Vec<u8>,
    /// Literal encoding prefix after adjacent string concatenation.
    pub encoding: LiteralEncoding,
}

/// Expression discriminant.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Identifier reference (resolved later).
    Ident(Symbol),
    /// Integer literal.
    IntLit(IntLiteral),
    /// Floating literal.
    FloatLit(FloatLiteral),
    /// Character constant.
    CharLit(CharLiteral),
    /// String literal(s). Adjacent concatenation is already done.
    StringLit(StringLiteral),
    /// `a op b`
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// Prefix or postfix unary.
    Unary { op: UnOp, operand: Box<Expr> },
    /// `a ? b : c`
    Cond { cond: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr> },
    /// GNU `a ?: b` omitted-middle conditional.
    OmittedCond { cond: Box<Expr>, else_expr: Box<Expr> },
    /// GNU label address expression: `&&label`.
    LabelAddr(Symbol),
    /// `a = b`, `a += b`, ...
    Assign { op: AssignOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `,` operator.
    Comma { lhs: Box<Expr>, rhs: Box<Expr> },
    /// Function call.
    Call { callee: Box<Expr>, args: Vec<Expr> },
    /// `__builtin_offsetof(type-name, member-designator)`.
    BuiltinOffsetof {
        /// Type being queried.
        ty: Box<TypeName>,
        /// Member-designator path.
        designators: Vec<OffsetofDesignator>,
    },
    /// `__builtin_types_compatible_p(type-name, type-name)`.
    BuiltinTypesCompatible {
        /// Left type argument.
        lhs: Box<TypeName>,
        /// Right type argument.
        rhs: Box<TypeName>,
    },
    /// `__builtin_va_arg(va_list, type-name)`.
    BuiltinVaArg { ap: Box<Expr>, ty: Box<TypeName> },
    /// GNU C statement expression `({ block-item* })`.
    StmtExpr(Box<Block>),
    /// `a.b`
    Member { base: Box<Expr>, field: Symbol },
    /// `a->b`
    Arrow { base: Box<Expr>, field: Symbol },
    /// `a[b]`
    Index { base: Box<Expr>, index: Box<Expr> },
    /// `(type)expr`
    Cast { ty: TypeName, expr: Box<Expr> },
    /// `sizeof expr`
    SizeofExpr(Box<Expr>),
    /// `sizeof(type)`
    SizeofType(TypeName),
    /// GNU `__alignof__ expr`
    AlignofExpr(Box<Expr>),
    /// GNU `__alignof__(type)`
    AlignofType(TypeName),
    /// `(type){ init }`  -- C99 compound literal.
    CompoundLiteral {
        /// Type being initialised.
        ty: TypeName,
        /// Initializer (boxed to break the recursive size cycle).
        init: Box<Initializer>,
    },
    /// Parenthesised expression (preserved for span fidelity).
    Paren(Box<Expr>),
}

/// Binary operators (excluding `,` and assignment forms).
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

/// Unary operators (prefix / postfix).
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
    /// `*`
    Deref,
    /// `&`
    AddrOf,
    /// `++x`
    PreInc,
    /// `--x`
    PreDec,
    /// `x++`
    PostInc,
    /// `x--`
    PostDec,
}

/// Assignment operators.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssignOp {
    /// `=`
    Eq,
    /// `+=`
    AddEq,
    /// `-=`
    SubEq,
    /// `*=`
    MulEq,
    /// `/=`
    DivEq,
    /// `%=`
    RemEq,
    /// `<<=`
    ShlEq,
    /// `>>=`
    ShrEq,
    /// `&=`
    AndEq,
    /// `^=`
    XorEq,
    /// `|=`
    OrEq,
}

impl Default for DeclSpecs {
    fn default() -> Self {
        Self {
            span: rcc_span::DUMMY_SP,
            storage: None,
            type_specs: Vec::new(),
            quals: TypeQuals::default(),
            func_specs: FuncSpecs::default(),
            attrs: Vec::new(),
        }
    }
}
