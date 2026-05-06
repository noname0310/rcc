> ✓ done — 2026-05-06

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

- [x] `cargo test --workspace` includes C11 unit coverage.
- [x] LLVM-enabled e2e compiles and runs at least one C11 program using
      `_Static_assert`, `_Alignof`, `_Generic`, and `_Noreturn`.
- [x] Toybox wrapper no longer passes `-D_Noreturn=`.
- [x] `real_world/projects/10-toybox/RESULTS.md` links any remaining blocker
      to a precise compiler task rather than the old `_Noreturn` workaround.
- [x] `docs/conformance.md` distinguishes C99, C11-core, and deferred optional
      C11 library features.

## Completion Notes

- Added a Linux LLVM e2e fixture in `rcc_driver` that compiles, links, and runs
  a C11 program using `_Static_assert`, `_Alignof`, `_Generic`, and
  `_Noreturn`.
- Updated `docs/conformance.md` with distinct C99, C11-core, and deferred C11
  hosted-library classifications.
- Reran the Toybox applet smoke without `_Noreturn` macro substitution.
  `_Noreturn` and `sigjmp_buf` no longer appear in the failure log; remaining
  blockers are hosted Linux/POSIX surface gaps tracked by
  `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md`.

## Verification

```sh
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p rcc_driver c11_core_constructs_compile_link_and_run --test e2e
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  cargo test -p rcc_driver --features rcc_codegen_llvm/llvm \
    c11_core_constructs_compile_link_and_run --test e2e -- --nocapture
NO_COLOR=1 LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh
```

## References

- WG14 N1570 public C11 draft.
- Existing `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md`.
