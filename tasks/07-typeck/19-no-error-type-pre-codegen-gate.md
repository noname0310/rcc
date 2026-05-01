# 07-19: No-error-type pre-codegen gate

**Phase:** 07-typeck    **Depends on:** 07-18    **Milestone:** M3/M6 release gate

## Goal
Add a crate-level invariant check that a clean typeck pass leaves no
reachable `Ty::Error`, unresolved placeholder, or untyped expression in
HIR. This prevents subtle semantic holes from becoming invalid LLVM IR.

## Scope
- In: verifier over every function body, top-level global, typedef,
  record field, enum repr, initializer leaf, and string-literal def.
- In: distinguish "error already emitted" from "silent placeholder".
- In: run this verifier in tests and in the driver before CFG.
- Out: dataflow checks; owned by CFG verifier tasks.

## Deliverables
- `rcc_typeck::verify_typed_hir(...)` or equivalent.
- Driver integration after `check()` and before `build_bodies()`.
- Regression fixture with a deliberately unsupported expression proving
  the verifier catches it.

## Acceptance
- A successful `rcc --emit=mir` run cannot contain `Ty::Error` in any
  reachable HIR expression.
- Unsupported-but-parsed extension stubs such as builtin placeholder
  paths are either diagnosed or explicitly feature-gated before this
  verifier passes.
- The verifier message points at the source span that produced the
  placeholder.

## References
- Architecture invariant: no synthetic placeholder should cross phase
  boundaries silently.
- 06-23 HIR placeholder regression gate.
