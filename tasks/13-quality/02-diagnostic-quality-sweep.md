# 13-02: Diagnostic quality sweep

**Phase:** 13-quality    **Depends on:** 02-02    **Milestone:** M7

## Goal
Walk the entire error-code registry (`docs/error-codes.md`); for each
code, verify the message matches rustc/clang-quality guidelines:
- Primary label highlights the offending span.
- Secondary label illustrates the context.
- A `help:` line suggests a fix where possible.

## Scope
- In: add missing labels / notes / helps; update UI test snapshots.
- Out: translating messages (i18n; future).

## Deliverables
- Updated UI snapshots.
- `docs/error-codes.md` example snippets polished.

## Acceptance
- Rubric checklist applied to every E code.
- Random sample of 5 diagnostics reviewed by ≥ 2 team members.

## References
- rustc's "excellent diagnostics" convention.
