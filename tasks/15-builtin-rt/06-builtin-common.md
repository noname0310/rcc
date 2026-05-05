> ✓ done — 2026-05-05

# 15-06: Common builtin functions

**Phase:** 15-builtin-rt    **Depends on:** 15-05, 05-35    **Milestone:** M6

## Goal
Recognize and lower a set of commonly used GCC/Clang builtin
functions: `__builtin_offsetof`, `__builtin_types_compatible_p`,
`__builtin_expect`, `__builtin_unreachable`,
`__builtin_constant_p`, and `__builtin_bswap{16,32,64}`.

## Scope
- In:
  - `__builtin_offsetof(type, member)` → constant-fold to byte
    offset using `LayoutCx`.
  - `__builtin_types_compatible_p(t1, t2)` → constant 0 or 1
    based on type compatibility check.
  - `__builtin_expect(expr, val)` → pass through `expr` (hint
    only; optionally emit LLVM `expect` intrinsic).
  - `__builtin_unreachable()` → LLVM `unreachable` instruction.
  - `__builtin_constant_p(expr)` → fold to 1 if `expr` is a
    compile-time constant, 0 otherwise.
  - `__builtin_bswap16/32/64(x)` → LLVM `bswap` intrinsic.
- Out: parser recognition of builtin forms whose arguments are
  type-names rather than expressions (task 05-35);
  `__builtin_popcount`, `__builtin_clz`, `__builtin_ctz`, math
  builtins (future task).

## Deliverables
- Builtin table entries for each function.
- Constant-folding or codegen lowering for each.
- Lowering for builtin AST nodes introduced by task 05-35 where
  ordinary call-expression lowering is not enough.
- Tests for each builtin.

## Parser Surface Dependency
Task 05-35 provides `ExprKind::BuiltinOffsetof` and
`ExprKind::BuiltinTypesCompatible` so phase 15 can lower type-name
arguments without rediscovering them from ordinary call syntax.

## Acceptance
- `__builtin_offsetof(struct S, field)` evaluates to the correct
  constant in a static assertion.
- `__builtin_unreachable()` emits LLVM `unreachable`.
- `__builtin_bswap32(0x01020304)` constant-folds to `0x04030201`.

## References
- GCC other builtins documentation.
- LLVM `@llvm.bswap`, `@llvm.expect` intrinsics.
