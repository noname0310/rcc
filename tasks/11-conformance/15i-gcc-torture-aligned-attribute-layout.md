# 11-15i: gcc-torture aligned attribute layout

**Phase:** 11-conformance    **Depends on:** 11-15e    **Milestone:** M6

## Goal
Implement enough GNU `__attribute__((aligned(N)))` layout semantics to stop
miscompiling gcc-torture alignment tests when GNU attributes are explicitly
enabled.

## Scope
- In: type/declarator/record attributes that raise object or aggregate
  alignment, especially `__attribute__((aligned(alignment)))`.
- Out: unrelated attributes such as `packed`, `section`, `noreturn`, or target
  vector attributes unless a reduced alignment test requires a minimal
  interaction.

## Deliverables
- Preserve semantic aligned-attribute metadata from AST/HIR lowering into the
  shared layout service.
- Update LLVM global/local allocation alignment when the computed HIR layout
  requires stronger-than-natural alignment.
- Reduced tests for the 20010904 family and direct layout-service unit tests.

## Acceptance
- `gcc-torture::execute::20010904-1` passes with `-fgnu-attributes`.
- `gcc-torture::execute::20010904-2` passes with `-fgnu-attributes`.
- A reduced `typedef struct x { int a; int b; } __attribute__((aligned(32))) X;`
  fixture proves `sizeof`/stride and global object alignment are consistent.
- No xfail, skip, or warning suppression is used to hide the runtime abort.

## References
- `target/wsl/gcc-torture-15e-probe-after.json`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20010904-1.c`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20010904-2.c`
