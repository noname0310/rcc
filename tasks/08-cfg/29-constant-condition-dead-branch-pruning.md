> ✓ done — 2026-05-05 — implemented in commit

# 08-29: Constant-condition dead-branch pruning

**Phase:** 08-cfg    **Depends on:** 08-28    **Milestone:** M5

## Goal
Do not emit CFG for source branches that are unreachable because their
controlling expression is a compile-time constant false/true value.

## Scope
- In: fold side-effect-free integer/arithmetic constant conditions during CFG
  lowering, including `sizeof`/`__alignof__`, comparisons, logical operators,
  casts, and conversion wrappers inserted by typeck.
- In: preserve side effects for non-constant prefixes such as `f() && 0` while
  still proving the final condition false.
- In: prune untaken `if`/ternary arms and constant loop dispatches before LLVM
  codegen sees calls inside dead branches.
- Out: full optimization/DCE. This is a correctness-oriented CFG lowering rule,
  not an optimizer pipeline.

## Acceptance
- `if ((err != 0) && (sizeof("S_READ_ARC4RANDOM_C") == 1u)) disabled();`
  produces no CFG call to `disabled`.
- LibTomMath `s_mp_rand_platform.c` links at `-O0`; disabled platform random
  readers must not survive solely because `MP_HAS(...)` lowers to a constant
  false expression.
- `f() && 0` still lowers the call to `f`, but does not lower the RHS/body.
- Existing CFG verifier and source-pipeline edge tests remain green.

## References
- LibTomMath `MP_HAS` configuration idiom: dead code is hidden behind
  compile-time false `sizeof("...") == 1u` checks and must not leak undefined
  platform calls into `-O0` objects.
