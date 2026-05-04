# 11-15j: gcc-torture aggregate by-value runtime

**Phase:** 11-conformance    **Depends on:** 11-15e    **Milestone:** M6

## Goal
Reduce and fix the aggregate copy / by-value calling bug exposed by
`gcc-torture::execute::20011113-1`.

## Scope
- In: struct assignment, `memcpy` into aggregate locals, by-value aggregate
  argument passing, and ABI lowering for aggregates around the 16-byte SysV
  direct/indirect boundary.
- Out: variadic aggregate ABI bugs and bit-field-only layout bugs unless the
  reduction proves they are the same root cause.

## Deliverables
- A reduced source fixture that clang exits with 0 and current rcc aborts.
- One concrete fix in CFG or LLVM codegen, or narrower follow-up tasks if the
  reduced failure splits into independent aggregate-copy and ABI-call bugs.
- IR or runtime regression tests showing aggregate contents survive
  `memcpy`, assignment, and by-value call boundaries.

## Acceptance
- `gcc-torture::execute::20011113-1` either passes or is split into narrower
  checked tasks with each remaining abort tied to a concrete root cause.
- The reduction includes a clang comparison, not only an rcc pass/fail result.
- No xfail, skip, or result masking is added.

## References
- `target/wsl/gcc-torture-15e-probe-after.json`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20011113-1.c`
