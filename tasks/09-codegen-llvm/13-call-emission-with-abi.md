> ✓ done — 2026-05-02

# 09-13: Call emission with ABI lowering

**Phase:** 09-codegen-llvm    **Depends on:** 09-07, 09-08, 09-09, 09-12    **Milestone:** M3

## Goal

Emit `TerminatorKind::Call` using the same ABI classification used for function
declarations, including indirect calls and destination stores.

## Scope

- In: direct calls, function-pointer calls, scalar/aggregate args, `sret`, void
  calls, non-void destination stores, and normal target edge.
- Out: variadic builtins and `va_arg`; owned by 09-19.

## Deliverables

- `emit_call_terminator` helper.
- Tests for forward declarations, function pointers, void calls, scalar returns,
  and aggregate returns.

## Acceptance

- Caller and callee agree on LLVM function type and ABI lowering.
- A call with `target: None` emits a valid terminating instruction path.

## References

- `rcc_cfg::TerminatorKind::Call`
- SysV x86-64 ABI 3.2.3
