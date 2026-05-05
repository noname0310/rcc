# 04-22: multiline function-like macro invocation

> ✓ done — 2026-05-05

**Phase:** 04-preprocess    **Depends on:** 04-08, 04-14    **Milestone:** M6+

## Goal
Allow function-like macro invocations to collect arguments across source
line boundaries in normal text, as required by C99 preprocessing-token
semantics.

## Trigger
- zlib `trees.c` calls debug macros as:
  `Assert(expr,\n        "message");`.
- The old preprocessor expanded one non-directive logical line at a time,
  so the invocation was split before the closing `)` and the macro name and
  trailing argument leaked into the parser.

## Scope
- In:
  - Buffer consecutive active non-directive text lines before invoking the
    Prosser expander.
  - Flush the buffered text before directives or inactive conditional
    branches so directive side effects remain line-oriented.
  - Add a regression test for an empty function-like macro call split across
    a newline.
- Out:
  - Directive parsing across lines.
  - New macro syntax or GNU-only macro behavior.

## Acceptance
- [x] `#define Assert(cond,msg)` consumes `Assert(1,\n"msg")` fully.
- [x] Tokens after the multiline invocation remain in order.
- [x] Directive side effects still occur in source order.
- [x] zlib `trees.c` progresses past the debug macro parse errors.

## References
- C99 §5.1.1.2 translation phases
- C99 §6.10.3 macro replacement
- `real_world/projects/03-zlib/plan.md`
