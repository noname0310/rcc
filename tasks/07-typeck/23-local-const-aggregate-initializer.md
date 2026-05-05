# 07-23: local const aggregate initializer stores

> ✓ done — 2026-05-05

**Phase:** 07-typeck    **Depends on:** 07-20, 06-24    **Milestone:** M6+

## Goal
Separate lowered initializer stores from ordinary assignment expressions so
valid C99 declarations such as `const unsigned char s[] = "text";` do not
trip the non-modifiable-lvalue assignment diagnostic.

## Trigger
- The zlib real-world smoke probe rejects `const unsigned char input[] =
  "hello from rcc zlib smoke";` with E0080 before codegen.

## Scope
- In:
  - Add a HIR statement form for initializer stores.
  - Lower local aggregate/string/compound-literal initializer leaves to that
    statement instead of `Expr(Assign)`.
  - Type-check initializer stores with lvalue and RHS coercion checks but
    without C99 §6.5.16 assignment modifiability checks.
  - Keep ordinary assignment to `const` objects/elements rejected.
  - Update CFG lowering to consume the explicit initializer-store statement.
- Out:
  - File-scope static initializer folding.
  - New initializer syntax support.

## Acceptance
- [x] Local `const` array/string initializer stores do not emit E0080.
- [x] Ordinary assignment to `const` array elements still emits E0080.
- [x] CFG aggregate zero-fill recognition no longer relies on guessing a
  contiguous run of ordinary assignment expressions.
- [x] zlib smoke progresses past the local `const` string initializer.

## References
- C99 §6.7.8 initialization
- C99 §6.5.16 assignment operators
- `real_world/projects/03-zlib/plan.md`
