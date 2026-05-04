//! C99 type system and the `TyCtxt` interner.

use rcc_data_structures::FxHashMap;

use crate::DefId;

/// Interned type id (opaque; compare by `TyId`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TyId(pub u32);

/// A fully-resolved C99 type.
///
/// Kept flat (no boxed recursion) — pointer/array/function components hold
/// `TyId`s into the `TyCtxt`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    /// `void`
    Void,
    /// Integer types.
    Int {
        /// Whether the type is signed.
        signed: bool,
        /// Conversion rank (C99 §6.3.1.1).
        rank: IntRank,
    },
    /// `float` / `double` / `long double`.
    Float(FloatKind),
    /// `_Complex` variants (C99 §6.2.5p11).
    Complex(FloatKind),
    /// GNU fixed-size vector extension.
    Vector {
        /// Scalar element type.
        elem: TyId,
        /// Number of vector lanes.
        lanes: u32,
        /// Total vector object size in bytes.
        bytes: u64,
    },
    /// Pointer to qualified type.
    Ptr(Qual),
    /// Array of `elem`. `len` = `None` for incomplete or `[*]` VLA.
    Array {
        /// Element type (qualified).
        elem: Qual,
        /// Constant length (known at compile time), or `None` for VLA / incomplete.
        len: Option<u64>,
        /// Whether this is a VLA (runtime-sized).
        is_vla: bool,
    },
    /// Function type.
    Func {
        /// Return type (qualifiers never legal on return in C; retained for uniformity).
        ret: TyId,
        /// Parameter types. Empty => `(void)` or unspecified (distinguished by `proto`).
        params: Vec<TyId>,
        /// `...`
        variadic: bool,
        /// Whether the declaration used a prototype (empty params mean unspec if false).
        proto: bool,
    },
    /// Struct / union / enum reference by `DefId`.
    Record(DefId),
    /// Reference to an enum by `DefId`.
    Enum(DefId),
    /// Compiler-provided `__builtin_va_list` (SysV x86-64 baseline).
    BuiltinVaList,
    /// Error sentinel used during type checking to keep lowering lossy but alive.
    Error,
}

/// Integer rank category.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntRank {
    /// `_Bool`
    Bool,
    /// `char` (rank == `signed char` == `unsigned char`).
    Char,
    /// `short`
    Short,
    /// `int`
    Int,
    /// `long`
    Long,
    /// `long long`
    LongLong,
}

/// Float kind.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FloatKind {
    /// `float`
    F32,
    /// `double`
    F64,
    /// `long double` (platform-dependent; usually 80 or 128 bit).
    F80,
}

/// Qualified type: `TyId` + qualifier bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Qual {
    /// Underlying type.
    pub ty: TyId,
    /// `const`
    pub is_const: bool,
    /// `volatile`
    pub is_volatile: bool,
    /// `restrict` (only meaningful on pointers).
    pub is_restrict: bool,
}

impl Qual {
    /// Unqualified qualifier wrapper.
    pub fn plain(ty: TyId) -> Self {
        Self { ty, is_const: false, is_volatile: false, is_restrict: false }
    }
}

/// Computed layout for a type (via `rcc_hir::LayoutCx`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Size in bytes.
    pub size: u64,
    /// Required alignment in bytes.
    pub align: u32,
}

/// Type interner + pre-built common types.
pub struct TyCtxt {
    types: Vec<Ty>,
    dedup: FxHashMap<Ty, TyId>,

    // Common types cached for fast access:
    /// `void`
    pub void: TyId,
    /// `_Bool`
    pub bool_: TyId,
    /// `char`
    pub char_: TyId,
    /// `signed char`
    pub schar: TyId,
    /// `unsigned char`
    pub uchar: TyId,
    /// `short`
    pub short: TyId,
    /// `unsigned short`
    pub ushort: TyId,
    /// `int`
    pub int: TyId,
    /// `unsigned int`
    pub uint: TyId,
    /// `long`
    pub long: TyId,
    /// `unsigned long`
    pub ulong: TyId,
    /// `long long`
    pub long_long: TyId,
    /// `unsigned long long`
    pub ulong_long: TyId,
    /// `float`
    pub float: TyId,
    /// `double`
    pub double: TyId,
    /// `long double`
    pub long_double: TyId,
    /// `_Complex float`
    pub complex_float: TyId,
    /// `_Complex double`
    pub complex_double: TyId,
    /// `_Complex long double`
    pub complex_long_double: TyId,
    /// Error sentinel.
    pub error: TyId,
    /// `__builtin_va_list`
    pub builtin_va_list: TyId,
}

impl TyCtxt {
    /// Build a fresh context preloaded with scalar C99 types.
    pub fn new() -> Self {
        let mut this = Self {
            types: Vec::new(),
            dedup: FxHashMap::default(),
            void: TyId(0),
            bool_: TyId(0),
            char_: TyId(0),
            schar: TyId(0),
            uchar: TyId(0),
            short: TyId(0),
            ushort: TyId(0),
            int: TyId(0),
            uint: TyId(0),
            long: TyId(0),
            ulong: TyId(0),
            long_long: TyId(0),
            ulong_long: TyId(0),
            float: TyId(0),
            double: TyId(0),
            long_double: TyId(0),
            complex_float: TyId(0),
            complex_double: TyId(0),
            complex_long_double: TyId(0),
            error: TyId(0),
            builtin_va_list: TyId(0),
        };
        this.void = this.intern(Ty::Void);
        this.bool_ = this.intern(Ty::Int { signed: false, rank: IntRank::Bool });
        this.char_ = this.intern(Ty::Int { signed: true, rank: IntRank::Char });
        this.schar = this.intern(Ty::Int { signed: true, rank: IntRank::Char });
        this.uchar = this.intern(Ty::Int { signed: false, rank: IntRank::Char });
        this.short = this.intern(Ty::Int { signed: true, rank: IntRank::Short });
        this.ushort = this.intern(Ty::Int { signed: false, rank: IntRank::Short });
        this.int = this.intern(Ty::Int { signed: true, rank: IntRank::Int });
        this.uint = this.intern(Ty::Int { signed: false, rank: IntRank::Int });
        this.long = this.intern(Ty::Int { signed: true, rank: IntRank::Long });
        this.ulong = this.intern(Ty::Int { signed: false, rank: IntRank::Long });
        this.long_long = this.intern(Ty::Int { signed: true, rank: IntRank::LongLong });
        this.ulong_long = this.intern(Ty::Int { signed: false, rank: IntRank::LongLong });
        this.float = this.intern(Ty::Float(FloatKind::F32));
        this.double = this.intern(Ty::Float(FloatKind::F64));
        this.long_double = this.intern(Ty::Float(FloatKind::F80));
        this.complex_float = this.intern(Ty::Complex(FloatKind::F32));
        this.complex_double = this.intern(Ty::Complex(FloatKind::F64));
        this.complex_long_double = this.intern(Ty::Complex(FloatKind::F80));
        this.error = this.intern(Ty::Error);
        this.builtin_va_list = this.intern(Ty::BuiltinVaList);
        this
    }

    /// Intern a `Ty` and return its id.
    pub fn intern(&mut self, ty: Ty) -> TyId {
        if let Some(&id) = self.dedup.get(&ty) {
            return id;
        }
        let id = TyId(self.types.len() as u32);
        self.types.push(ty.clone());
        self.dedup.insert(ty, id);
        id
    }

    /// Look up a type by id.
    pub fn get(&self, id: TyId) -> &Ty {
        &self.types[id.0 as usize]
    }
}

impl Default for TyCtxt {
    fn default() -> Self {
        Self::new()
    }
}
