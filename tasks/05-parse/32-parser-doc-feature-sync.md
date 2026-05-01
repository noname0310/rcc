# 05-32: Parser docs and feature matrix sync

**Phase:** 05-parse    **Depends on:** 05-31    **Milestone:** M2.1

## Goal
Remove stale parser documentation and align xfail reasons with the
actual parser blockers so future agents do not fix the wrong layer.

## Scope
- In:
  - Update stale module docs in `rcc_parse` that still describe landed
    features as missing.
  - Add a parser feature matrix documenting C99-complete syntax,
    GNU/C11 syntax intentionally deferred, and syntax that is parsed
    but semantically checked later.
  - Correct parser-related xfail reasons in
    `third_party/testsuites/c-testsuite/xfail.toml`.
  - Cross-reference parser-surface tasks from later extension/runtime
    tasks without changing the read-only architecture plan.
- Out:
  - Implementing any missing syntax.

## Deliverables
- Updated comments/docs in `crates/rcc_parse/src`.
- `docs/parser-feature-matrix.md`.
- Corrected xfail reason strings for c-testsuite parser blockers.
- Any task-file scope notes needed to prevent duplicate parser work in
  phase 14 or phase 15.

## Acceptance
- No `rcc_parse` comment says block declarations, `for` declaration
  init, function definitions, K&R definitions, or typedef-name feedback
  are unimplemented.
- xfail entries for `00213`, `00214`, and `00216` name the real parser
  blockers: GNU statement expression, GNU builtin/type syntax, and GNU
  range designator as applicable.
- `rg "not yet implemented|deferred to task 05|same stub" crates/rcc_parse/src`
  returns no stale phase-05 references.

## References
- `crates/rcc_parse/src/stmt.rs` module docs.
- `crates/rcc_parse/src/decl.rs` module docs and typedef tests.
- `third_party/testsuites/c-testsuite/xfail.toml`.
