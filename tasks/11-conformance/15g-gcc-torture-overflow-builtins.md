# 11-15g: gcc-torture overflow builtin semantics

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15b    **Milestone:** M6

## Goal
Implement the GCC arithmetic overflow builtins exercised by gcc-torture without
reducing them to undeclared-name compatibility aliases.

## Scope
- In: `__builtin_add_overflow`, `__builtin_mul_overflow`, and
  `__builtin_mul_overflow_p` cases currently surfaced by full gcc-torture.
- In: type checking of pointer result operands, signed/unsigned overflow
  semantics, and LLVM lowering to checked arithmetic intrinsics or equivalent
  compare/store sequences.
- Out: arbitrary vector builtins and unrelated GCC builtin surface area.

## Deliverables
- HIR/typeck representation for overflow builtins that preserves the result
  pointer write and boolean overflow result.
- CFG/codegen lowering with tests for signed and unsigned int/long cases.
- gcc-torture representative runs for `pr64006`, `pr68381`, `pr71554`, and
  `pr85095` or a narrower bug split if one of those exposes an unrelated
  compiler defect.

## Acceptance
- `__builtin_mul_overflow(a, b, &out)` stores the wrapped product and returns
  the correct overflow boolean for signed and unsigned integer operands.
- `__builtin_add_overflow` has equivalent add semantics.
- `__builtin_mul_overflow_p` folds/evaluates to the same overflow boolean
  without requiring a result pointer.
- No xfail/skip is added for these cases.

## References
- `target/wsl/gcc-torture-full-15f-final.json`

## Result
- Added dedicated HIR/CFG nodes for GCC checked-overflow builtins instead of
  treating them as ordinary undeclared calls.
- Type checking validates integer operands and result-pointer/probe result
  types while preserving the original signed/unsigned operand types.
- LLVM lowering evaluates in a widened integer type, stores the wrapped result
  for the pointer form, and returns the round-trip mismatch as `int`.
- WSL probes passed:
  `pr64006`, `pr68381`, `pr71554`, `pr85095`, `pr105984`.
- Full WSL gcc-torture result:
  1176 pass / 474 fail / 0 xfail / 0 skip.
