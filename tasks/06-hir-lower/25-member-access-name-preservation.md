> ✓ done — 2026-05-01

# 06-25: Preserve member-access names for typeck

**Phase:** 06-hir-lower    **Depends on:** 06-24    **Milestone:** M3 pre-codegen stabilization

## Goal
Stop lowering `a.b` / `a->b` to a permanent `field_index: 0`
placeholder. HIR must preserve enough information for typeck to
resolve the actual member and compute the correct result type before
CFG and LLVM codegen consume the access.

## Scope
- In: extend the HIR member-access representation so it carries the
  requested member name until semantic resolution.
- In: preserve source spans for the member token and the base
  expression.
- In: keep the existing resolved projection form for downstream CFG
  once typeck has resolved the member.
- Out: bitfield load/store layout; owned by 09-21.
- Out: anonymous-member recursive lookup semantics beyond the cases
  already represented by HIR record lowering.

## Deliverables
- HIR shape update for unresolved vs resolved member access, or an
  equivalent lossless encoding.
- HIR lowering changes for `ExprKind::Member` and `ExprKind::Arrow`.
- Regression tests showing `struct S { int a; int b; }; s.b` does not
  lose the name `b` before typeck.
- Documentation comment on the HIR enum explaining which phase resolves
  the member.

## Acceptance
- Source `struct S { int a; int b; }; int f(struct S s) { return s.b; }`
  lowers to HIR with the requested member name preserved before typeck.
- `p->b` preserves both the implicit dereference and the requested
  member name.
- No HIR expression for source member access hardcodes `field_index: 0`
  unless the source actually resolves to field 0 after typeck.

## References
- `crates/rcc_hir_lower/src/lib.rs`, current `Field { field_index: 0 }`
  placeholder.
- C99 §6.5.2.3 — structure and union members.
- Follow-up: 07-13 resolves the preserved member names.
