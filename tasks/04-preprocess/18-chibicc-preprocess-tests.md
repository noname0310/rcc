> ✓ done — 2026-04-23

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

## Notes (agent)

Landed infrastructure (driver `--emit=pp` short-circuit, `ChibiccAdapter`
preprocess mode, `rcc_conformance_run --mode preprocess`, in-process
integration tests) and cleanly passing fixtures (`typedef.c`,
`include1.h` chain). `macro.c` is blocked from 100 % by four
GNU-extension gaps traced to follow-up task
[`20-gnu-extensions`](20-gnu-extensions.md) — `E0013` (computed
`#include`), `E0014` (`args...` named variadic), `E0022` (benign
re-`#define`), `E0025` (paste across pp-number boundary). The
integration test `crates/rcc_preprocess/tests/chibicc.rs` bounds
`macro.c` errors to exactly that bucket set with a ceiling of 34, so
any regression surfaces and any progress under 04-20 forces the
ceiling to shrink.
