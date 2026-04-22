# 10-01: `--emit=` stage dumps

**Phase:** 10-driver    **Depends on:** 03-13, 05-28, 08-14, 09-15    **Milestone:** M3

## Goal
Connect every `EmitKind` to a pretty-printer already implemented in
the relevant crate. When multiple emit kinds are requested, write each
to `<output>.<stage>` (or stdout when single + no output).

## Scope
- In: pipeline branch per stage; `pretty` submodules on lexer /
  parser / cfg / codegen already exist; HIR pretty-printer added
  here if missing.
- Out: machine-readable JSON dumps (future task).

## Deliverables
- `pipeline.rs` dispatching to pretty-printers.
- Smoke test for each `EmitKind` value.

## Acceptance
- `rcc hello.c --emit=tokens --emit=ast -o out` produces `out.tokens`
  and `out.ast`.

## References
- Skeleton driver's existing stubs.
