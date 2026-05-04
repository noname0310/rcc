//! End-to-end preprocessor expansion matrix (task 04-17).
//!
//! A single table of `(source, expected)` rows drives
//! [`Preprocessor::run`] across every directive and macro-shape feature
//! introduced in phase 04. Each row is a self-contained translation
//! unit: the source bytes are loaded into a fresh [`Session`] with a
//! capturing emitter, [`Preprocessor::run`] is invoked, and the
//! returned pp-token stream is pretty-printed via [`pretty_tokens`]
//! into a space-separated spelling sequence for structural comparison.
//!
//! Rendering format
//! ----------------
//! [`pretty_tokens`] emits each non-whitespace pp-token's source text
//! (the byte slice covered by its span) separated by exactly one space.
//! This is unambiguous (unlike raw concatenation, which would collapse
//! `100` / `101` into the undifferentiated `100101`) and stable across
//! span-layout changes because it only depends on the *spellings* the
//! preprocessor pipes through. Synthesised tokens — e.g. from
//! stringize (`#x`) or paste (`a##b`) — already carry spans into
//! small synthetic source files created by the expander, so their
//! spellings materialise the same way as ordinary tokens.
//!
//! Failure reporting
//! -----------------
//! On mismatch [`run_table`] prints the row index, the original
//! source, the expected rendering, the actual rendering, and a
//! newline-joined character diff so regressions are easy to bisect
//! without re-running a debugger.
//!
//! The rows are organised by feature with section dividers; several
//! are ported (verbatim or as faithful analogues) from chibicc's
//! `test/macro.c` — see the `[chibicc]` annotations in the source.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_errors::{CaptureEmitter, Handler};
use rcc_lexer::{PpToken, PpTokenKind};
use rcc_preprocess::Preprocessor;
use rcc_session::{Options, Session};

/// Render the preprocessor's token output as a space-separated
/// sequence of token spellings, filtering out whitespace / newline /
/// EOF tokens that never carry semantic content.
///
/// Every non-filtered token contributes `session.source_map.file(
/// tok.span.file).src[tok.span.lo .. tok.span.hi]` — the raw source
/// slice the token points at. For synthesised tokens (stringize,
/// paste, `__LINE__`, `__FILE__`, predefined macros) the expander
/// registers small virtual files whose contents already equal the
/// intended spelling, so the same byte-slice projection works
/// uniformly.
fn pretty_tokens(session: &Session, tokens: &[PpToken]) -> String {
    let sm = session.source_map.read().unwrap();
    let mut parts: Vec<String> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        if matches!(tok.kind, PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof) {
            continue;
        }
        let src = &sm.file(tok.span.file).src;
        parts.push(src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string());
    }
    parts.join(" ")
}

/// Build a fresh session with a capturing emitter and load `src`
/// under the virtual path `<row>`. The returned [`FileId`] feeds
/// directly into [`Preprocessor::run`].
fn seed(src: &str) -> (Session, rcc_span::FileId, CaptureEmitter) {
    let cap = CaptureEmitter::new();
    let sess =
        Session::with_handler(Options::default(), Handler::with_emitter(Box::new(cap.clone())));
    let id = sess.source_map.write().unwrap().add_file(PathBuf::from("<row>"), Arc::from(src));
    (sess, id, cap)
}

/// Drive the table: for each `(source, expected)` pair run the full
/// preprocessor pipeline, pretty-print the expansion, and assert
/// equality. On failure, print a compact diagnostic pointing at the
/// offending row so the failure message carries enough context to
/// debug without rerunning the harness.
///
/// Each row is also asserted to produce *no* diagnostics — every row
/// in this table is well-formed on purpose; diagnostic-raising
/// shapes (redefinition, `#error`, out-of-range `#line`, …) are
/// covered by the unit tests in `rcc_preprocess::run_tests`.
fn run_table(rows: &[(&str, &str)]) {
    let mut failures: Vec<String> = Vec::new();
    for (i, (src, expected)) in rows.iter().enumerate() {
        let (mut sess, id, cap) = seed(src);
        let mut pp = Preprocessor::new(&mut sess);
        let out = pp.run(id);
        let actual = pretty_tokens(pp.session, &out);
        let diags = cap.diagnostics();
        if &actual != expected || !diags.is_empty() {
            let mut msg = String::new();
            msg.push_str(&format!("row #{i} FAILED\n"));
            msg.push_str("--- source ---\n");
            msg.push_str(src);
            if !src.ends_with('\n') {
                msg.push('\n');
            }
            msg.push_str(&format!("--- expected ---\n{expected}\n"));
            msg.push_str(&format!("--- actual   ---\n{actual}\n"));
            if !diags.is_empty() {
                msg.push_str("--- diagnostics ---\n");
                for d in &diags {
                    msg.push_str(&format!("  {:?} {:?} {}\n", d.level, d.code, d.message));
                }
            }
            failures.push(msg);
        }
    }
    if !failures.is_empty() {
        panic!(
            "{} of {} expansion rows failed:\n\n{}",
            failures.len(),
            rows.len(),
            failures.join("\n")
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// The table.
//
// Rows are grouped by feature. Keep each row single-source so the
// failure message (which prints the source verbatim) stays legible.
// Every expected string is the exact output of `pretty_tokens`:
// tokens joined by a single ASCII space, no leading/trailing padding.
// ─────────────────────────────────────────────────────────────────────

#[rustfmt::skip]
const ROWS: &[(&str, &str)] = &[
    // ── object-like macros ─────────────────────────────────────────
    // 00: bare object-like substitution.
    ("#define FOO 42\nFOO\n",                                    "42"),
    // 01: empty replacement list → identifier vanishes. [chibicc]
    ("#define EMPTY\nEMPTY x\n",                                 "x"),
    // 02: identifier with no matching macro passes through unchanged.
    ("not_a_macro\n",                                            "not_a_macro"),
    // 03: two-level object-like chain.
    ("#define A 1\n#define B A\nB\n",                            "1"),
    // 04: body containing several pp-tokens.
    ("#define PAIR 1 2\nPAIR\n",                                 "1 2"),
    // 05: self-referential object-like macro — the hide-set blocks
    //     a second expansion, so the original name is re-emitted.
    ("#define FOO FOO\nFOO\n",                                   "FOO"),
    // 06: mutually recursive object-like macros terminate on the
    //     original name per §6.10.3.4.
    ("#define A B\n#define B A\nA\n",                            "A"),
    // 07: redefinition to the identical replacement list is silent
    //     and the token still expands. [chibicc]
    ("#define PI 3\n#define PI 3\nPI\n",                         "3"),
    // 08: `#undef` removes the definition; the identifier is then
    //     treated as opaque.
    ("#define X 1\n#undef X\nX\n",                               "X"),
    // 09: float pp-number in replacement list is preserved.
    ("#define PI 3.14\nPI PI\n",                                 "3.14 3.14"),

    // ── function-like macros ───────────────────────────────────────
    // 10: simple two-argument substitution. [chibicc]
    ("#define ADD(x,y) x+y\nADD(3,4)\n",                         "3 + 4"),
    // 11: nested-parentheses argument collects as a single argument.
    ("#define F(x) x\nF((a,b))\n",                               "( a , b )"),
    // 12: ternary-style body (classic MAX).                       [chibicc]
    ("#define MAX(a,b) ((a)>(b)?(a):(b))\nMAX(1, 2)\n",          "( ( 1 ) > ( 2 ) ? ( 1 ) : ( 2 ) )"),
    // 13: zero-argument function-like macro.
    ("#define PI() 314\nPI()\n",                                 "314"),
    // 14: unparenthesised identifier — object-like view blocks call
    //     site interpretation; `F` acts as a bare identifier.
    ("#define F(x) x+x\nF\n",                                    "F"),
    // 15: outer macro expands its arg through a helper macro.
    ("#define F(x) x+x\n#define G(y) F(y)\nG(3)\n",              "3 + 3"),
    // 16: nested invocation: inner `M(1)` expands first, then outer.
    ("#define M(x) (x*2)\nM(M(1))\n",                            "( ( 1 * 2 ) * 2 )"),
    // 17: a multi-token replacement list with a literal comma.
    ("#define TRIPLE(x) x, x, x\nTRIPLE(7)\n",                   "7 , 7 , 7"),
    // 18: argument re-expansion across a deeper chain.
    ("#define A x\n#define F(a) a\nF(A)\n",                      "x"),
    // 19: intentionally empty actual arguments preserve separators exactly
    //     like tcc-tests2 71_macro_empty_arg.
    ("#define T(a,b,c) a b c\nT(1,+,2) T(+,,) T(,2,*) T(,7,) T(,,)\n",
                                                                 "1 + 2 + 2 * 7"),

    // ── stringize `#` ──────────────────────────────────────────────
    // 20: bare stringize — the identifier becomes a string literal.
    ("#define STR(x) #x\nSTR(hello)\n",                          "\"hello\""),
    // 21: multi-token argument is stringised with a single space
    //     between each token (§6.10.3.2p2). [chibicc]
    ("#define STR(x) #x\nSTR(a + b)\n",                          "\"a + b\""),
    // 22: backslash and quote inside the argument are escaped.
    ("#define STR(x) #x\nSTR(\"q\")\n",                          "\"\\\"q\\\"\""),
    // 23: empty argument stringises to an empty literal. [chibicc]
    ("#define STR(x) #x\nSTR()\n",                               "\"\""),
    // 24: interleaving stringised pieces.
    ("#define S(x) #x \" and \" #x\nS(yo)\n",                    "\"yo\" \" and \" \"yo\""),

    // ── paste `##` ─────────────────────────────────────────────────
    // 25: identifier paste.                                        [chibicc]
    ("#define CAT(a,b) a##b\nCAT(foo,bar)\n",                    "foobar"),
    // 26: paste of two pp-numbers.                                 [chibicc]
    ("#define CAT(a,b) a##b\nCAT(1,2)\n",                        "12"),
    // 27: paste with empty right operand re-emits the left operand.
    ("#define CAT(a,b) a##b\nCAT(foo,)\n",                       "foo"),
    // 28: paste with empty left operand re-emits the right operand.
    ("#define CAT(a,b) a##b\nCAT(,bar)\n",                       "bar"),
    // 29: paste result is rescanned for further expansion.
    ("#define FOO 42\n#define CAT(a,b) a##b\nCAT(F,OO)\n",       "42"),
    // 30: indirection layer to stringify-then-paste a value.       [chibicc]
    ("#define GLUE(a,b) a##b\n#define MKVAR(n) GLUE(var_,n)\nMKVAR(1)\n",
                                                                 "var_1"),

    // ── variadic `__VA_ARGS__` ─────────────────────────────────────
    // 31: bare variadic forwarding — commas are preserved.         [chibicc]
    ("#define V(...) __VA_ARGS__\nV(1,2,3)\n",                   "1 , 2 , 3"),
    // 32: named parameter plus variadic tail.                      [chibicc]
    ("#define L(x,...) x+__VA_ARGS__\nL(1,2,3)\n",               "1 + 2 , 3"),
    // 33: variadic passed through an indirection.                  [chibicc]
    ("#define CALL(f,...) f(__VA_ARGS__)\nCALL(foo,1,2)\n",      "foo ( 1 , 2 )"),
    // 34: zero variadic arguments → the parameter substitutes to
    //     an empty sequence (C99 §6.10.3p5 as interpreted by the
    //     default `gnu_va_args_elision = true` option; the token
    //     output drops the variadic parameter cleanly).
    ("#define L(x,...) x __VA_ARGS__\nL(1)\n",                   "1"),
    // 34: stringise the variadic parameter.
    ("#define VS(...) #__VA_ARGS__\nVS(a, b, c)\n",              "\"a, b, c\""),

    // ── `#if` / `#ifdef` / `#ifndef` / `#elif` / `#else` ───────────
    // 35: `#if 1` keeps the body.
    ("#if 1\nalive\n#endif\n",                                   "alive"),
    // 36: `#if 0` skips the body but emits the tail.
    ("#if 0\ndead\n#endif\ntail\n",                              "tail"),
    // 37: arithmetic in the controlling expression.
    ("#if 1+1==2\nok\n#endif\n",                                 "ok"),
    // 38: `#else` runs when the `#if` is false.
    ("#if 0\nlive\n#else\ndead\n#endif\n",                       "dead"),
    // 39: `#elif` ladder picks the first truthy branch.
    ("#if 0\nA\n#elif 1\nB\n#elif 1\nC\n#else\nD\n#endif\n",     "B"),
    // 40: `#ifdef` sees macros defined on a prior line.
    ("#define FOO 1\n#ifdef FOO\nyes\n#else\nno\n#endif\n",      "yes"),
    // 41: `#ifdef` without a definition takes the `#else` branch.
    ("#ifdef NOPE\nyes\n#else\nno\n#endif\n",                    "no"),
    // 42: `#ifndef` is the complement of `#ifdef`.
    ("#ifndef ABSENT\nyes\n#else\nno\n#endif\n",                 "yes"),
    // 43: `defined` operator usable inside `#if`.
    ("#define FOO 1\n#if defined FOO\na\n#else\nb\n#endif\n",    "a"),
    // 44: `defined(FOO)` parenthesised form.
    ("#if defined(NOPE)\na\n#else\nb\n#endif\n",                 "b"),
    // 45: nested `#if` — only the surviving innermost branch is
    //     emitted.
    ("#if 1\n#if 0\nin\n#else\nelse\n#endif\n#endif\n",          "else"),
    // 46: short-circuit: an outer `#if 0` keeps a syntactically
    //     valid but arithmetically broken inner `#if` dormant.
    ("#if 0\n#if 1/0\nbad\n#endif\n#endif\nok\n",                "ok"),

    // ── `#line` ────────────────────────────────────────────────────
    // 47: `#line N` renumbers the next physical line — `__LINE__`
    //     on that line reads the override value.
    ("#line 100\n__LINE__\n",                                    "100"),
    // 48: `#line N "f"` also retargets `__FILE__`.
    ("#line 100 \"foo.c\"\n__FILE__ __LINE__\n",                 "\"foo.c\" 100"),

    // ── predefined macros ──────────────────────────────────────────
    // 49: `__STDC__` is the integer constant 1 per §6.10.8.
    ("__STDC__\n",                                               "1"),
    // 50: `__STDC_VERSION__` identifies C99.
    ("__STDC_VERSION__\n",                                       "199901L"),
    // 51: hosted implementation bit.
    ("__STDC_HOSTED__\n",                                        "1"),
    // 52: `__LINE__` tracks physical lines relative to the file.
    ("\n\n__LINE__\n",                                           "3"),
];

#[test]
fn expansion_matrix() {
    // Safety net: the task mandates ≥ 40 rows; guard that invariant
    // alongside the data so accidental deletion is caught loudly.
    assert!(
        ROWS.len() >= 40,
        "expansion matrix must carry ≥ 40 rows (C99 §6.10 coverage grid, task 04-17); got {}",
        ROWS.len()
    );
    run_table(ROWS);
}
