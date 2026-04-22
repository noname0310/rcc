# 04-18: chibicc preprocessor tests

**Phase:** 04-preprocess    **Depends on:** 04-17    **Milestone:** M5

## Goal
Run the preprocessor portion of chibicc's `test/macro.c` (and
`test/typedef.c`, `test/include.c`) via the `ChibiccAdapter` with
`--emit=pp` mode — compare tokens against `cc -E` of the same file.

## Scope
- In: new conformance filter `ChibiccAdapter::run_preprocess_only`;
  differential comparison against host `cc -E`.
- Out: running the test binaries (that's M6 work).

## Deliverables
- Addition to `ChibiccAdapter` or a sibling adapter variant.
- KPI target: all chibicc preprocessor tests green at M5.

## Acceptance
- `cargo run --release --package rcc_conformance -- \
    --suite chibicc --mode preprocess`: 100 % pass.
- Updates `docs/conformance.md` row for chibicc preprocessor.

## References
- Task [`11-conformance/05-chibicc-preprocess.md`](../11-conformance/05-chibicc-preprocess.md).
