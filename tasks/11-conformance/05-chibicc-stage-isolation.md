# 11-05: chibicc stage isolation

**Phase:** 11-conformance    **Depends on:** 01-09, 10-17    **Milestone:** M2

## Goal
Make the chibicc adapter capable of running a true stage-focused slice
without counting later-stage fixtures or compiling `test/common` with `rcc`.

## Scope
- In:
  - Add a conformance mode for the stage-1..3 slice.
  - Discover only `arith.c`, `control.c`, and `function.c`.
  - Do not compile upstream `test/common` with `rcc` in this mode; it uses
    much later features than the stage slice.
  - Provide an explicit support policy: either a minimal stage helper compiled
    by host `cc`, or a generated support object whose source contains only the
    `assert` helper needed by the selected fixtures.
  - Ensure the report identifies each selected fixture as a separate TU.
- Out:
  - Making the selected TUs pass.
  - GNU statement-expression semantics, case ranges, computed goto, varargs,
    aggregate ABI, and runtime library fidelity.

## Deliverables
- `ChibiccMode::Stages1To3` or equivalent CLI mode.
- Adapter tests proving discovery returns exactly `arith`, `control`, and
  `function`.
- Support-helper test proving the mode does not invoke `rcc` on
  `test/common`.
- Dashboard/report wording updated to explain that this mode is stage-isolated.

## Acceptance
- `rcc_conformance_run --suite chibicc --mode <stage-1-3>` reports exactly
  three cases.
- Later chibicc fixtures are not reported as pass/fail/xfail/skip for this
  mode.
- A failing selected TU is reported as a real failure, not hidden by xfail.
- The mode can be run on CI without requiring later chibicc features from
  `test/common`.

## Rationale
The old combined task said M2 should pass chibicc `arith.c`, `control.c`, and
`function.c`, but those upstream fixtures are not stage-isolated under the
current adapter:

- `arith.c` uses GNU binary integer literals (`0b...`) and 41 GNU statement
  expressions.
- `control.c` uses 54 GNU statement expressions, GNU case ranges
  (`case 0 ... 5:`), and GNU computed goto (`&&label`, `goto *p`).
- `function.c` uses 29 GNU statement expressions and substantially later
  ABI/runtime features (varargs, float/double calls, struct arguments and
  returns, `__func__`, etc.).
- `ChibiccAdapter::Compile` currently compiles `test/common` with `rcc` for
  every fixture. That helper itself uses later-stage features such as
  varargs, aggregate returns, compound literals, and record initializers.

This task isolates the test slice. Later tasks make each selected TU green.
