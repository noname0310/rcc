> ✓ done — 2026-05-03

# 09-21: Bitfield access codegen

**Phase:** 09-codegen-llvm    **Depends on:** 09-03, 09-09, 09-20    **Milestone:** M4

## Goal

Emit reads and writes of C struct bitfields using layout metadata and
shift/mask sequences over the containing storage unit.

## Scope

- In: signed/unsigned extraction, zero-width alignment fields, write masking,
  volatile bitfield loads/stores, and addressability restrictions.
- Out: implementation-defined packing beyond the SysV baseline.

## Deliverables

- Bitfield access helper used by place load/store.
- Host-cc differential fixtures for representative bitfield structs.

## Acceptance

- Reading a signed bitfield sign-extends to the declared integer type.
- Writing one bitfield does not modify neighboring bitfields.

## References

- C99 6.7.2.1
