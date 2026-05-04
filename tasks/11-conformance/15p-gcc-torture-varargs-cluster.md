# 11-15p: gcc-torture varargs cluster

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Fix remaining SysV `va_list` runtime behavior exposed by gcc-torture.

## Scope
- In: `pr64979`, `va-arg-21`, `va-arg-5`, `va-arg-6`.
- Out: fortify `vprintf` wrappers and vector varargs.

## Deliverables
- Reduced fixtures for `va_list *`, copied `va_list`, aggregate arguments, and
  mixed integer/double overflow-area reads.
- ABI/codegen fixes for cases proven to be C99 varargs bugs.

## Acceptance
- At least one reduced fixture per listed case exists.
- Passing cases are verified by WSL execution, not just IR shape checks.

## References
- `docs/gcc-torture-signal-clusters.md`
