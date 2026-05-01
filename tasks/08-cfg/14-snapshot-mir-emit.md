# 08-14: `--emit=mir` pretty printer

> **Status:** done. CFG bodies now have a stable MIR-style text dump
> through `rcc_cfg::pretty::dump_body`, and the driver prints it for
> `--emit=mir`. Snapshot coverage lives under
> `crates/rcc_driver/tests/snapshots/mir/`.

**Phase:** 08-cfg    **Depends on:** 08-01    **Milestone:** M3

## Goal
Pretty-print a `Body` in a stable, readable textual format suitable
for `insta` snapshots. Rough shape inspired by rustc's MIR dumps.

## Scope
- In: `rcc_cfg::pretty::dump_body(&Body, &TyCtxt) -> String`;
  driver wires it up for `EmitKind::Mir`.
- Out: CFG analysis visualisers (future).

## Deliverables
- Pretty-printer.
- ≥ 5 snapshots under `crates/rcc_driver/tests/snapshots/mir/`.

## Acceptance
- Dump of a simple function is byte-stable across runs.
- `rcc --emit=mir hello.c` prints the dump unconditionally.

## References
- rustc MIR dumps.
