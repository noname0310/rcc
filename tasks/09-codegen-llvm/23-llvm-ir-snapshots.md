# 09-23: LLVM IR snapshots

**Phase:** 09-codegen-llvm    **Depends on:** 09-22    **Milestone:** M3

## Goal

Add stable snapshot tests for `--emit=llvm-ir` so codegen regressions are
visible before external conformance runs fail.

## Scope

- In: deterministic naming, normalization of unstable IDs, small C fixtures,
  and snapshots for function, branch, call, global, aggregate, and VLA cases.
- Out: FileCheck-style semantic assertions; owned by 09-24.

## Deliverables

- Snapshot harness in `rcc_codegen_llvm` or driver tests.
- Initial fixture set covering the M3/M4 backend surface.

## Acceptance

- Snapshots are stable across two consecutive test runs.
- Snapshot updates require explicit review and are not hidden behind broad globs.

## References

- `insta`
