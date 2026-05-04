> ✓ done — 2026-05-04

# 04-21: macro-expanded `#line` directive

**Phase:** 04-preprocess    **Depends on:** 04-15, 04-20    **Milestone:** M6+

## Goal
Implement C99 `#line` handling after macro expansion so a `#line` directive
whose line number or filename is provided by a macro is accepted and reflected
in subsequent source locations.

## Trigger
- `c-testsuite::00152` is currently xfailed as
  "macro-expanded `#line` directive is not fully implemented".

## Scope
- In:
  - Expand the pp-token sequence after `#line` before interpreting it.
  - Accept a macro-expanded decimal line number.
  - Accept a macro-expanded optional string-literal file name.
  - Preserve existing diagnostics for malformed expanded directive bodies.
  - Remove the `c-testsuite::00152` xfail once the TU passes.
- Out:
  - Non-C99 line-marker extensions such as GCC `# 12 "file" 3 4`.

## Deliverables
- Preprocessor unit tests for object-like and function-like macros producing
  `#line` operands.
- Driver/conformance regression for `c-testsuite::00152`.
- Updated xfail list.

## Acceptance
- `c-testsuite::00152` is `pass`, not `xfail`.
- Existing strict `#line` tests still pass.

## References
- C99 §6.10.4
- `third_party/testsuites/c-testsuite/tests/single-exec/00152.c`
