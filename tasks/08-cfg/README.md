# 08-cfg

**Goal of the phase.** Build one `rcc_cfg::Body` per function from
typed HIR. Control flow (`if/while/for/do/switch/goto/break/continue`),
short-circuit operators, VLAs, and compound initializers all get
lowered to `BasicBlock` + `Terminator` sequences. Non-SSA —
`alloca + load/store` everywhere.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-body-builder.md`](01-body-builder.md) | `BodyBuilder` API and invariants. |
| 02 | [`02-local-allocation.md`](02-local-allocation.md) | Parameters → locals 0..N; return slot. |
| 03 | [`03-expr-to-rvalue.md`](03-expr-to-rvalue.md) | Expression lowering core. |
| 04 | [`04-place-projections.md`](04-place-projections.md) | Deref / Field / Index. |
| 05 | [`05-short-circuit-lowering.md`](05-short-circuit-lowering.md) | `&&`, `||`, `?:`. |
| 06 | [`06-if-else-lowering.md`](06-if-else-lowering.md) | |
| 07 | [`07-loop-lowering.md`](07-loop-lowering.md) | while / do / for. |
| 08 | [`08-switch-lowering.md`](08-switch-lowering.md) | `SwitchInt` + default. |
| 09 | [`09-goto-label-fixup.md`](09-goto-label-fixup.md) | Forward-goto patch. |
| 10 | [`10-call-lowering.md`](10-call-lowering.md) | `Call` terminator. |
| 11 | [`11-init-lowering.md`](11-init-lowering.md) | Aggregate initializer plan. |
| 12 | [`12-storage-live-dead.md`](12-storage-live-dead.md) | Scope-bounded liveness. |
| 13 | [`13-vla-lowering.md`](13-vla-lowering.md) | Variable-length arrays. |
| 14 | [`14-snapshot-mir-emit.md`](14-snapshot-mir-emit.md) | `--emit=mir` dumps. |
| 15 | [`15-unit-tests.md`](15-unit-tests.md) | MIR snapshot fixture table. |

## Exit criteria

- Every well-typed HIR function produces a `Body` where every
  `BasicBlock` has a `Terminator` and every `Place` targets a live
  `Local`.
- `--emit=mir hello.c` matches the checked-in snapshot.
