# 15a-12: C11 Conformance and Real-World Gates

**Phase:** 15a-c11-transition  
**Depends on:** 15a-11-c11-library-header-sweep  
**Milestone:** c11-transition

## Goal

Add repeatable C11 gates so the transition is enforced by tests before phase
16 hosted Linux work resumes.

## Scope

- In: focused parser/HIR/typeck/codegen tests for every C11 construct added in
  this phase.
- In: driver e2e fixture compiled with `-std=c11`.
- In: Toybox smoke must stop using `_Noreturn` macro substitution.
- In: conformance dashboard gets a C11 status row, even if external suites are
  still mostly C99.
- In: CI runs the C11 smoke where LLVM is available.
- Out: claiming full C11 conformance before atomics/threads optional behavior
  is explicitly classified.

## Acceptance

- [ ] `cargo test --workspace` includes C11 unit coverage.
- [ ] LLVM-enabled e2e compiles and runs at least one C11 program using
      `_Static_assert`, `_Alignof`, `_Generic`, and `_Noreturn`.
- [ ] Toybox wrapper no longer passes `-D_Noreturn=`.
- [ ] `real_world/projects/10-toybox/RESULTS.md` links any remaining blocker
      to a precise compiler task rather than the old `_Noreturn` workaround.
- [ ] `docs/conformance.md` distinguishes C99, C11-core, and deferred optional
      C11 library features.

## References

- WG14 N1570 public C11 draft.
- Existing `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md`.
