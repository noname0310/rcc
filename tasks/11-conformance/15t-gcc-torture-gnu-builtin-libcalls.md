# 11-15t: gcc-torture GNU builtin libcalls

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Make GNU builtin/libcall handling explicit instead of relying on accidental
host libc behavior.

## Scope
- In: `20021127-1`, `fprintf-chk-1`, `printf-chk-1`, `vfprintf-1`,
  `vfprintf-chk-1`, `vprintf-1`, `vprintf-chk-1`, `pr103255`.
- Out: ordinary C99 calls to declared libc functions that already link.

## Deliverables
- A builtin policy table: fold, alias to libc symbol, lower specially, or reject
  unless a GNU feature flag is enabled.
- Reductions for `llabs`, `__builtin_offsetof`, and fortify printf wrappers.
- Codegen/preprocess/typeck fixes or explicit follow-up tasks.

## Acceptance
- Each listed case has a specific builtin policy.
- No fortify case is marked xfail without a policy-backed reason.

## References
- `docs/gcc-torture-signal-clusters.md`
