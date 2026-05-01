# 08-21: Evaluation-order conformance policy

**Phase:** 08-cfg    **Depends on:** 08-20    **Milestone:** M3 stabilization

## Goal
Make CFG's chosen evaluation order explicit and keep conformance tests
from treating C unspecified behavior as a compiler bug. The current
call lowering evaluates callee and arguments left-to-right, which is a
valid choice but must be documented and filtered in differential tests.

## Scope
- In: document CFG evaluation order for call callee, call arguments,
  binary operands where this lowering chooses an order, and initializer
  leaf stores.
- In: add tests that assert the MIR order is stable for debugability.
- In: mark or filter external tests whose expected output depends on
  unspecified argument evaluation order.
- In: add comments near call lowering explaining the policy.
- Out: implementing alternative target-specific evaluation orders.

## Deliverables
- `docs/cfg-semantics.md` or an equivalent section in an existing docs
  file.
- Snapshot tests for call argument side effects using MIR order only,
  not runtime expected output.
- Conformance adapter metadata for skipping/demoting unspecified-order
  fixtures.

## Acceptance
- There is one documented policy for evaluation order.
- Differential tests do not fail solely because host `cc` chooses a
  different order for unspecified side effects.
- `cargo test -p rcc_cfg eval_order` passes.

## References
- C99 §6.5 expression evaluation.
- `crates/rcc_cfg/src/lower.rs` call lowering.
- `crates/rcc_conformance` outcome classification.
