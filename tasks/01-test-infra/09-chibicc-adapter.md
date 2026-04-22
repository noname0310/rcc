> ✓ done — 2026-04-23

> ✓ done — 2026-04-23

# 01-09: Implement `ChibiccAdapter`

**Phase:** 01-test-infra    **Depends on:** 01-02, 01-07    **Milestone:** M0.5

## Goal
Run chibicc's tests the way its `Makefile` does: compile each `test/*.c`
together with `test/common.c`, link, execute, success if exit code is 0.

## Scope
- In: `discover()` returns every `test/*.c` except `common.c`;
  `run()` compiles the test file and `common.c` with `rcc`, links,
  executes, checks exit code.
- Out: breaking down chibicc tests by milestone subset (that is
  [`11-conformance/04-chibicc-stages-1-3.md`](../11-conformance/04-chibicc-stages-1-3.md) etc.).

## Deliverables
- `ChibiccAdapter::discover` + `ChibiccAdapter::run`.
- Fixture tests in `crates/rcc_conformance/tests/fixtures/chibicc-mini/`.

## Acceptance
- Discovery count equals `ls test/*.c | wc -l` minus 1 (common.c).
- Against real sources with the echo-exit stub rcc, `Skip` outcomes
  clearly state "compiler does not implement X".

## References
- chibicc's `Makefile` "test" target.
- Plan §9.3.
