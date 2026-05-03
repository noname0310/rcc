# 06-27: File-scope function prototypes

**Phase:** 06-hir-lower    **Depends on:** 06-26    **Milestone:** M4    **Size:** Medium

## Goal

Classify file-scope function declarations as HIR functions, not globals whose
object type happens to be `Ty::Func`.

## Problem

The full source pipeline currently fails for this valid C99 program:

```c
int callee(int);
int f(void) { return callee(7); }
```

HIR lowering creates the prototype as `DefKind::Global { ty: Ty::Func, ... }`.
LLVM codegen then tries to lower that function type as a global object type and
fails with `function is not a basic type`. The bug is in declaration
classification before typeck/CFG/codegen; codegen already handles function
declarations when the HIR kind is correct.

## Scope

- In: file-scope declaration lowering, prototype/function definition
  redeclaration merge, storage/linkage preservation, and regression fixtures.
- Out: K&R-style function definitions, inline semantics beyond the existing
  `06-13` contract, and ABI/codegen call emission changes.

## Deliverables

- `rcc_hir_lower` changes that detect assembled `Ty::Func` declarators at
  file scope and create/update `DefKind::Function { has_body: false, ... }`.
- Compatible redeclaration handling so a later function definition reuses or
  merges with the prototype instead of creating an incompatible global/object
  def.
- Tests for `extern`, `static`, and bare prototypes.
- A negative guard proving `int (*fp)(int);` remains `DefKind::Global` with a
  pointer-to-function object type.

## Acceptance

- The source pipeline accepts:

  ```c
  int callee(int);
  int f(void) { return callee(7); }
  ```

- HIR contains a function def for `callee`, not a global object def.
- `static int callee(int);` preserves internal linkage.
- `extern int callee(int);` preserves external linkage.
- `int (*fp)(int);` is still lowered as a global object.
- No `DefKind::Global` may carry a direct `Ty::Func` after HIR finalization;
  add a test or verifier assertion for this invariant.

## References

- C99 6.2.2 Linkages of identifiers
- C99 6.7 Declarations
- C99 6.9 External definitions
- Discovered while adding `09-23` full source-to-LLVM IR snapshots.
