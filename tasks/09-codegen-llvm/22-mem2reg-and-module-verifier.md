# 09-22: mem2reg and module verifier gate

**Phase:** 09-codegen-llvm    **Depends on:** 09-10, 09-12, 09-14, 09-15, 09-16    **Milestone:** M3

## Goal

Add a backend verification gate that proves emitted LLVM IR is valid and that
the non-SSA CFG strategy promotes clean scalar locals through mem2reg.

## Scope

- In: module verification after codegen, optional test-only mem2reg pass,
  instruction count assertions, and structured error text on failure.
- Out: full `-O2` pipeline; owned by driver/quality tasks.

## Deliverables

- `verify_module` helper called in tests and optionally debug builds.
- Tests for promotable locals and address-taken locals.

## Acceptance

- On `int f(int x){ int y=x+1; return y; }`, mem2reg leaves zero scalar local
  allocas.
- On `int *p=&y`, the address-taken alloca remains.

## References

- LLVM mem2reg pass
