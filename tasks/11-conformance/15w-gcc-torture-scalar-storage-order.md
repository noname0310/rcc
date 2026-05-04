> ✓ done — 2026-05-04 — implemented GNU scalar_storage_order bit-field storage for 20230630-2

# 11-15w: gcc-torture scalar storage order attribute

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Decide and, if accepted, implement GNU `scalar_storage_order` for bit-field
storage.

## Scope
- In: `20230630-2`.
- Out: general endian cross-compilation support.

## Deliverables
- A policy note for `scalar_storage_order`.
- If implemented, layout/codegen tests proving reversed bit-field byte order on
  little-endian targets.

## Acceptance
- `20230630-2` is no longer an unexplained signal case.
- The decision is explicit because the feature is outside C99.

## References
- `docs/gcc-torture-signal-clusters.md`
