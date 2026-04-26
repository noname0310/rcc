//! `rcc_typeck`: type checking + implicit conversion insertion.
//!
//! Implements C99 §6.3 (conversions), §6.5 (expression typing), and
//! §6.6 (constant expressions). Mutates the HIR in place by inserting
//! `HirExprKind::Convert { .. }` nodes where an implicit conversion applies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_hir::{
    Body, ConvertKind, FloatKind, HirCrate, HirExpr, HirExprId, HirExprKind, IntRank, Qual, Ty,
    TyCtxt, TyId, ValueCat,
};
use rcc_session::Session;

pub mod const_eval;

pub use const_eval::{ConstEval, ConstValue};

/// Width in bits of `int` assumed by the type checker.
///
/// Target abstraction will land in phase 15; until then every backend in the
/// workspace assumes a 32-bit `int`, matching the assumption other phases
/// have already baked in (see e.g. enumerator value selection in
/// `rcc_hir_lower`).
const INT_BITS: u32 = 32;

/// Run full type checking over `hir`. After this call every `HirExpr` has a
/// resolved `ty` and every mandatory implicit conversion has been inserted.
///
/// M2 scope: interface only.
pub fn check(_session: &mut Session, _tcx: &mut TyCtxt, _hir: &mut HirCrate) {
    // Implementation in M2-follow-up.
}

/// Integer promotion (C99 §6.3.1.1).
///
/// Applied to a value of integer type. The return value is the type the
/// operand should be converted to before further evaluation:
///
/// * For non-bitfield operands (`bit_width == None`):
///   - Any integer type whose conversion rank is **less than** that of `int`
///     (`_Bool`, `char`, `signed char`, `unsigned char`, `short`,
///     `unsigned short`) promotes to `int` if every value of the original
///     type is representable in `int`, otherwise `unsigned int`.
///   - All other integer types are unchanged.
///
/// * For bitfield operands (`bit_width == Some(n)`):
///   - Promotion is governed by the bitfield's value range, not its declared
///     storage type's range. A bitfield of width `n` declared with a signed
///     integer type holds `[-2^(n-1), 2^(n-1) - 1]`; one declared with an
///     unsigned integer type holds `[0, 2^n - 1]`.
///   - If `int` can represent every value of the bitfield → `int`,
///     otherwise → `unsigned int`. By the time a bitfield with rank greater
///     than `int` matters, `n` has already exceeded `INT_BITS`, so the rule
///     "every value representable" still produces the right answer.
///
/// Non-integer types pass through unchanged so callers can chain this with
/// the usual arithmetic conversions blindly.
pub fn integer_promotion(tcx: &TyCtxt, ty: TyId, bit_width: Option<u32>) -> TyId {
    let Ty::Int { signed, rank } = *tcx.get(ty) else {
        return ty;
    };

    if let Some(width) = bit_width {
        // C99 §6.3.1.1p2: a bitfield is promoted based on the values it can
        // actually hold. A zero-width bitfield is not an lvalue and therefore
        // never reaches integer promotion, but we treat it as fitting in `int`
        // for safety (range is the empty set, trivially a subset of `int`).
        return promote_bitfield(tcx, signed, width);
    }

    // Non-bitfield: lookup by rank.
    match rank {
        IntRank::Bool | IntRank::Char | IntRank::Short => {
            // Every value of these types fits in a 32-bit signed `int` on
            // every target rcc cares about, so the answer is always `int`.
            // (`unsigned short` on a 16-bit-int target would map to
            // `unsigned int`; that branch is dead today but kept explicit
            // below for clarity once `INT_BITS` becomes target-dependent.)
            if signed || sub_int_unsigned_fits_in_int(rank) {
                tcx.int
            } else {
                tcx.uint
            }
        }
        IntRank::Int | IntRank::Long | IntRank::LongLong => ty,
    }
}

/// `unsigned char` / `unsigned short` always fit in `int` when
/// `INT_BITS == 32`. Helper exists so the day-`INT_BITS`-becomes-16 edit
/// touches one place.
fn sub_int_unsigned_fits_in_int(rank: IntRank) -> bool {
    match rank {
        // `_Bool` has range {0, 1}; `unsigned char` is at most 8 bits;
        // `unsigned short` is at least 16 bits, but on every modern target
        // (and on every target rcc compiles for) <= INT_BITS - 1.
        IntRank::Bool | IntRank::Char | IntRank::Short => true,
        _ => false,
    }
}

fn promote_bitfield(tcx: &TyCtxt, signed: bool, width: u32) -> TyId {
    // Width 0 is special: non-promotable named bitfields have width >= 1, and
    // unnamed zero-width bitfields are never read. Map to `int` for safety.
    if width == 0 {
        return tcx.int;
    }

    if signed {
        // Signed bitfield value range is [-2^(w-1), 2^(w-1) - 1]. Any width up
        // to `INT_BITS` fits in `int`; widths greater than `INT_BITS` cannot
        // occur in well-formed C99 (bitfield width must not exceed the
        // declared type's width), but if they did the value would still fit
        // when the storage type rank is > Int — and the storage-type rank
        // would already exceed `int`, so falling through to `int` is wrong;
        // however, integer_promotion's contract for storage rank > int is
        // "stay unchanged", which is handled by the early rank check above
        // for non-bitfields. For bitfields of rank > int, the user asked for
        // a sub-int promotion of a wider value; treat as `unsigned int` if
        // it doesn't fit in signed int.
        if width <= INT_BITS {
            tcx.int
        } else {
            tcx.uint
        }
    } else {
        // Unsigned bitfield value range is [0, 2^w - 1]. Fits in signed `int`
        // (which holds [0, 2^(INT_BITS-1) - 1] on the non-negative side) iff
        // `w < INT_BITS`.
        if width < INT_BITS {
            tcx.int
        } else {
            tcx.uint
        }
    }
}

/// Width in bits of an `IntRank` on the LP64 model rcc currently targets.
///
/// Phase 15 (`TargetInfo`) replaces this hard-coded table with a
/// target-driven one. Until then, every backend rcc supports is LP64
/// (`int` = 32, `long` = `long long` = 64, plus 8-bit `char` and 16-bit
/// `short`). Values match what `rcc_codegen_llvm` already emits.
fn int_rank_bits(rank: IntRank) -> u32 {
    match rank {
        IntRank::Bool => 1,
        IntRank::Char => 8,
        IntRank::Short => 16,
        IntRank::Int => INT_BITS,
        IntRank::Long => 64,
        IntRank::LongLong => 64,
    }
}

/// "Unsigned counterpart" of an `IntRank`. For C99 §6.3.1.8 step 4 we may
/// need `unsigned long` from `long`, etc. Helper returns the matching
/// pre-interned `TyId` from the context.
fn unsigned_counterpart(tcx: &TyCtxt, rank: IntRank) -> TyId {
    match rank {
        // `_Bool` is already unsigned; `char`'s unsigned counterpart is
        // `unsigned char`. Neither path is reachable for the §6.3.1.8 rule
        // (their integer-promoted form is `int`/`unsigned int`), but we
        // keep the entries so the helper is total.
        IntRank::Bool => tcx.bool_,
        IntRank::Char => tcx.uchar,
        IntRank::Short => tcx.ushort,
        IntRank::Int => tcx.uint,
        IntRank::Long => tcx.ulong,
        IntRank::LongLong => tcx.ulong_long,
    }
}

/// Usual arithmetic conversions (C99 §6.3.1.8). Returns the common real type.
///
/// Implements the spec ladder verbatim:
///
/// 1. If either operand has `long double` type, the other is converted to
///    `long double`.
/// 2. Otherwise, if either has `double` type, the other → `double`.
/// 3. Otherwise, if either has `float` type, the other → `float`.
/// 4. Otherwise, integer promotions are performed on both operands, then
///    one of the following sub-rules applies:
///    - (4a) If both have the same type, no further conversion is needed.
///    - (4b) If both are signed or both are unsigned, the operand of lesser
///      rank is converted to the type of the operand of greater rank.
///    - (4c.i) Otherwise (exactly one operand is signed, the other
///      unsigned), if the unsigned operand has rank ≥ signed operand's
///      rank, convert the signed operand to the unsigned type.
///    - (4c.ii) Else if the signed type can represent every value of the
///      unsigned type (signed has more value bits), convert the unsigned
///      operand to the signed type.
///    - (4c.iii) Otherwise, both operands are converted to the unsigned
///      counterpart of the signed operand's type.
///
/// `_Complex` arithmetic (C99 §6.3.1.8 second paragraph) is deferred to
/// task 07-12; we only handle real arithmetic here.
///
/// The caller is responsible for actually inserting `Convert` nodes on
/// each operand to bring it to the returned common type.
pub fn usual_arithmetic(tcx: &TyCtxt, a: TyId, b: TyId) -> TyId {
    // Steps 1-3: floating types dominate, in long-double / double / float order.
    match (tcx.get(a), tcx.get(b)) {
        (Ty::Float(FloatKind::F80), _) | (_, Ty::Float(FloatKind::F80)) => return tcx.long_double,
        (Ty::Float(FloatKind::F64), _) | (_, Ty::Float(FloatKind::F64)) => return tcx.double,
        (Ty::Float(FloatKind::F32), _) | (_, Ty::Float(FloatKind::F32)) => return tcx.float,
        _ => {}
    }

    // Step 4: apply integer promotion to both operands.
    let a = integer_promotion(tcx, a, None);
    let b = integer_promotion(tcx, b, None);

    // Decompose both promoted operands into (signed, rank). Non-integer
    // operands (`Ty::Error`, pointers, records) reach this function only
    // through a malformed call; keep it lossy by returning the first
    // operand unchanged so downstream passes can keep going on already
    // poisoned input.
    //
    // `Ty` is not `Copy` (some variants carry a `Vec`), so we destructure
    // through a reference; `signed`/`rank` are themselves `Copy` and bind
    // by value via the default-binding-mode rules.
    let Ty::Int { signed: sa, rank: ra } = tcx.get(a) else { return a };
    let Ty::Int { signed: sb, rank: rb } = tcx.get(b) else { return a };
    let (sa, ra, sb, rb) = (*sa, *ra, *sb, *rb);

    // Step 4a: same type after promotion → done.
    if a == b {
        return a;
    }

    // Step 4b: same signedness → operand of greater rank wins.
    if sa == sb {
        return if ra >= rb { a } else { b };
    }

    // Step 4c: mixed signedness. Identify the signed and unsigned operands.
    let (signed_ty, signed_rank, unsigned_ty, unsigned_rank) =
        if sa { (a, ra, b, rb) } else { (b, rb, a, ra) };

    // Step 4c.i: unsigned rank ≥ signed rank → result is the unsigned type.
    if unsigned_rank >= signed_rank {
        return unsigned_ty;
    }

    // Step 4c.ii: signed rank > unsigned rank. The signed type can represent
    // every value of the unsigned type iff it has strictly more value bits
    // (signed-bits − 1 > unsigned-bits, i.e. signed-bits ≥ unsigned-bits + 2).
    // On LP64 this is true for `long`/`long long` paired with `unsigned int`
    // (64 ≥ 32 + 2) and for any wider signed paired with a strictly narrower
    // unsigned. On a hypothetical LLP64 target where `long` is 32 bits this
    // helper would correctly fall through to step 4c.iii.
    let signed_bits = int_rank_bits(signed_rank);
    let unsigned_bits = int_rank_bits(unsigned_rank);
    if signed_bits >= unsigned_bits + 2 {
        return signed_ty;
    }

    // Step 4c.iii: convert both to the unsigned counterpart of the signed
    // operand's type. (Reached today only on hypothetical non-LP64 targets;
    // included for spec completeness so phase-15 retargeting is one edit.)
    unsigned_counterpart(tcx, signed_rank)
}

/// Syntactic context in which an expression appears, for the purposes of
/// C99 §6.3.2.1p3 / p4 array-and-function decay.
///
/// The default context (`Normal`) decays array lvalues to a pointer to the
/// first element and function designators to a pointer to function. The
/// other variants are the spec's enumerated exceptions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecayContext {
    /// Ordinary use: arrays decay to `&arr[0]`, functions decay to `&func`.
    Normal,
    /// Operand of `sizeof` (C99 §6.3.2.1p3: array case) /
    /// `sizeof` of a function-designator is a constraint violation but we
    /// still decline to decay so the diagnostic can spot the function type.
    SizeofOperand,
    /// Operand of unary `&` (C99 §6.3.2.1p3 array case + p4 function case).
    /// Address-of an array yields a pointer to the array, not to its first
    /// element; address-of a function yields a pointer to the function
    /// (semantically identical to the decayed form, but no `Convert` is
    /// inserted because `&f` and `f` are interchangeable per p4).
    AddrOfOperand,
    /// String literal used to initialise a `char[]` array (C99 §6.7.8p14):
    /// the array initialiser keeps its array type rather than decaying to
    /// `char *`.
    CharArrayInitializer,
}

/// Apply C99 §6.3.2.1p3 (array → pointer) and §6.3.2.1p4 (function →
/// pointer) decay to `expr` if `ctx` permits it. Returns the id of either:
///
/// * the original expression (no decay needed or context forbids it), or
/// * a freshly-pushed `HirExprKind::Convert { kind: ArrayToPtr | FuncToPtr }`
///   wrapper whose `ty` is the decayed pointer type.
///
/// The wrapper's `value_cat` is always `RValue` — both decays produce a
/// non-modifiable rvalue per the spec ("which is not an lvalue").
///
/// `ctx == Normal` is the rule; the other variants encode the three
/// enumerated exceptions in p3/p4. Callers should pass the more specific
/// variant whenever the syntactic position is known. Unknown positions
/// default to `Normal` (the conservative choice — failing to decay where
/// the spec requires decay is a soundness bug; decaying where it isn't
/// required is at worst a missed diagnostic).
pub fn decay_if_needed(
    tcx: &mut TyCtxt,
    body: &mut Body,
    expr: HirExprId,
    ctx: DecayContext,
) -> HirExprId {
    // Look up the operand's type; clone the relevant variants so we can
    // hand `tcx` back as `&mut` for `intern`.
    let (decay_kind, new_ty) = match tcx.get(body.exprs[expr].ty).clone() {
        Ty::Array { elem, .. } if ctx == DecayContext::Normal => {
            (ConvertKind::ArrayToPtr, tcx.intern(Ty::Ptr(elem)))
        }
        Ty::Func { .. } if ctx == DecayContext::Normal => {
            // `func -> &func` is type "pointer to function", with no
            // qualifiers (functions cannot be qualified).
            let func_ty = body.exprs[expr].ty;
            (ConvertKind::FuncToPtr, tcx.intern(Ty::Ptr(Qual::plain(func_ty))))
        }
        // Either the operand is not a candidate for decay, or `ctx` forbids
        // the conversion in this position. In both cases the spec says the
        // expression keeps its original type, so we hand `expr` back
        // verbatim — no Convert wrapper is inserted.
        _ => return expr,
    };

    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: new_ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: decay_kind },
    });
    body.exprs[id].id = id;
    id
}

/// Compute the value category of `expr` per C99 §6.3.2.1.
///
/// An *lvalue* is an expression that designates an object; an *rvalue*
/// (or, in the standard's wording, the value of an expression that is
/// not an lvalue) is everything else — including the value-producing
/// result of a cast, a function call, a binary operator, address-of,
/// etc. C99 §6.3.2.1p1 enumerates the lvalue-producing forms; this
/// function is the canonical encoder of that table for HIR.
///
/// The classification is computed *from the kind*, not read from
/// `HirExpr::value_cat`: lowering writes a best-guess category as the
/// nodes are produced, but the type-checker must own the final answer
/// because lowering does not have full type information yet (e.g. the
/// distinction between a function designator and a regular identifier
/// depends on the resolved `DefKind`).
///
/// The rules implemented here are:
///
/// | HIR kind                                        | Category |
/// |-------------------------------------------------|----------|
/// | `IntConst`, `FloatConst`                         | rvalue   |
/// | `StringRef`                                      | lvalue   |
/// | `LocalRef`, `DefRef`                             | lvalue   |
/// | `Deref(p)` (i.e. `*p`)                           | lvalue   |
/// | `Index { base, .. }` (`a[i]` lowered to `*(a+i)`)| lvalue   |
/// | `Field { base, .. }` (`s.f`, `p->f`)             | inherits from `base` |
/// | `Convert { kind: LvalueToRvalue }`              | rvalue   |
/// | `Convert { kind: ArrayToPtr | FuncToPtr }`      | rvalue   |
/// | other `Convert { .. }`                          | rvalue   |
/// | `Cast { .. }`                                   | rvalue   |
/// | `Binary`, `Unary`, `Call`                       | rvalue   |
/// | `AddressOf`                                     | rvalue   |
/// | `Cond`, `Comma`, `Assign`                       | rvalue   |
///
/// Notes:
/// - `Field` follows the base because C99 §6.5.2.3p3 says `s.f` is an
///   lvalue iff `s` is. The `p->f` case is always an lvalue and is
///   already represented as `Field { base: Deref(p), .. }` in HIR, so
///   the recursive rule produces the right answer without a special
///   case.
/// - Pre/post increment and decrement are *rvalues*: they produce the
///   updated (or original) value, not an lvalue designating the
///   modified object (C99 §6.5.3.1p2 and §6.5.2.4p2). They're carried
///   in `Unary`, which uniformly returns rvalue.
/// - Assignment expressions (`a = b`) are rvalues per C99 §6.5.16p3.
pub fn value_category(body: &Body, expr: HirExprId) -> ValueCat {
    match body.exprs[expr].kind {
        // Constants and arithmetic / pointer-producing operators.
        HirExprKind::IntConst(_)
        | HirExprKind::FloatConst(_)
        | HirExprKind::Binary { .. }
        | HirExprKind::Unary { .. }
        | HirExprKind::Call { .. }
        | HirExprKind::Cast { .. }
        | HirExprKind::AddressOf(_)
        | HirExprKind::Cond { .. }
        | HirExprKind::Comma { .. }
        | HirExprKind::Assign { .. }
        | HirExprKind::Convert { .. } => ValueCat::RValue,

        // Identifier-style designators are lvalues. String literals are
        // arrays of `char` (with static storage duration) and §6.4.5p6
        // makes them lvalues that decay to pointers in most contexts.
        HirExprKind::LocalRef(_)
        | HirExprKind::DefRef(_)
        | HirExprKind::StringRef(_)
        | HirExprKind::Deref(_)
        | HirExprKind::Index { .. } => ValueCat::LValue,

        // `s.f` is an lvalue iff `s` is. `p->f` is lowered as
        // `Field { base: Deref(p), .. }`, so this also covers it.
        HirExprKind::Field { base, .. } => value_category(body, base),
    }
}

/// Apply the C99 §6.3.2.1p2 lvalue-to-rvalue conversion to `expr` if
/// needed. Returns the id of either:
///
/// * the original expression, or
/// * a freshly-pushed `Convert { kind: LvalueToRvalue }` wrapper whose
///   type strips top-level qualifiers (§6.3.2.1p2: "the value has the
///   unqualified version of the type of the lvalue") and whose
///   `value_cat` is `RValue`.
///
/// The conversion is *not* applied to:
///
/// * expressions of array type — those decay via `decay_if_needed`
///   (§6.3.2.1p3) and the lvalue-to-rvalue rule explicitly excludes
///   them ("except when it is the operand of … or is an array");
/// * expressions that are already rvalues (no-op);
/// * function designators — handled by `decay_if_needed`.
///
/// Callers responsible for context-specific exemptions (operand of
/// `sizeof`, `&`, the LHS of `=` / `op=`, `++`/`--`) must simply not
/// call this helper in those positions. The helper is the unconditional
/// "force this position to an rvalue" primitive; the calling-side
/// decision of whether to force is in task 07-07.
pub fn lvalue_to_rvalue_if_needed(tcx: &mut TyCtxt, body: &mut Body, expr: HirExprId) -> HirExprId {
    if value_category(body, expr) == ValueCat::RValue {
        return expr;
    }

    let orig_ty = body.exprs[expr].ty;

    // Arrays and functions don't take this path (they decay first).
    // We're conservative here: if the operand still has array/function
    // type by the time we're invoked, leave it alone — `decay_if_needed`
    // is the right tool.
    match tcx.get(orig_ty) {
        Ty::Array { .. } | Ty::Func { .. } => return expr,
        _ => {}
    }

    // C99 §6.3.2.1p2: the converted value has the *unqualified* version
    // of the lvalue's type. For our `Ty` model qualifiers ride on the
    // pointee inside `Ptr` / `Array::elem`; the top-level `TyId` for an
    // ordinary scalar already has no qualifiers, so no rewrite is
    // required. Pointer-to-qualified-T stays pointer-to-qualified-T:
    // the qualifier belongs to the pointee, not the pointer value.
    let new_ty = orig_ty;
    let span = body.exprs[expr].span;
    let id = body.exprs.push(HirExpr {
        id: HirExprId(0),
        ty: new_ty,
        value_cat: ValueCat::RValue,
        span,
        kind: HirExprKind::Convert { operand: expr, kind: ConvertKind::LvalueToRvalue },
    });
    body.exprs[id].id = id;
    id
}

/// Verify that `lhs` is an lvalue, suitable as the destination of an
/// assignment (`=` or any compound `op=`). Emits E0080 ("assignment to
/// rvalue") when the LHS is not an lvalue and returns `false`. The
/// caller is then free to either keep going (the typechecker will paper
/// over the constraint violation downstream) or skip further checks on
/// the offending statement.
///
/// This helper covers C99 §6.5.16p2's *lvalue* requirement only. The
/// orthogonal *modifiable*-lvalue requirement (no const-qualified
/// objects, no array types, no incomplete types, no const-qualified
/// member of a struct/union, …) lives in task 07-05.
pub fn check_assignment_lhs(session: &mut Session, body: &Body, lhs: HirExprId) -> bool {
    if value_category(body, lhs) == ValueCat::LValue {
        return true;
    }

    let span = body.exprs[lhs].span;
    session
        .handler
        .struct_err(span, "assignment to rvalue: left operand must designate an object")
        .code(rcc_errors::codes::E0080)
        .emit();
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Truth table for non-bitfield integer promotion.
    /// (input alias getter, expected output alias getter)
    #[test]
    fn integer_promotion_truth_table_non_bitfield() {
        let tcx = TyCtxt::new();

        // (description, input, expected)
        let cases: &[(&str, TyId, TyId)] = &[
            ("_Bool -> int", tcx.bool_, tcx.int),
            ("signed char -> int", tcx.schar, tcx.int),
            ("char -> int", tcx.char_, tcx.int),
            ("unsigned char -> int", tcx.uchar, tcx.int),
            ("short -> int", tcx.short, tcx.int),
            ("unsigned short -> int", tcx.ushort, tcx.int),
            ("int -> int (unchanged)", tcx.int, tcx.int),
            ("unsigned int -> unsigned int (unchanged)", tcx.uint, tcx.uint),
            ("long -> long (unchanged)", tcx.long, tcx.long),
            ("unsigned long -> unsigned long (unchanged)", tcx.ulong, tcx.ulong),
            ("long long -> long long (unchanged)", tcx.long_long, tcx.long_long),
            ("unsigned long long -> unsigned long long", tcx.ulong_long, tcx.ulong_long),
        ];

        for (desc, input, expected) in cases {
            let got = integer_promotion(&tcx, *input, None);
            assert_eq!(got, *expected, "{desc}");
        }
    }

    #[test]
    fn integer_promotion_passes_through_non_integer_types() {
        let tcx = TyCtxt::new();
        // void / float / double / long double / error all pass through.
        for ty in [tcx.void, tcx.float, tcx.double, tcx.long_double, tcx.error] {
            assert_eq!(integer_promotion(&tcx, ty, None), ty);
        }
    }

    #[test]
    fn integer_promotion_unsigned_int_bitfield_3bit_to_int() {
        // Acceptance criterion from the task: a 3-bit unsigned int bitfield
        // promotes to `int`, since its range [0, 7] fits in int.
        let tcx = TyCtxt::new();
        let got = integer_promotion(&tcx, tcx.uint, Some(3));
        assert_eq!(got, tcx.int);
    }

    #[test]
    fn integer_promotion_bitfield_unsigned_widths() {
        let tcx = TyCtxt::new();

        // Unsigned bitfield: fits in signed int iff width <= INT_BITS - 1 = 31.
        for width in 1..=31u32 {
            let got = integer_promotion(&tcx, tcx.uint, Some(width));
            assert_eq!(got, tcx.int, "unsigned int : {width} should promote to int");
        }
        // 32-bit unsigned bitfield exceeds signed int range -> unsigned int.
        let got = integer_promotion(&tcx, tcx.uint, Some(32));
        assert_eq!(got, tcx.uint);
    }

    #[test]
    fn integer_promotion_bitfield_signed_widths() {
        let tcx = TyCtxt::new();

        // Signed bitfield always fits in signed int up to INT_BITS = 32 bits.
        for width in 1..=32u32 {
            let got = integer_promotion(&tcx, tcx.int, Some(width));
            assert_eq!(got, tcx.int, "signed int : {width} should promote to int");
        }
    }

    #[test]
    fn integer_promotion_bitfield_storage_rank_governs_signedness() {
        // The C99 rule says rank/signedness is "determined by the declared
        // type" — so an `unsigned char : 4` bitfield is treated with the
        // unsigned-range formula even though the natural promotion of
        // `unsigned char` (no bitfield) is also `int`.
        let tcx = TyCtxt::new();

        // unsigned char : 4 -> [0, 15] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.uchar, Some(4)), tcx.int);
        // signed char : 4 -> [-8, 7] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.schar, Some(4)), tcx.int);
        // unsigned short : 16 -> [0, 65535] fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.ushort, Some(16)), tcx.int);
        // _Bool : 1 -> {0, 1} fits in int -> int
        assert_eq!(integer_promotion(&tcx, tcx.bool_, Some(1)), tcx.int);
    }

    #[test]
    fn integer_promotion_bitfield_zero_width_maps_to_int() {
        // Width-0 bitfields are never read, but if integer_promotion is
        // accidentally invoked on one we must not panic and we must produce
        // something sensible.
        let tcx = TyCtxt::new();
        assert_eq!(integer_promotion(&tcx, tcx.uint, Some(0)), tcx.int);
        assert_eq!(integer_promotion(&tcx, tcx.int, Some(0)), tcx.int);
    }

    #[test]
    fn usual_arithmetic_still_works_after_signature_change() {
        // Smoke-test: usual_arithmetic was the in-tree caller that needed
        // updating. Make sure char + char still yields int.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.char_, tcx.char_), tcx.int);
        assert_eq!(usual_arithmetic(&tcx, tcx.short, tcx.uint), tcx.uint);
        assert_eq!(usual_arithmetic(&tcx, tcx.long, tcx.int), tcx.long);
    }

    /// Acceptance criteria spelled out in the task file.
    #[test]
    fn usual_arithmetic_acceptance_signed_int_op_unsigned_int() {
        // Step 4c.i (equal rank, mixed signedness): result is `unsigned int`.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.int, tcx.uint), tcx.uint);
        assert_eq!(usual_arithmetic(&tcx, tcx.uint, tcx.int), tcx.uint);
    }

    #[test]
    fn usual_arithmetic_acceptance_long_op_unsigned_int_lp64() {
        // Step 4c.ii: on LP64, `long` has 64 bits and can represent every
        // value of 32-bit `unsigned int`, so the result is `long`.
        let tcx = TyCtxt::new();
        assert_eq!(usual_arithmetic(&tcx, tcx.long, tcx.uint), tcx.long);
        assert_eq!(usual_arithmetic(&tcx, tcx.uint, tcx.long), tcx.long);
    }

    /// Truth-table for §6.3.1.8 across the 13 scalar types. Checks every
    /// rule (steps 1-9) at least twice with both orderings (a,b) and (b,a)
    /// to make sure the implementation is symmetric.
    ///
    /// The 13 types per the spec are:
    ///   long double, double, float,
    ///   long long, unsigned long long,
    ///   long, unsigned long,
    ///   int, unsigned int,
    ///   short, unsigned short,
    ///   char, _Bool.
    ///
    /// We do not literally enumerate 169 pairs — instead the table encodes
    /// representative cases for every C99 sub-rule.
    #[test]
    fn usual_arithmetic_truth_table_13_scalars() {
        let tcx = TyCtxt::new();

        // Each row: (description, lhs, rhs, expected common type).
        // The implementation must be symmetric, so we feed each row twice
        // (a,b) and (b,a). Cells where lhs == rhs are not duplicated.
        let cases: &[(&str, TyId, TyId, TyId)] = &[
            // ---- Step 1: long double dominates everything. ----
            ("long double / long double", tcx.long_double, tcx.long_double, tcx.long_double),
            ("long double / double", tcx.long_double, tcx.double, tcx.long_double),
            ("long double / float", tcx.long_double, tcx.float, tcx.long_double),
            ("long double / int", tcx.long_double, tcx.int, tcx.long_double),
            ("long double / unsigned long long", tcx.long_double, tcx.ulong_long, tcx.long_double),
            ("long double / _Bool", tcx.long_double, tcx.bool_, tcx.long_double),
            // ---- Step 2: double beats float and any integer. ----
            ("double / double", tcx.double, tcx.double, tcx.double),
            ("double / float", tcx.double, tcx.float, tcx.double),
            ("double / unsigned long", tcx.double, tcx.ulong, tcx.double),
            ("double / char", tcx.double, tcx.char_, tcx.double),
            // ---- Step 3: float beats any integer. ----
            ("float / float", tcx.float, tcx.float, tcx.float),
            ("float / long long", tcx.float, tcx.long_long, tcx.float),
            ("float / unsigned int", tcx.float, tcx.uint, tcx.float),
            ("float / _Bool", tcx.float, tcx.bool_, tcx.float),
            // ---- Step 4a: integer promotion brings both to the same type. ----
            ("_Bool / _Bool -> int", tcx.bool_, tcx.bool_, tcx.int),
            ("char / char -> int", tcx.char_, tcx.char_, tcx.int),
            ("short / short -> int", tcx.short, tcx.short, tcx.int),
            ("unsigned short / unsigned short -> int", tcx.ushort, tcx.ushort, tcx.int),
            ("char / short -> int (both promote to int)", tcx.char_, tcx.short, tcx.int),
            ("_Bool / unsigned short -> int", tcx.bool_, tcx.ushort, tcx.int),
            ("int / int", tcx.int, tcx.int, tcx.int),
            ("unsigned int / unsigned int", tcx.uint, tcx.uint, tcx.uint),
            ("long / long", tcx.long, tcx.long, tcx.long),
            ("unsigned long / unsigned long", tcx.ulong, tcx.ulong, tcx.ulong),
            ("long long / long long", tcx.long_long, tcx.long_long, tcx.long_long),
            (
                "unsigned long long / unsigned long long",
                tcx.ulong_long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4b: same signedness, different rank. ----
            ("int / long -> long (both signed)", tcx.int, tcx.long, tcx.long),
            ("int / long long -> long long (both signed)", tcx.int, tcx.long_long, tcx.long_long),
            ("long / long long -> long long (both signed)", tcx.long, tcx.long_long, tcx.long_long),
            ("unsigned int / unsigned long -> unsigned long", tcx.uint, tcx.ulong, tcx.ulong),
            (
                "unsigned long / unsigned long long -> unsigned long long",
                tcx.ulong,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            (
                "unsigned int / unsigned long long -> unsigned long long",
                tcx.uint,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.i: equal rank, mixed signedness → unsigned wins. ----
            ("int / unsigned int -> unsigned int", tcx.int, tcx.uint, tcx.uint),
            ("long / unsigned long -> unsigned long", tcx.long, tcx.ulong, tcx.ulong),
            (
                "long long / unsigned long long -> unsigned long long",
                tcx.long_long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.i: unsigned rank > signed rank → unsigned wins. ----
            ("int / unsigned long -> unsigned long", tcx.int, tcx.ulong, tcx.ulong),
            (
                "int / unsigned long long -> unsigned long long",
                tcx.int,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            (
                "long / unsigned long long -> unsigned long long",
                tcx.long,
                tcx.ulong_long,
                tcx.ulong_long,
            ),
            // ---- Step 4c.ii: signed rank > unsigned rank, signed type can
            //                  represent every value of the unsigned type
            //                  (LP64: long has 64 bits, unsigned int has 32).
            ("long / unsigned int -> long (LP64)", tcx.long, tcx.uint, tcx.long),
            (
                "long long / unsigned int -> long long (LP64)",
                tcx.long_long,
                tcx.uint,
                tcx.long_long,
            ),
            // After integer promotion, `unsigned short` becomes `int` (every
            // value of unsigned short fits in int on a 32-bit-int target),
            // so pairing it with `long` falls through to step 4b after
            // promotion, not 4c. Same for char/_Bool.
            ("long / unsigned short -> long", tcx.long, tcx.ushort, tcx.long),
            ("long long / unsigned short -> long long", tcx.long_long, tcx.ushort, tcx.long_long),
            ("long / char -> long", tcx.long, tcx.char_, tcx.long),
            ("long / _Bool -> long", tcx.long, tcx.bool_, tcx.long),
            // ---- Sub-int signed/unsigned mixes promote to int/unsigned int
            //      via §6.3.1.1, then re-enter §6.3.1.8 step 4. ----
            ("char / unsigned int -> unsigned int", tcx.char_, tcx.uint, tcx.uint),
            ("short / unsigned int -> unsigned int", tcx.short, tcx.uint, tcx.uint),
            ("unsigned short / int -> int (promotes to int)", tcx.ushort, tcx.int, tcx.int),
            ("unsigned char / int -> int", tcx.uchar, tcx.int, tcx.int),
            ("_Bool / int -> int", tcx.bool_, tcx.int, tcx.int),
            ("_Bool / unsigned int -> unsigned int", tcx.bool_, tcx.uint, tcx.uint),
        ];

        for (desc, a, b, expected) in cases {
            let got_ab = usual_arithmetic(&tcx, *a, *b);
            assert_eq!(got_ab, *expected, "(a,b): {desc}");
            let got_ba = usual_arithmetic(&tcx, *b, *a);
            assert_eq!(got_ba, *expected, "(b,a): {desc} (symmetry)");
        }
    }

    /// Direct white-box test for step 4c.iii: when the signed type cannot
    /// represent every value of the unsigned type, both convert to the
    /// unsigned counterpart of the signed type. This branch is unreachable
    /// on LP64 with the current scalar set (every signed type whose rank
    /// strictly exceeds an unsigned operand's rank also has at least 2
    /// extra bits over it). We exercise it indirectly via the helper.
    #[test]
    fn usual_arithmetic_step_4c_iii_helpers() {
        let tcx = TyCtxt::new();
        assert_eq!(unsigned_counterpart(&tcx, IntRank::Int), tcx.uint);
        assert_eq!(unsigned_counterpart(&tcx, IntRank::Long), tcx.ulong);
        assert_eq!(unsigned_counterpart(&tcx, IntRank::LongLong), tcx.ulong_long);
        assert_eq!(int_rank_bits(IntRank::Int), 32);
        assert_eq!(int_rank_bits(IntRank::Long), 64);
        assert_eq!(int_rank_bits(IntRank::LongLong), 64);
        assert_eq!(int_rank_bits(IntRank::Short), 16);
        assert_eq!(int_rank_bits(IntRank::Char), 8);
        assert_eq!(int_rank_bits(IntRank::Bool), 1);
    }

    // ------------------------------------------------------------------
    // Array/function decay (C99 §6.3.2.1p3-4) — decay_if_needed.
    // ------------------------------------------------------------------
    //
    // These tests exercise the helper directly against a hand-built `Body`
    // rather than driving lowering end-to-end; the helper's contract is
    // purely "given an expr id whose type is array/function, return the
    // decayed wrapper unless the context forbids it". End-to-end coverage
    // arrives in task 07-07 once `check()` actually runs.

    use rcc_span::DUMMY_SP;

    /// Build a minimal `IntConst`-shaped leaf expression of type `ty` and
    /// category `cat` and return its id. The constant payload is a stand-in
    /// — the decay helper inspects `ty`/`value_cat` only, never the kind.
    fn push_leaf_expr(body: &mut Body, ty: TyId, cat: ValueCat) -> HirExprId {
        let id = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty,
            value_cat: cat,
            span: DUMMY_SP,
            kind: HirExprKind::IntConst(0),
        });
        body.exprs[id].id = id;
        id
    }

    fn intern_int_array(tcx: &mut TyCtxt, len: u64) -> TyId {
        tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: Some(len), is_vla: false })
    }

    fn intern_int_func_no_args(tcx: &mut TyCtxt) -> TyId {
        let int = tcx.int;
        tcx.intern(Ty::Func { ret: int, params: Vec::new(), variadic: false, proto: true })
    }

    /// Acceptance: `int arr[10]; int *p = arr;` inserts ArrayToPtr around `arr`.
    /// We model this as `decay_if_needed(arr, Normal)` and check the wrapper.
    #[test]
    fn decay_array_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::Normal);

        // A new wrapper expression must have been pushed.
        assert_ne!(decayed, arr_id, "decay must allocate a fresh expr id");
        let wrapper = &body.exprs[decayed];

        // Wrapper kind: Convert { operand: arr_id, kind: ArrayToPtr }.
        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, arr_id);
                assert_eq!(kind, ConvertKind::ArrayToPtr);
            }
            ref other => panic!("expected Convert wrapper, got {other:?}"),
        }

        // Wrapper type: `int *` (Ptr to plain int).
        match tcx.get(wrapper.ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, tcx.int),
            other => panic!("expected Ptr(int), got {other:?}"),
        }

        // Decayed expression is an rvalue (C99 §6.3.2.1p3).
        assert_eq!(wrapper.value_cat, ValueCat::RValue);
    }

    /// Function designator → pointer-to-function (C99 §6.3.2.1p4).
    #[test]
    fn decay_function_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::Normal);

        assert_ne!(decayed, fn_id);
        let wrapper = &body.exprs[decayed];

        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, fn_id);
                assert_eq!(kind, ConvertKind::FuncToPtr);
            }
            ref other => panic!("expected Convert wrapper, got {other:?}"),
        }

        // Wrapper type: pointer to the original function type.
        match tcx.get(wrapper.ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, fn_ty),
            other => panic!("expected Ptr(func_ty), got {other:?}"),
        }

        assert_eq!(wrapper.value_cat, ValueCat::RValue);
    }

    /// Acceptance: `int arr[10]; sizeof arr;` does NOT decay — sizeof returns
    /// 40. We assert the array type is preserved (size is a codegen concern).
    #[test]
    fn decay_array_skipped_inside_sizeof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::SizeofOperand);

        // Same id, same type — no Convert wrapper inserted.
        assert_eq!(result, arr_id, "sizeof operand must not decay");
        assert_eq!(body.exprs[result].ty, arr_ty);
        match tcx.get(body.exprs[result].ty) {
            Ty::Array { len, .. } => assert_eq!(*len, Some(10)),
            other => panic!("expected Array preserved, got {other:?}"),
        }
    }

    /// `&arr` — the operand of unary `&` does not decay (C99 §6.3.2.1p3).
    #[test]
    fn decay_array_skipped_inside_addrof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 10);
        let arr_id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, arr_id, DecayContext::AddrOfOperand);

        assert_eq!(result, arr_id);
        assert_eq!(body.exprs[result].ty, arr_ty);
    }

    /// `char a[] = "abc";` — the string literal initialiser keeps array type.
    #[test]
    fn decay_skipped_inside_char_array_initializer() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let char_arr_ty =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let lit_id = push_leaf_expr(&mut body, char_arr_ty, ValueCat::LValue);

        let result =
            decay_if_needed(&mut tcx, &mut body, lit_id, DecayContext::CharArrayInitializer);

        assert_eq!(result, lit_id);
        assert_eq!(body.exprs[result].ty, char_arr_ty);
    }

    /// Function designator under `sizeof` — sizeof of a function is a
    /// constraint violation in C99, but the helper still declines to decay
    /// so the diagnostic pass can spot the function type. (No diagnostic is
    /// emitted by decay_if_needed itself.)
    #[test]
    fn decay_function_skipped_inside_sizeof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::SizeofOperand);

        assert_eq!(result, fn_id);
        assert_eq!(body.exprs[result].ty, fn_ty);
    }

    /// Function designator under `&` — `&f` and `f` (decayed) are
    /// interchangeable per §6.3.2.1p4, so we leave the operand alone and let
    /// the AddressOf node carry the same pointer-to-function type itself.
    #[test]
    fn decay_function_skipped_inside_addrof() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let fn_id = push_leaf_expr(&mut body, fn_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, fn_id, DecayContext::AddrOfOperand);

        assert_eq!(result, fn_id);
        assert_eq!(body.exprs[result].ty, fn_ty);
    }

    /// Non-array, non-function operands pass through untouched in every
    /// context. Run the rule across the four context variants.
    #[test]
    fn decay_passthrough_for_non_decaying_types() {
        let mut tcx = TyCtxt::new();
        let int_ty = tcx.int;
        for ctx in [
            DecayContext::Normal,
            DecayContext::SizeofOperand,
            DecayContext::AddrOfOperand,
            DecayContext::CharArrayInitializer,
        ] {
            let mut body = Body::default();
            let id = push_leaf_expr(&mut body, int_ty, ValueCat::RValue);
            let result = decay_if_needed(&mut tcx, &mut body, id, ctx);
            assert_eq!(result, id, "non-array/func passthrough in {ctx:?}");
            assert_eq!(body.exprs[result].ty, int_ty);
        }
    }

    /// Pointer-typed operands are not "arrays" — they must pass through
    /// even in `Normal` context (no double-decay).
    #[test]
    fn decay_pointer_does_not_decay() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let id = push_leaf_expr(&mut body, ptr_ty, ValueCat::LValue);

        let result = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        assert_eq!(result, id);
        assert_eq!(body.exprs[result].ty, ptr_ty);
    }

    /// VLAs (`int v[n]`) decay too — `len: None, is_vla: true` is still an
    /// `Array` and its element type is well-defined.
    #[test]
    fn decay_vla_to_ptr_in_normal_context() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let vla_ty = tcx.intern(Ty::Array { elem: Qual::plain(tcx.int), len: None, is_vla: true });
        let id = push_leaf_expr(&mut body, vla_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        assert_ne!(decayed, id);
        match body.exprs[decayed].kind {
            HirExprKind::Convert { kind, .. } => assert_eq!(kind, ConvertKind::ArrayToPtr),
            ref other => panic!("expected Convert/ArrayToPtr, got {other:?}"),
        }
        match tcx.get(body.exprs[decayed].ty) {
            Ty::Ptr(q) => assert_eq!(q.ty, tcx.int),
            other => panic!("expected Ptr(int), got {other:?}"),
        }
    }

    /// Qualified element type (e.g. `const int arr[3]`) decays to a pointer
    /// whose pointee carries the same qualifiers (C99 §6.3.2.1p3 + §6.7.3).
    #[test]
    fn decay_preserves_element_qualifiers() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let elem = Qual { ty: tcx.int, is_const: true, is_volatile: false, is_restrict: false };
        let arr_ty = tcx.intern(Ty::Array { elem, len: Some(3), is_vla: false });
        let id = push_leaf_expr(&mut body, arr_ty, ValueCat::LValue);

        let decayed = decay_if_needed(&mut tcx, &mut body, id, DecayContext::Normal);
        match tcx.get(body.exprs[decayed].ty) {
            Ty::Ptr(q) => {
                assert_eq!(q.ty, tcx.int);
                assert!(q.is_const, "const-ness of element type must survive decay");
                assert!(!q.is_volatile);
            }
            other => panic!("expected Ptr(const int), got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // value_category — every HirExprKind arm.
    // ------------------------------------------------------------------

    use rcc_hir::{rcc_hir_binop::BinOp, rcc_hir_binop::UnOp, DefId, Local};

    /// Push a fully-typed `HirExpr` with the given `kind` and return its id.
    /// `value_cat` here is the *lowering-time guess* that lib.rs writes; the
    /// type-checker is supposed to override it via `value_category`. We
    /// deliberately set it to the WRONG category in some of these tests so
    /// that any accidental "read it back from value_cat" implementation gets
    /// caught.
    fn push_kind(body: &mut Body, ty: TyId, kind: HirExprKind) -> HirExprId {
        let id = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty,
            // Sentinel: the unit under test must derive the answer from
            // `kind`, not echo this back.
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind,
        });
        body.exprs[id].id = id;
        id
    }

    /// Acceptance row: literals are rvalues.
    #[test]
    fn value_category_int_const_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    #[test]
    fn value_category_float_const_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.double, HirExprKind::FloatConst(0.0));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// String literal is an array-typed lvalue (C99 §6.4.5p6).
    #[test]
    fn value_category_string_ref_is_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr =
            tcx.intern(Ty::Array { elem: Qual::plain(tcx.char_), len: Some(4), is_vla: false });
        let id = push_kind(&mut body, arr, HirExprKind::StringRef(DefId(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Identifier resolving to a local object → lvalue (C99 §6.5.1p2).
    #[test]
    fn value_category_local_ref_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Identifier resolving to a top-level def (global / function) → lvalue.
    #[test]
    fn value_category_def_ref_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::DefRef(DefId(0)));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Binary op result is always an rvalue.
    #[test]
    fn value_category_binary_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// Unary op (including pre/post inc/dec) is rvalue per §6.5.3.1p2.
    #[test]
    fn value_category_unary_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let operand = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Unary { op: UnOp::Neg, operand });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// Function call result is rvalue (C99 §6.5.2.2p10 — the value of a
    /// function call is not an lvalue).
    #[test]
    fn value_category_call_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let callee = push_kind(&mut body, tcx.int, HirExprKind::DefRef(DefId(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Call { callee, args: Vec::new() });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `s.f` follows the base. Lvalue base → lvalue field.
    #[test]
    fn value_category_field_inherits_lvalue_from_base() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Field { base, field_index: 0 });
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// `(a + b).f` (rvalue base) → rvalue field. Synthetic but covers the
    /// inheritance rule when the base is not itself an lvalue.
    #[test]
    fn value_category_field_inherits_rvalue_from_base() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let base =
            push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });
        let id = push_kind(&mut body, tcx.int, HirExprKind::Field { base, field_index: 0 });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `a[i]` → lvalue (lowered to `*(a + i)` semantically).
    #[test]
    fn value_category_index_is_lvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let index = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Index { base, index });
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Convert wrappers always produce rvalues — the whole point of an
    /// LvalueToRvalue / ArrayToPtr / FuncToPtr / Pointer / IntegerPromotion
    /// / UsualArithmetic conversion is to *yield a value*.
    #[test]
    fn value_category_convert_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        for kind in [
            ConvertKind::IntegerPromotion,
            ConvertKind::UsualArithmetic,
            ConvertKind::ArrayToPtr,
            ConvertKind::FuncToPtr,
            ConvertKind::LvalueToRvalue,
            ConvertKind::Pointer,
        ] {
            let id = push_kind(&mut body, tcx.int, HirExprKind::Convert { operand: inner, kind });
            assert_eq!(value_category(&body, id), ValueCat::RValue, "Convert {kind:?}");
        }
    }

    /// Cast expression is an rvalue per §6.5.4p4.
    #[test]
    fn value_category_cast_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let operand = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand, to: tcx.int });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `&x` produces a pointer rvalue.
    #[test]
    fn value_category_address_of_is_rvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let id = push_kind(&mut body, ptr_ty, HirExprKind::AddressOf(inner));
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `*p` is an lvalue (C99 §6.5.3.2p4).
    #[test]
    fn value_category_deref_is_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let inner = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(Local(0)));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Deref(inner));
        assert_eq!(value_category(&body, id), ValueCat::LValue);
    }

    /// Conditional `a ? b : c` is an rvalue (§6.5.15p4).
    #[test]
    fn value_category_cond_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let cond = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let then_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(2));
        let else_expr = push_kind(&mut body, tcx.int, HirExprKind::IntConst(3));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Cond { cond, then_expr, else_expr });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `,` is an rvalue.
    #[test]
    fn value_category_comma_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Comma { lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    /// `a = b` is an rvalue (§6.5.16p3 — "An assignment expression has the
    /// value of the left operand after the assignment, but is not an
    /// lvalue").
    #[test]
    fn value_category_assign_is_rvalue() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let rhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let id = push_kind(&mut body, tcx.int, HirExprKind::Assign { lhs, rhs });
        assert_eq!(value_category(&body, id), ValueCat::RValue);
    }

    // ------------------------------------------------------------------
    // lvalue_to_rvalue_if_needed
    // ------------------------------------------------------------------

    /// LValue scalar → wrapped in `Convert { kind: LvalueToRvalue }`.
    #[test]
    fn l_to_r_wraps_scalar_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, inner);
        assert_ne!(after, inner, "scalar lvalue must allocate a Convert wrapper");

        let wrapper = &body.exprs[after];
        match wrapper.kind {
            HirExprKind::Convert { operand, kind } => {
                assert_eq!(operand, inner);
                assert_eq!(kind, ConvertKind::LvalueToRvalue);
            }
            ref other => panic!("expected Convert/LvalueToRvalue, got {other:?}"),
        }
        assert_eq!(wrapper.value_cat, ValueCat::RValue);
        assert_eq!(wrapper.ty, tcx.int);
    }

    /// Already-rvalue → no wrapper, returns same id.
    #[test]
    fn l_to_r_passthrough_rvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let id = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id);
    }

    /// Array-typed lvalue → no wrapper (decay is a separate conversion).
    #[test]
    fn l_to_r_passthrough_array_lvalue() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let arr_ty = intern_int_array(&mut tcx, 3);
        let id = push_kind(&mut body, arr_ty, HirExprKind::LocalRef(Local(0)));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id, "array lvalue must not get LvalueToRvalue wrapper");
    }

    /// Function-designator lvalue → no wrapper.
    #[test]
    fn l_to_r_passthrough_function_designator() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let fn_ty = intern_int_func_no_args(&mut tcx);
        let id = push_kind(&mut body, fn_ty, HirExprKind::DefRef(DefId(0)));
        let after = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, id);
        assert_eq!(after, id, "function designator must not get LvalueToRvalue wrapper");
    }

    /// Idempotent: applying the helper twice does not stack wrappers.
    #[test]
    fn l_to_r_is_idempotent() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let once = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, inner);
        let twice = lvalue_to_rvalue_if_needed(&mut tcx, &mut body, once);
        assert_eq!(once, twice, "second application must be a no-op");
    }

    // ------------------------------------------------------------------
    // check_assignment_lhs (E0080).
    // ------------------------------------------------------------------

    /// Acceptance: `x = 1;` — `x` is an lvalue, no diagnostic.
    #[test]
    fn assignment_lhs_lvalue_local_accepted() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok, "LocalRef LHS must be accepted as lvalue");
        assert!(cap.diagnostics().is_empty(), "no E0080 expected");
    }

    /// Acceptance: `(int)x = 1;` — cast result is an rvalue → E0080.
    #[test]
    fn assignment_lhs_cast_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let inner = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Cast { operand: inner, to: tcx.int });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok, "cast LHS must be rejected as rvalue");

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0080));
    }

    /// `1 = x;` — int literal LHS is an rvalue → E0080.
    #[test]
    fn assignment_lhs_int_const_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0080));
    }

    /// `(a + b) = 1;` — binary-op result LHS rejected.
    #[test]
    fn assignment_lhs_binary_rejected_e0080() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let l = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let r = push_kind(&mut body, tcx.int, HirExprKind::IntConst(1));
        let lhs =
            push_kind(&mut body, tcx.int, HirExprKind::Binary { op: BinOp::Add, lhs: l, rhs: r });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(!ok);
        assert_eq!(cap.diagnostics().len(), 1);
    }

    /// `*p = 1;` — deref LHS is an lvalue, accepted.
    #[test]
    fn assignment_lhs_deref_accepted() {
        let mut tcx = TyCtxt::new();
        let mut body = Body::default();
        let ptr_ty = tcx.intern(Ty::Ptr(Qual::plain(tcx.int)));
        let p = push_kind(&mut body, ptr_ty, HirExprKind::LocalRef(Local(0)));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Deref(p));

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }

    /// `a[i] = 1;` — subscript LHS is an lvalue, accepted.
    #[test]
    fn assignment_lhs_index_accepted() {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        let base = push_kind(&mut body, tcx.int, HirExprKind::LocalRef(Local(0)));
        let idx = push_kind(&mut body, tcx.int, HirExprKind::IntConst(0));
        let lhs = push_kind(&mut body, tcx.int, HirExprKind::Index { base, index: idx });

        let (mut session, cap) = Session::for_test();
        let ok = check_assignment_lhs(&mut session, &body, lhs);
        assert!(ok);
        assert!(cap.diagnostics().is_empty());
    }
}
