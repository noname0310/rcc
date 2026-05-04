# 11-16h: tcc-tests2 bit-field layout

**Phase:** 11-conformance    **Depends on:** 11-16g    **Milestone:** M6

## Goal
Close the remaining bit-field layout/runtime mismatches in tcc-tests2.

## Scope
- In: `tcc-tests2::95_bitfields` and `tcc-tests2::95_bitfields_ms`.
- Out: target ABIs other than the current Linux x86_64 WSL target unless the
  task explicitly adds a layout policy document for them.

## Deliverables
- Layout tests for default and Microsoft-compatible bit-field packing.
- Codegen tests for extracting/storing signed and unsigned bit-fields after
  the chosen layout policy.

## Acceptance
- Both target cases pass through tcc-tests2.
- Any `-mms-bitfields` or equivalent policy is explicit and feature-gated.

## References
- `target/wsl/tcc-tests2-16-final.json`
- `docs/gnu-scalar-storage-order.md`.
