> ✓ done — 2026-05-04

# 04-20: GNU preprocessor extensions

**Phase:** 04-preprocess    **Depends on:** 04-18    **Milestone:** M5+

## Goal
Close the four GNU / C99-strictness gaps surfaced by running chibicc's
`test/macro.c` under our preprocessor (task 04-18), so the fixture
reaches 0 diagnostics and the M5 KPI cell for chibicc preprocessor
tests lands at 100 %.

The gaps are tracked with stable error codes (the current, strict-C99
behaviour):

| code  | fixture shape                            | fix                                                  |
|-------|------------------------------------------|------------------------------------------------------|
| E0022 | `#define M 3` `#define M 4` (no `#undef`)| new `Options::gnu_permissive_redefinition`, warn-only|
| E0013 | `#include MACRO` / `#include M > `       | extend `directive::parse_directive` (§6.10.2p4)       |
| E0014 | `#define M(args...) …` (GNU variadic)    | new `Options::gnu_named_variadic`, relax param parse |
| E0025 | `CONCAT(4, .57)` → `4.57`                | new `Options::gnu_permissive_paste` / re-lexer relax |

## Scope
- In: the four individual relaxations below. Each must be individually
  toggleable via an `Options` flag (so the C99-strict default is
  preserved) and must leave existing `rcc_preprocess` unit tests
  unchanged when the flag is off.
- Out: any other GNU extension not listed above (e.g. `__attribute__`,
  `__extension__`, `##__VA_ARGS__` — the last is already handled by
  the existing `gnu_va_args_elision` flag).

### 04-20 (a) E0022 — benign redefinition without `#undef`

Add `Options::gnu_permissive_redefinition: bool` (default `false`).
When `true`, re-`#define`ing an identifier whose previous definition
has the same *kind* (object-like ↔ object-like, or function-like with
identical param arity + variadicity) should emit a `W00NN` warning
instead of the current `E0022` error. Differing-kind redefinitions
stay as `E0022`.

C99 §6.10.3p2 forbids the relaxed form; this mirrors gcc/clang's
permissive default while keeping strict mode for standard-conformance
runs.

### 04-20 (b) E0013 — computed `#include`

Extend `directive::parse_directive` to accept an `#include` whose
body is neither `"..."` nor `<...>`: per C99 §6.10.2p4 the preprocessor
must macro-expand the body first and then re-tokenise the result as a
header name. (This is actually C99, *not* a GNU extension — it was
parked under 04-18 as the shape `macro.c` exercises with `#define M13
"include3.h"` / `#include M13` and `#define M13 < include4.h` /
`#include M13 >`.) No new option flag; wire this on unconditionally.

Acceptance: chibicc's `#include M13` and `#include M13 >` cases
resolve against the fixture directory without E0013.

### 04-20 (c) E0014 — GNU named variadic `args...`

Add `Options::gnu_named_variadic: bool` (default `false`). When
`true`, `parse_params` additionally accepts a final `IDENT "..."`
token pair and treats it as a named variadic: uses of the identifier
(not `__VA_ARGS__`) in the replacement list get the full
variadic-argument substitution.

### 04-20 (d) E0025 — paste across pp-number boundary

Add `Options::gnu_permissive_paste: bool` (default `false`). When
`true`, relax the paste validator so that pasting a pp-number with
another pp-token produces a new pp-number as long as the
concatenation is itself a valid pp-number (e.g. `4` `##` `.57` →
`4.57`). The strict validator stays unchanged for ordinary-identifier
pastes; only the pp-number lane is loosened.

## Deliverables
- Four `Options` flags + their wiring through
  `rcc_session::Options` / `rcc_driver::options_from_cli` (CLI
  surface TBD — suggestion: `--gnu-permissive-redefinition`,
  `--gnu-permissive-paste`, `--gnu-named-variadic`).
- Four `rcc_preprocess` unit tests (one per relaxation) with both
  strict-off and permissive-on shapes.
- Update to `crates/rcc_preprocess/tests/chibicc.rs`: flip
  `chibicc_macro_c_runs_to_completion_with_bounded_gaps` to assert
  **zero** errors once the matching option is enabled in the test
  session; drop `MACRO_C_ERROR_CEILING`.
- Update `docs/conformance.md` chibicc preprocessor row: 2/2 pass
  (i.e. `macro.c` moves from `fail` to `pass`).

## Acceptance
- `cargo test -p rcc_preprocess --test chibicc`: all three tests
  pass with `macro.c` emitting 0 errors.
- `cargo run --release --package rcc_conformance -- --suite chibicc \
    --mode preprocess`: 2 / 2 pass.
- Turning every new option back to its C99-strict default must keep
  the phase-04 unit tests unchanged (regression guard).

## References
- Task [`18-chibicc-preprocess-tests`](18-chibicc-preprocess-tests.md) —
  the ceiling / bucket list this task is meant to dissolve.
- chibicc `test/macro.c` — primary acceptance fixture.
- C99 §6.10.2p4 (computed `#include`), §6.10.3p2 (redefinition),
  §6.10.3.3 (paste & pp-numbers).
- GCC Manual, "Variadic Macros" (`args...` form) and clang's
  equivalent `-Wgnu-variable-sized-type-not-at-end` family.
