//! C99 preprocessing-number recogniser (task 03-lex/05).
//!
//! pp-number grammar (C99 §6.4.8):
//!
//! ```text
//! pp-number := digit
//!            | . digit
//!            | pp-number digit
//!            | pp-number identifier-nondigit
//!            | pp-number e sign
//!            | pp-number E sign
//!            | pp-number p sign
//!            | pp-number P sign
//!            | pp-number .
//! ```
//!
//! Classification into [`PpNumberKind`]:
//! - `Float` if the pp-number contains `.`, or — outside a hex (`0x`/`0X`)
//!   prefix — `e`/`E`, or — inside a hex prefix — `p`/`P`.
//! - `Integer` otherwise.
//!
//! The recogniser is pure maximal-munch: it returns the full byte span
//! regardless of whether the result is a valid integer or floating
//! constant. Actual numeric decoding lives in the parser (phase 05).

use rcc_lexer::{PpNumberKind, PpTokenKind, Tokenizer};
use rcc_span::FileId;

fn tokenize(src: &str) -> Vec<rcc_lexer::PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

/// Strip incidental whitespace / newline tokens so assertions can focus
/// on the pp-number payload.
fn non_ws(src: &str) -> Vec<rcc_lexer::PpToken> {
    tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect()
}

// ── Table-driven shape + classification coverage ────────────────────

#[test]
fn table_driven_pp_number_shapes() {
    // (src, expected_kind, expected_hi)
    //
    // Each row exercises a distinct branch of the pp-number grammar
    // and/or the Integer-vs-Float classification:
    //   - plain decimal integer
    //   - leading zero (C99 lets the parser decide octal vs decimal)
    //   - hex integer
    //   - hex integer with ULL suffix
    //   - decimal float variants (`.42`, `3.14`, `3.`)
    //   - decimal float with exponent + sign + suffix
    //   - hex float with binary exponent (`p`) — with and without sign
    //   - leading-zero decimal float with exponent (`0e+1`)
    //   - maximal munch over a `+`/`-` sign right after `e`/`E`/`p`/`P`
    let cases: &[(&str, PpNumberKind, u32)] = &[
        ("42", PpNumberKind::Integer, 2),
        ("0", PpNumberKind::Integer, 1),
        ("0123", PpNumberKind::Integer, 4),
        ("0xFF", PpNumberKind::Integer, 4),
        ("0xFFULL", PpNumberKind::Integer, 7),
        ("0xdeadbeefULL", PpNumberKind::Integer, 13),
        (".42", PpNumberKind::Float, 3),
        ("3.", PpNumberKind::Float, 2),
        ("3.14", PpNumberKind::Float, 4),
        ("3.14f", PpNumberKind::Float, 5),
        ("3.14e-10f", PpNumberKind::Float, 9),
        ("0e+1", PpNumberKind::Float, 4),
        ("1E2", PpNumberKind::Float, 3),
        ("0x1.0p0", PpNumberKind::Float, 7),
        ("0x1p-2", PpNumberKind::Float, 6),
        ("0x1p+2f", PpNumberKind::Float, 7),
        // Sign-absorption also applies inside a hex prefix when an `e`/`E`
        // happens to be followed by `+`/`-`: the sign is eaten even
        // though the token remains Integer-shaped (no `p`/`P`).
        ("0x1e+2", PpNumberKind::Integer, 6),
    ];

    for (src, want_kind, want_hi) in cases {
        let toks = non_ws(src);
        assert_eq!(toks.len(), 1, "src={src:?} expected exactly one non-ws token, got {toks:?}");
        let t = toks[0];
        assert_eq!(
            t.kind,
            PpTokenKind::PpNumber(*want_kind),
            "src={src:?}: expected PpNumber({want_kind:?}), got {:?}",
            t.kind
        );
        assert_eq!(t.span.lo.0, 0, "src={src:?}: lo mismatch");
        assert_eq!(t.span.hi.0, *want_hi, "src={src:?}: hi mismatch, tok={t:?}");
    }
}

// ── Acceptance: `3.14f + 0xdeadbeefULL` ────────────────────────────

#[test]
fn acceptance_float_plus_hex_integer_classifies_correctly() {
    // Task acceptance: lexing this yields exactly two PpNumber tokens,
    // a Float (`3.14f`) and an Integer (`0xdeadbeefULL`), with the `+`
    // between them surviving as whatever the fallback recogniser emits
    // (Unknown today; a punctuator after task 03-lex/08).
    let src = "3.14f + 0xdeadbeefULL";
    let pp_numbers: Vec<_> =
        tokenize(src).into_iter().filter(|t| matches!(t.kind, PpTokenKind::PpNumber(_))).collect();

    assert_eq!(pp_numbers.len(), 2, "expected exactly two PpNumber tokens, got {pp_numbers:?}");

    assert_eq!(pp_numbers[0].kind, PpTokenKind::PpNumber(PpNumberKind::Float));
    assert_eq!(pp_numbers[0].span.lo.0, 0);
    assert_eq!(pp_numbers[0].span.hi.0, 5);

    let hex_lo = src.find("0xdeadbeefULL").unwrap() as u32;
    assert_eq!(pp_numbers[1].kind, PpTokenKind::PpNumber(PpNumberKind::Integer));
    assert_eq!(pp_numbers[1].span.lo.0, hex_lo);
    assert_eq!(pp_numbers[1].span.hi.0, hex_lo + "0xdeadbeefULL".len() as u32);
}

// ── `.` disambiguation: `.digit` starts a pp-number, bare `.` does not ──

#[test]
fn dot_not_followed_by_digit_is_not_a_pp_number() {
    // `.x` must NOT be lexed as a pp-number — the grammar requires a
    // digit immediately after the leading `.`. The `.` falls through to
    // the catch-all Unknown arm (will become `Punct(Dot)` once task
    // 03-lex/08 lands), and `x` is an identifier.
    let src = ".x";
    let toks = non_ws(src);
    assert!(
        toks.iter().all(|t| !matches!(t.kind, PpTokenKind::PpNumber(_))),
        "`.x` must not produce a PpNumber: {toks:?}"
    );

    // The identifier `x` must still be recognised in full.
    let ident = toks.iter().find(|t| t.kind == PpTokenKind::Ident).expect("ident `x`");
    assert_eq!(ident.span.lo.0, 1);
    assert_eq!(ident.span.hi.0, 2);
}

#[test]
fn ellipsis_is_not_a_pp_number() {
    // `...` must not be swallowed by the pp-number recogniser — all
    // three dots must fall through to the punctuator/unknown arm.
    let src = "...";
    let toks = non_ws(src);
    assert!(
        toks.iter().all(|t| !matches!(t.kind, PpTokenKind::PpNumber(_))),
        "`...` must not produce a PpNumber: {toks:?}"
    );
}

#[test]
fn dot_followed_by_digit_via_line_splice_still_starts_pp_number() {
    // C99 phase-2 line splicing is transparent to the lexer: `.\<LF>42`
    // must lex exactly as `.42` — a single Float pp-number spanning the
    // physical bytes from the `.` through the final `2`.
    let src = ".\\\n42";
    let toks = non_ws(src);
    assert_eq!(toks.len(), 1, "expected one pp-number across the splice, got {toks:?}");
    assert_eq!(toks[0].kind, PpTokenKind::PpNumber(PpNumberKind::Float));
    assert_eq!(toks[0].span.lo.0, 0);
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

#[test]
fn line_splice_in_middle_of_pp_number_is_transparent() {
    // `1\<LF>234` must be one Integer pp-number whose physical span
    // covers the full input, splice bytes included.
    let src = "1\\\n234";
    let toks = non_ws(src);
    assert_eq!(toks.len(), 1, "expected one pp-number across the splice, got {toks:?}");
    assert_eq!(toks[0].kind, PpTokenKind::PpNumber(PpNumberKind::Integer));
    assert_eq!(toks[0].span.lo.0, 0);
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

// ── Span-partition invariant (lightweight fuzz seed) ────────────────

#[test]
fn span_partition_over_mixed_pp_numbers() {
    // The spans emitted for a string of whitespace-separated pp-numbers
    // must exactly cover their respective byte ranges — no overlaps,
    // no gaps. This is the "fuzz corpus spans partition the input"
    // acceptance bullet, checked over a small deterministic corpus.
    let src = "42 0xFF .5 1e10 0x1p0 0xdeadbeefULL 3.14e-10f";
    let toks = tokenize(src);

    // Non-ws tokens must all be PpNumbers in this corpus.
    let nums: Vec<_> = toks
        .iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();

    assert!(
        nums.iter().all(|t| matches!(t.kind, PpTokenKind::PpNumber(_))),
        "expected only PpNumber tokens outside whitespace, got {nums:?}"
    );

    // Spans must be in strict left-to-right order and non-overlapping.
    for pair in nums.windows(2) {
        assert!(
            pair[0].span.hi.0 <= pair[1].span.lo.0,
            "token spans must not overlap: {:?} then {:?}",
            pair[0],
            pair[1],
        );
    }

    // Every byte of the source must lie inside exactly one emitted
    // token span (whitespace runs are collapsed but their byte ranges
    // must still fall between consecutive token spans).
    let total_covered: u32 = toks.iter().map(|t| t.span.hi.0 - t.span.lo.0).sum::<u32>()
        + src.bytes().filter(|b| *b == b' ').count() as u32;
    assert_eq!(
        total_covered,
        src.len() as u32,
        "covered bytes + whitespace must equal source length",
    );
}

// ── Sanity: no panics on weird inputs ───────────────────────────────

#[test]
fn pp_number_recogniser_tolerates_invalid_sequences() {
    // pp-number is permissive by design; the recogniser must never
    // panic on inputs like `0xzzzULL`, `1.2.3`, `0x`, `1e`, `.0ep+`.
    // Every such input lexes to a single PpNumber whose span covers
    // the whole input (maximal munch). Validity is rechecked in phase
    // 05 — not here.
    let malformed = ["0xzzzULL", "1.2.3", "0x", "1e", ".0ep+", "1_000"];
    for src in malformed {
        let toks = non_ws(src);
        assert_eq!(toks.len(), 1, "src={src:?}: expected one pp-number, got {toks:?}");
        assert!(
            matches!(toks[0].kind, PpTokenKind::PpNumber(_)),
            "src={src:?}: expected PpNumber, got {:?}",
            toks[0].kind,
        );
        assert_eq!(toks[0].span.lo.0, 0);
        assert_eq!(toks[0].span.hi.0, src.len() as u32);
    }
}
