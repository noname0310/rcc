> ✓ done — 2026-05-02

# 09-10: Entry-block alloca and local materialization

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-09    **Milestone:** M3

## Goal

Materialize CFG locals as LLVM storage in the function entry block, including
parameter stores, temporary slots, and scope marker handling.

## Scope

- In: non-VLA local allocas before any branch, parameter initialization stores,
  `StorageLive`, `StorageDead`, and debug-friendly names.
- Out: dynamic VLA allocation; owned by 09-17.

## Deliverables

- `LocalMap` for `rcc_cfg::Local -> alloca/address`.
- Test proving all non-VLA allocas precede non-alloca entry instructions.

## Acceptance

- Trivial scalar locals are promotable by mem2reg after 09-22.
- Taking the address of a local keeps a real alloca.

## References

- LLVM Kaleidoscope Chapter 7
