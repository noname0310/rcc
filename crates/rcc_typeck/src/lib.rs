//! `rcc_typeck`: type checking + implicit conversion insertion.
//!
//! Implements C99 §6.3 (conversions), §6.5 (expression typing), and
//! §6.6 (constant expressions). Mutates the HIR in place by inserting
//! `HirExprKind::Convert { .. }` nodes where an implicit conversion applies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_hir::{FloatKind, HirCrate, IntRank, Ty, TyCtxt, TyId};
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

/// Usual arithmetic conversions (C99 §6.3.1.8). Returns the common type.
/// Caller is responsible for inserting conversions on both operands.
pub fn usual_arithmetic(tcx: &TyCtxt, a: TyId, b: TyId) -> TyId {
    // Long double dominates, then double, then float.
    match (tcx.get(a), tcx.get(b)) {
        (Ty::Float(FloatKind::F80), _) | (_, Ty::Float(FloatKind::F80)) => tcx.long_double,
        (Ty::Float(FloatKind::F64), _) | (_, Ty::Float(FloatKind::F64)) => tcx.double,
        (Ty::Float(FloatKind::F32), _) | (_, Ty::Float(FloatKind::F32)) => tcx.float,
        _ => {
            // Integer case: promote then pick higher rank; tie-break by signedness.
            let a = integer_promotion(tcx, a, None);
            let b = integer_promotion(tcx, b, None);
            match (tcx.get(a), tcx.get(b)) {
                (Ty::Int { signed: sa, rank: ra }, Ty::Int { signed: sb, rank: rb }) => {
                    if ra > rb {
                        a
                    } else if rb > ra {
                        b
                    } else if sa == sb || !sa {
                        // Equal rank: same signedness -> either (pick `a`);
                        // different signedness -> the unsigned operand wins
                        // (C99 §6.3.1.8), and `!sa` means `a` is unsigned.
                        a
                    } else {
                        b
                    }
                }
                _ => a,
            }
        }
    }
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
}
