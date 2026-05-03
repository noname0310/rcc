> ✓ done — 2026-05-04

# 08-25: Global object lvalue contract

**Phase:** 08-cfg    **Depends on:** 08-24, 06-27    **Milestone:** M4    **Size:** Medium

## Goal

Preserve the semantic difference between a global object's address, a global
object lvalue-to-rvalue load, and a function designator before LLVM codegen.

## Problem

The full pipeline currently emits suspicious LLVM IR for:

```c
static int x = 5;
int f(void) { return x; }
```

Instead of loading the integer value from `@x`, codegen receives/derives a value
that causes the pointer `@x` to be stored into an integer return slot. That means
the CFG/codegen boundary is not explicit enough about whether `DefRef(x)` is an
object place, an address value, or an already-loaded rvalue.

## Scope

- In: CFG operand/place contract for global objects, verifier checks, source
  pipeline fixtures, and the minimum codegen adaptation needed to satisfy the
  contract.
- Out: function prototype classification; that belongs to `06-27`.
- Out: new ABI support or global initializer semantics beyond object read/write.

## Deliverables

- A documented CFG invariant for global objects:
  - object use in value context lowers through a load from a global place,
  - address-of use lowers to an address value,
  - function designators remain callable/addressable function values and are not
    treated as object loads.
- Type-aware verifier checks that reject storing a pointer value into a scalar
  slot unless the destination type is pointer-compatible.
- Full source pipeline fixtures covering global object read/address cases.
- LLVM IR snapshot or FileCheck coverage after `09-24`/`09-25` is available.

## Acceptance

- `static int x = 5; int f(void) { return x; }` lowers to a CFG/codegen path
  that loads from `@x`, never stores `ptr @x` into an `i32` slot.
- `static int x; int *f(void) { return &x; }` remains address-based.
- `static int x; int f(void) { return *&x; }` loads the object value.
- `int f(void) { return 1; } int (*g(void))(void) { return f; }` treats `f` as
  a function designator/pointer value, not a global object load.
- The CFG verifier catches the old pointer-into-scalar return-slot shape before
  LLVM codegen runs.

## References

- C99 6.3.2.1 Lvalues, arrays, and function designators
- C99 6.5.3.2 Address and indirection operators
- `tasks/09-codegen-llvm/23-llvm-ir-snapshots.md`
