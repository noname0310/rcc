# 08-23: Type-aware CFG verifier

**Phase:** 08-cfg    **Depends on:** 07-19    **Milestone:** M3 pre-codegen stabilization

## Goal
Extend the release-profile CFG verifier from structural checks to the
minimal type-aware checks LLVM codegen relies on. If a body reaches
codegen, local slots, assignment rvalues, call destinations, and return
slot stores must be type-compatible.

## Scope
- In: verify `StatementKind::Assign` destination and rvalue types.
- In: verify return-slot stores match `Body::ret_ty`.
- In: verify `TerminatorKind::Call` destination type matches the
  callee's function return type when statically knowable.
- In: verify field and index projections are legal for the base type.
- In: keep the verifier conservative when a shape cannot be resolved,
  but report an error instead of silently accepting it.
- Out: full borrow/lifetime dataflow.

## Deliverables
- New `CfgErrorKind` variants for type mismatch and invalid projection.
- Tests for bad return slot, bad assignment, invalid field index, and
  invalid array/pointer index.
- Driver remains wired to report verifier errors before codegen.

## Acceptance
- A CFG body that stores `double` into an `int` return slot without a
  cast is rejected by `verify_body`.
- `Projection::Field(2)` on a record with only two fields is rejected.
- `Projection::Index` on a non-array/non-pointer base is rejected.
- All existing 08-cfg fixtures still pass.

## References
- `crates/rcc_cfg/src/verify.rs`.
- 07-19 typed-HIR verifier.
- 09-codegen-llvm input contract.
