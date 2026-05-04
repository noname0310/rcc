> ✓ done — 2026-05-04

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

## Result
- Root cause was not the `U` by-value ABI path itself. `U` was correctly
  passed as `ptr byval(%rcc.record.28) align 8`.
- The abort came from a physical layout mismatch for `struct t`: LLVM lowered
  adjacent bit-fields as separate `i32` fields in the global type, while CFG
  member access used the shared HIR layout service and read `d` at byte offset
  8.
- Fixed LLVM record type lowering so structs containing bit-fields use explicit
  packed layout and coalesce adjacent bit-fields that share one storage unit.
- Fixed global aggregate constants so bit-field initializers are packed into
  that storage unit before following non-bitfield members are emitted.
- Added a reduced C99 e2e fixture,
  `crates/rcc_driver/tests/e2e/aggregate_bitfield_byval.c`, covering bit-field
  global layout, `memcpy`, struct assignment, and by-value aggregate calls.
- Verified the reduced fixture with host `cc -std=c99` and rcc; both exit 0.
- Verified WSL LLVM 18 conformance:
  `gcc-torture::execute::20011113-1` passes.
- Used no xfail, skip, or result masking.

## References
- `target/wsl/gcc-torture-15e-probe-after.json`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20011113-1.c`
