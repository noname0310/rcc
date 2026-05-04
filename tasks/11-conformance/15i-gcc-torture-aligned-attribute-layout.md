> ✓ done — 2026-05-04

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

## Result
- Added `Record.align_override` to HIR and taught HIR lowering to preserve GNU
  `aligned(N)` attributes from record specs, declaration specs, and declarator
  attributes when `-fgnu-attributes` is enabled.
- Updated the shared HIR layout service so aligned records raise final
  alignment and tail padding instead of only recording natural member layout.
- Updated LLVM type lowering to emit packed explicit-layout structs with
  synthetic tail padding when HIR layout requires a record stride larger than
  LLVM's natural struct size; nested containing records inherit the explicit
  layout requirement.
- Updated global aggregate constants to include explicit padding entries for
  those LLVM record types.
- Verified WSL LLVM 18 conformance probes:
  `gcc-torture::execute::20010904-1` and
  `gcc-torture::execute::20010904-2` both pass.
- Used no xfail, skip, or warning suppression.

## References
- `target/wsl/gcc-torture-15e-probe-after.json`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20010904-1.c`
- `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/20010904-2.c`
