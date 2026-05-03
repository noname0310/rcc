> ✓ done — 2026-05-04

# 08-12: StorageLive / StorageDead

> **Status:** done. Block-scoped locals are bracketed by
> `StorageLive` / `StorageDead` via `BodyBuilder::enter_scope` /
> `exit_scope` / `storage_live`. `break` / `continue` / `return` flush
> every scope they jump out of via `emit_storage_deads_to_depth`.
> Goto-out-of-scope is a known limitation (the pre-pass does not
> currently record per-label scope depth).

**Phase:** 08-cfg    **Depends on:** 08-01    **Milestone:** M3

## Goal
Bracket every block-scoped local's lifetime with `StorageLive(local)`
on scope entry and `StorageDead(local)` on scope exit. Enables LLVM's
`mem2reg` and stack-slot reuse.

## Scope
- In: emit `StorageLive` right after the `alloca` (i.e. after the
  local's first use inside the entry block); emit `StorageDead` at
  every block exit (normal fall-through + `break` / `continue` /
  `return` / thrown-over via `goto`).
- Out: precise drop semantics (C has none; simpler than Rust).

## Deliverables
- Helpers baked into scope-entry / scope-exit callbacks inside
  `BodyBuilder`.
- Snapshot: `{ int x; { int y; } }` emits correct ordering.

## Acceptance
- For every `StorageLive(L)` there is exactly one `StorageDead(L)` on
  every reachable path.
- `continue` / `break` crossing scopes emit the intervening
  `StorageDead`s in reverse order.

## References
- rustc MIR StorageLive/Dead invariants.
