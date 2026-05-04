> ✓ done — 2026-05-04

# 08-26: goto into ordinary block scope

**Phase:** 08-cfg    **Depends on:** 08-17    **Milestone:** M6+

## Goal
Allow `goto` into an ordinary nested block when doing so does not enter the
scope of a variably modified object, and generate correct storage-live state
at the label.

## Trigger
- `c-testsuite::00199` panics with `goto into local scope` for a valid jump
  into a block containing ordinary automatic locals.
- `c-testsuite::00207` panics on the same invariant, while also containing a
  VLA case that must remain guarded by the C99 constraint.

## Scope
- In:
  - Distinguish ordinary automatic locals from VLA / variably modified locals
    in label-depth validation.
  - Permit gotos that enter only ordinary automatic scopes.
  - Ensure `StorageLive` is emitted or considered live for locals whose scope
    is entered at a label.
  - Keep diagnostics for illegal jumps into variably modified scopes.
- Out:
  - Non-C99 computed goto.

## Deliverables
- CFG tests for ordinary block-scope label jumps.
- CFG tests proving jumps into VLA scopes remain rejected cleanly, not by panic.
- c-testsuite regressions for `00199` and the non-VLA part of `00207`.

## Acceptance
- `c-testsuite::00199` compiles and executes successfully.
- `c-testsuite::00207` no longer panics; any remaining failure is a normal
  diagnostic or runtime mismatch with a follow-up owner.

## Result
- `c-testsuite::00199` and `c-testsuite::00207` both compile, execute, and
  match expected output under WSL LLVM 18.
- Full c-testsuite moved from 211 pass / 5 fail to 213 pass / 3 fail.

## References
- C99 §6.8.6.1
- `crates/rcc_cfg/src/build.rs`
